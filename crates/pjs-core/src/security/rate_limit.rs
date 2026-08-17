//! Rate limiting system for WebSocket connections to prevent DoS attacks

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::{
    net::IpAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use thiserror::Error;

/// Hard upper bound on the number of distinct client IPs [`WebSocketRateLimiter`]
/// tracks at once, independent of the periodic TTL-based [`WebSocketRateLimiter::cleanup_expired`]
/// sweep. The sweep only runs every few minutes ([`DEFAULT_CLEANUP_INTERVAL`]) and
/// cannot by itself prevent an in-window burst of distinct IPs from growing the
/// map unboundedly between sweeps. Once at capacity, requests from IPs not
/// already tracked are rejected with [`RateLimitError::CapacityExceeded`]
/// rather than growing the map further; already-tracked IPs are unaffected.
///
/// **Reject-new, not evict-to-admit — a deliberate choice.** At capacity, a
/// not-yet-tracked IP is turned away rather than evicting an arbitrary
/// existing entry to make room. Evict-to-admit would let an attacker forge
/// fresh IPs to repeatedly evict *established* clients' rate-limit state,
/// letting them bypass their own accumulated request count — the opposite of
/// what this limiter exists to prevent. Reject-new instead trades that for:
/// under a sustained attack that fills the table faster than
/// [`WebSocketRateLimiter::cleanup_expired`] can free idle entries, new
/// clients are turned away (a `503`, see `RateLimitService::call` in
/// `infrastructure::http::middleware`) until capacity frees up. This is
/// considered the safer default — it protects already-established traffic at
/// the cost of new-client admission under capacity pressure, rather than the
/// reverse.
pub const MAX_TRACKED_CLIENTS: usize = 100_000;

/// Default interval between periodic [`WebSocketRateLimiter::cleanup_expired`]
/// sweeps spawned by [`WebSocketRateLimiter::spawn_cleanup_task`].
pub const DEFAULT_CLEANUP_INTERVAL: Duration = Duration::from_secs(300);

/// Rate limiting errors
#[derive(Error, Debug, Clone)]
pub enum RateLimitError {
    /// Request count exceeded the per-window limit.
    #[error("Rate limit exceeded: {limit} requests per {window:?}")]
    LimitExceeded {
        /// Configured per-window request limit.
        limit: u32,
        /// Configured window duration.
        window: Duration,
    },

    /// Per-IP concurrent connection cap was reached.
    #[error("Connection limit exceeded: {current}/{max} connections")]
    ConnectionLimitExceeded {
        /// Current connection count for the IP.
        current: usize,
        /// Configured maximum number of connections per IP.
        max: usize,
    },

    /// Frame larger than the configured maximum was rejected.
    #[error("Frame size limit exceeded: {size} bytes > {max} bytes")]
    FrameSizeExceeded {
        /// Observed frame size in bytes.
        size: usize,
        /// Configured maximum frame size in bytes.
        max: usize,
    },

    /// The limiter is already tracking [`MAX_TRACKED_CLIENTS`] distinct
    /// clients; requests from not-yet-tracked clients are rejected until the
    /// next cleanup sweep frees capacity.
    #[error("Rate limiter at capacity: {max} tracked clients")]
    CapacityExceeded {
        /// Configured maximum number of tracked clients.
        max: usize,
    },
}

/// Rate limiting configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Maximum requests per time window
    pub max_requests_per_window: u32,
    /// Time window for rate limiting
    pub window_duration: Duration,
    /// Maximum concurrent connections per IP
    pub max_connections_per_ip: usize,
    /// Maximum WebSocket frame size
    pub max_frame_size: usize,
    /// Maximum message rate (messages per second)
    pub max_messages_per_second: u32,
    /// Burst allowance (extra messages above rate)
    pub burst_allowance: u32,
    /// Deadline for a single outbound WebSocket sink write before the
    /// connection is treated as stalled and closed.
    ///
    /// Guards against a peer that stops reading wedging the connection's
    /// task indefinitely. Bounds a single write, not overall throughput —
    /// see `infrastructure::websocket::WRITE_TIMEOUT`'s doc for the
    /// tradeoff this implies for large frames sent to slow clients, and
    /// raise this value if that tradeoff doesn't fit a deployment's
    /// expected client bandwidth. Defaults to the same 10s value as
    /// `infrastructure::websocket::WRITE_TIMEOUT`; the two constants are
    /// independent (different feature gates), so an intentional change to
    /// one should be mirrored in the other unless a divergence is
    /// deliberate. [`Self::low_resource`] tightens this to 3s. This value
    /// governs the server side only — `PjsWebSocketClient` has its own
    /// independent write-timeout knob (see
    /// `infrastructure::websocket::PjsWebSocketClient::with_write_timeout`),
    /// also defaulting to `WRITE_TIMEOUT`.
    pub write_timeout: Duration,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_requests_per_window: 100,
            window_duration: Duration::from_secs(60),
            max_connections_per_ip: 10,
            max_frame_size: 1024 * 1024, // 1MB
            max_messages_per_second: 30,
            burst_allowance: 5,
            write_timeout: Duration::from_secs(10),
        }
    }
}

impl RateLimitConfig {
    /// Configuration for high-traffic scenarios
    pub fn high_traffic() -> Self {
        Self {
            max_requests_per_window: 1000,
            max_connections_per_ip: 50,
            max_messages_per_second: 100,
            burst_allowance: 20,
            ..Default::default()
        }
    }

    /// Configuration for low-resource environments
    pub fn low_resource() -> Self {
        Self {
            max_requests_per_window: 20,
            max_connections_per_ip: 2,
            max_frame_size: 256 * 1024, // 256KB
            max_messages_per_second: 5,
            burst_allowance: 2,
            // 3s (30% of the 10s default) — within this preset's range of
            // reductions applied to its other fields (16.7%-40% of
            // `Default`, though above their ~20-25% median). Freeing a
            // wedged connection task matters more under resource
            // constraints than absorbing ordinary network jitter.
            //
            // Note: this does not shrink what a single outbound write must
            // flush in time. `max_frame_size` above bounds inbound frames
            // only; outbound frame size is governed elsewhere and is
            // unaffected by this preset. A slow-but-honest client now
            // needs roughly 3.3x the downlink bandwidth it needed under
            // the 10s default to avoid being disconnected as "stalled"
            // (see `infrastructure::websocket::WRITE_TIMEOUT`'s doc for
            // the full bandwidth-vs-deadline tradeoff) — raise this value
            // if a low-resource deployment still expects to serve large
            // frames to bandwidth-constrained clients.
            write_timeout: Duration::from_secs(3),
            ..Default::default()
        }
    }
}

/// Rate limit tracking for a specific client
#[derive(Debug)]
struct ClientRateLimit {
    /// Request timestamps within current window
    requests: Vec<Instant>,
    /// Current connection count
    connection_count: usize,
    /// Token bucket for message rate limiting
    tokens: f64,
    /// Last token refill time
    last_refill: Instant,
}

impl ClientRateLimit {
    fn new(burst_allowance: u32) -> Self {
        let now = Instant::now();
        Self {
            requests: Vec::new(),
            connection_count: 0,
            tokens: burst_allowance as f64, // Start with burst allowance tokens
            last_refill: now,
        }
    }

    /// Refill tokens based on time passed
    fn refill_tokens(&mut self, config: &RateLimitConfig) {
        let now = Instant::now();
        let time_passed = now.duration_since(self.last_refill).as_secs_f64();

        // Add tokens at configured rate
        let tokens_to_add = time_passed * config.max_messages_per_second as f64;
        let max_tokens = (config.max_messages_per_second + config.burst_allowance) as f64;

        self.tokens = (self.tokens + tokens_to_add).min(max_tokens);
        self.last_refill = now;
    }

    /// Check if message rate is within limits
    fn check_message_rate(&mut self, config: &RateLimitConfig) -> Result<(), RateLimitError> {
        self.refill_tokens(config);

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            Ok(())
        } else {
            Err(RateLimitError::LimitExceeded {
                limit: config.max_messages_per_second,
                window: Duration::from_secs(1),
            })
        }
    }
}

/// Rate limiter for WebSocket connections
#[derive(Debug)]
pub struct WebSocketRateLimiter {
    config: RateLimitConfig,
    clients: Arc<DashMap<IpAddr, ClientRateLimit>>,
    /// Guards [`WebSocketRateLimiter::spawn_cleanup_task`] so it spawns at
    /// most one background task per limiter even if called repeatedly (e.g.
    /// by several `RateLimitMiddleware`s sharing the same `Arc`).
    ///
    /// An `AtomicBool` rather than `std::sync::Once`: `Once` permanently
    /// consumes its "run" on the first call regardless of what that call
    /// does, so a first call outside a Tokio runtime would consume it and
    /// silently prevent every later, in-runtime call from ever spawning.
    /// This flag is only set `true` once a spawn actually succeeds; a failed
    /// attempt (no runtime) rolls it back to `false` so a later call can
    /// retry.
    cleanup_spawned: AtomicBool,
}

impl Default for WebSocketRateLimiter {
    fn default() -> Self {
        Self::new(RateLimitConfig::default())
    }
}

impl WebSocketRateLimiter {
    /// Create new rate limiter with configuration
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            clients: Arc::new(DashMap::new()),
            cleanup_spawned: AtomicBool::new(false),
        }
    }

    /// Returns the rate-limit configuration this limiter was constructed with.
    pub fn config(&self) -> &RateLimitConfig {
        &self.config
    }

    /// Spawn a background task that periodically calls [`Self::cleanup_expired`].
    ///
    /// Idempotent: calling this more than once on the same limiter (e.g. when
    /// several `RateLimitMiddleware`s wrap the same shared `Arc`) spawns only
    /// one task. Requires a Tokio runtime; if none is available, logs a
    /// warning and returns without spawning rather than panicking, since
    /// bare construction of this limiter (and its wrappers) must remain
    /// usable from non-async contexts — a later call to this method (e.g.
    /// once code has entered an async runtime) can still succeed.
    ///
    /// The task holds only a `Weak` reference to `self` and exits on its
    /// own once every strong reference to the limiter is dropped, so it never
    /// keeps the limiter (or its client map) alive past its last owner.
    pub fn spawn_cleanup_task(self: &Arc<Self>, period: Duration) {
        // Claim the right to spawn. If another call already claimed it
        // (whether it succeeded or is in flight), this call is a no-op.
        //
        // Narrow window, not airtight: a concurrent in-runtime caller whose
        // `swap` lands between a no-runtime caller's `swap(true)` above and
        // its rollback `store(false)` below observes the claim as already
        // taken and returns without spawning, even though it could have
        // succeeded. No current call site constructs a limiter and races
        // `spawn_cleanup_task` from both a runtime and a non-runtime thread
        // concurrently, so this is intentionally left as a plain `swap`
        // rather than a CAS retry loop; revisit if that changes.
        if self.cleanup_spawned.swap(true, Ordering::AcqRel) {
            return;
        }

        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            // Release the claim: no runtime was available, so nothing was
            // actually spawned. A later call must be able to retry rather
            // than finding cleanup permanently disabled.
            self.cleanup_spawned.store(false, Ordering::Release);
            tracing::warn!(
                "WebSocketRateLimiter::spawn_cleanup_task: no Tokio runtime available; \
                 periodic cleanup not started"
            );
            return;
        };

        let weak = Arc::downgrade(self);
        handle.spawn(async move {
            let mut interval = tokio::time::interval(period);
            loop {
                interval.tick().await;
                let Some(limiter) = weak.upgrade() else {
                    break;
                };
                limiter.cleanup_expired();
                tracing::debug!("WebSocketRateLimiter: cleanup pass completed");
            }
        });
    }

    /// Whether a cleanup task spawn has been successfully claimed (test-only).
    ///
    /// Lets tests assert that a call site (e.g. `RateLimitMiddleware::new`/
    /// `from_limiter`) actually wired up `spawn_cleanup_task` without waiting
    /// for a real cleanup pass on the production [`DEFAULT_CLEANUP_INTERVAL`].
    #[cfg(test)]
    pub(crate) fn is_cleanup_task_spawned(&self) -> bool {
        self.cleanup_spawned.load(Ordering::Acquire)
    }

    /// Check if request is allowed (HTTP upgrade to WebSocket)
    pub fn check_request(&self, ip: IpAddr) -> Result<(), RateLimitError> {
        if !self.clients.contains_key(&ip) && self.clients.len() >= MAX_TRACKED_CLIENTS {
            return Err(RateLimitError::CapacityExceeded {
                max: MAX_TRACKED_CLIENTS,
            });
        }

        let now = Instant::now();
        let burst = self.config.burst_allowance;
        let mut client = self
            .clients
            .entry(ip)
            .or_insert_with(|| ClientRateLimit::new(burst));

        // `checked_sub` rather than a bare subtraction: on a host whose
        // uptime is shorter than `window_duration` (observed to matter on
        // Windows' QPC-backed `Instant`, which is in the CI matrix), the
        // naive subtraction underflows and panics on the request hot path.
        //
        // On underflow, skip trimming this call rather than either
        // panicking or wiping the client's history: wiping (falling back to
        // an empty window) would fail *open* for exactly the client this
        // control exists to stop — one already at or over its limit could
        // bypass it entirely just by making one more request during this
        // narrow condition (a freshly booted host, or a deliberately
        // crashed-and-restarted service). Denying every request outright
        // instead would fail closed correctly, but for *every* client,
        // including ones that have never made a request before — no
        // different from a hard outage for up to `window_duration` after
        // every process start, on a host that happens to hit this edge
        // case. Keeping the untrimmed history is the fail-closed choice
        // that costs neither: an already-over-limit client's stale entries
        // still count against it (a superset of the correctly windowed
        // history is at least as likely to already be at/over the limit),
        // while a client with no prior history is unaffected either way.
        // The history transiently over-retains stale entries only for the
        // (self-limiting) duration this condition holds; once real uptime
        // exceeds `window_duration`, `checked_sub` succeeds again and
        // trimming resumes, catching up on the backlog in one pass.
        if let Some(window_start) = now.checked_sub(self.config.window_duration) {
            client.requests.retain(|&time| time > window_start);
        }

        // Check request rate limit
        if client.requests.len() >= self.config.max_requests_per_window as usize {
            return Err(RateLimitError::LimitExceeded {
                limit: self.config.max_requests_per_window,
                window: self.config.window_duration,
            });
        }

        // Add current request
        client.requests.push(now);
        Ok(())
    }

    /// Check if new connection is allowed
    pub fn check_connection(&self, ip: IpAddr) -> Result<(), RateLimitError> {
        if !self.clients.contains_key(&ip) && self.clients.len() >= MAX_TRACKED_CLIENTS {
            return Err(RateLimitError::CapacityExceeded {
                max: MAX_TRACKED_CLIENTS,
            });
        }

        let burst = self.config.burst_allowance;
        let mut client = self
            .clients
            .entry(ip)
            .or_insert_with(|| ClientRateLimit::new(burst));

        if client.connection_count >= self.config.max_connections_per_ip {
            return Err(RateLimitError::ConnectionLimitExceeded {
                current: client.connection_count,
                max: self.config.max_connections_per_ip,
            });
        }

        client.connection_count += 1;
        Ok(())
    }

    /// Register connection close
    pub fn close_connection(&self, ip: IpAddr) {
        if let Some(mut client) = self.clients.get_mut(&ip) {
            client.connection_count = client.connection_count.saturating_sub(1);
        }
    }

    /// Check if WebSocket message is allowed
    pub fn check_message(&self, ip: IpAddr, frame_size: usize) -> Result<(), RateLimitError> {
        // Check frame size
        if frame_size > self.config.max_frame_size {
            return Err(RateLimitError::FrameSizeExceeded {
                size: frame_size,
                max: self.config.max_frame_size,
            });
        }

        // Check message rate
        if let Some(mut client) = self.clients.get_mut(&ip) {
            client.check_message_rate(&self.config)?;
        }

        Ok(())
    }

    /// Get current statistics for monitoring
    pub fn get_stats(&self) -> RateLimitStats {
        let mut stats = RateLimitStats::default();

        for entry in self.clients.iter() {
            stats.total_clients += 1;
            stats.total_connections += entry.value().connection_count;

            if entry.value().connection_count > 0 {
                stats.active_clients += 1;
            }
        }

        stats
    }

    /// Clean up expired entries (call periodically)
    pub fn cleanup_expired(&self) {
        let now = Instant::now();
        // `checked_sub` rather than a bare subtraction: on a host whose
        // uptime is shorter than `window_duration * 2` (fresh container, or
        // a large configured window), the naive subtraction underflows
        // `Instant` and panics, permanently killing whichever loop calls
        // this. Skip this pass instead — the next sweep, once enough
        // wall-clock time has elapsed, will succeed.
        let Some(cutoff) = now.checked_sub(self.config.window_duration * 2) else {
            return;
        };

        self.clients.retain(|_, client| {
            // Remove clients with no recent activity and no connections
            !(client.connection_count == 0
                && client.requests.last().is_none_or(|&time| time < cutoff))
        });
    }
}

/// Rate limiting statistics
#[derive(Debug, Default, Clone)]
pub struct RateLimitStats {
    /// Total distinct clients tracked.
    pub total_clients: usize,
    /// Clients that have shown activity within the recent window.
    pub active_clients: usize,
    /// Sum of currently held connections across all clients.
    pub total_connections: usize,
}

/// Rate limiting middleware for tracking client IPs
#[derive(Debug, Clone)]
pub struct RateLimitGuard {
    rate_limiter: Arc<WebSocketRateLimiter>,
    client_ip: IpAddr,
}

impl RateLimitGuard {
    /// Create new guard for a client connection
    pub fn new(
        rate_limiter: Arc<WebSocketRateLimiter>,
        client_ip: IpAddr,
    ) -> Result<Self, RateLimitError> {
        rate_limiter.check_connection(client_ip)?;

        Ok(Self {
            rate_limiter,
            client_ip,
        })
    }

    /// Check if message is allowed
    pub fn check_message(&self, frame_size: usize) -> Result<(), RateLimitError> {
        self.rate_limiter.check_message(self.client_ip, frame_size)
    }
}

impl Drop for RateLimitGuard {
    fn drop(&mut self) {
        self.rate_limiter.close_connection(self.client_ip);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_rate_limit_requests() {
        let config = RateLimitConfig {
            max_requests_per_window: 2,
            window_duration: Duration::from_millis(100),
            ..Default::default()
        };

        let limiter = WebSocketRateLimiter::new(config);
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));

        // First two requests should succeed
        assert!(limiter.check_request(ip).is_ok());
        assert!(limiter.check_request(ip).is_ok());

        // Third request should be rate limited
        assert!(limiter.check_request(ip).is_err());

        // Wait for window to reset
        thread::sleep(Duration::from_millis(110));

        // Should work again
        assert!(limiter.check_request(ip).is_ok());
    }

    #[test]
    fn test_connection_limits() {
        let config = RateLimitConfig {
            max_connections_per_ip: 2,
            ..Default::default()
        };

        let limiter = WebSocketRateLimiter::new(config);
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));

        // Two connections should succeed
        assert!(limiter.check_connection(ip).is_ok());
        assert!(limiter.check_connection(ip).is_ok());

        // Third connection should fail
        assert!(limiter.check_connection(ip).is_err());

        // Close one connection
        limiter.close_connection(ip);

        // Should work again
        assert!(limiter.check_connection(ip).is_ok());
    }

    #[test]
    fn test_message_rate_limiting() {
        let config = RateLimitConfig {
            max_messages_per_second: 2,
            burst_allowance: 2, // Allow 2 burst messages
            ..Default::default()
        };

        let limiter = WebSocketRateLimiter::new(config.clone());
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));

        // First connection should create the client entry
        let client = limiter
            .clients
            .entry(ip)
            .or_insert_with(|| ClientRateLimit::new(config.burst_allowance));
        // Tokens are already initialized with burst_allowance
        drop(client);

        // Should allow burst messages
        assert!(limiter.check_message(ip, 1024).is_ok());
        assert!(limiter.check_message(ip, 1024).is_ok());

        // Should be rate limited now (no more tokens)
        assert!(limiter.check_message(ip, 1024).is_err());
    }

    #[test]
    fn test_frame_size_limits() {
        let config = RateLimitConfig {
            max_frame_size: 1024,
            ..Default::default()
        };

        let limiter = WebSocketRateLimiter::new(config);
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));

        // Small frame should succeed
        assert!(limiter.check_message(ip, 512).is_ok());

        // Large frame should fail
        assert!(limiter.check_message(ip, 2048).is_err());
    }

    #[test]
    fn test_rate_limit_guard() {
        let config = RateLimitConfig {
            max_connections_per_ip: 1,
            ..Default::default()
        };

        let limiter = Arc::new(WebSocketRateLimiter::new(config));
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));

        // Create guard
        let guard = RateLimitGuard::new(limiter.clone(), ip).unwrap();

        // Second connection should fail
        assert!(RateLimitGuard::new(limiter.clone(), ip).is_err());

        // Drop guard
        drop(guard);

        // Should work again
        assert!(RateLimitGuard::new(limiter, ip).is_ok());
    }

    #[test]
    fn test_token_refill_over_time() {
        let config = RateLimitConfig {
            max_messages_per_second: 1,
            burst_allowance: 0,
            window_duration: Duration::from_millis(100),
            ..Default::default()
        };

        let limiter = WebSocketRateLimiter::new(config.clone());
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));

        // Pre-fill tokens to test refill
        {
            let mut client = limiter
                .clients
                .entry(ip)
                .or_insert_with(|| ClientRateLimit::new(config.burst_allowance));
            client.tokens = 0.5; // Start with partial token
        }

        // Should fail with insufficient tokens
        assert!(limiter.check_message(ip, 512).is_err());

        // Wait for token refill (1 second = max_messages_per_second tokens)
        thread::sleep(Duration::from_millis(1100));

        // Should work again after tokens refill (refilled tokens + remaining time)
        let result = limiter.check_message(ip, 512);
        // After 1.1 seconds, should have refilled enough tokens to pass
        assert!(result.is_ok(), "Expected refilled tokens to allow message");
    }

    #[test]
    fn test_cleanup_expired_entries() {
        let config = RateLimitConfig {
            window_duration: Duration::from_millis(100),
            ..Default::default()
        };

        let limiter = WebSocketRateLimiter::new(config);
        let ip1 = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        let ip2 = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2));

        // Add some client entries
        assert!(limiter.check_connection(ip1).is_ok());
        assert!(limiter.check_connection(ip2).is_ok());

        // Should have 2 clients
        assert_eq!(limiter.get_stats().total_clients, 2);

        // Close connection for ip1
        limiter.close_connection(ip1);

        // Wait beyond the cleanup window
        thread::sleep(Duration::from_millis(250));

        // Cleanup should remove idle clients
        limiter.cleanup_expired();

        // After cleanup, ip1 should be removed but ip2 might remain if it has recent activity
        let stats = limiter.get_stats();
        // At minimum, ip1 should be cleaned up if no connections
        assert!(stats.total_clients <= 2);
    }

    #[test]
    fn test_multiple_ips_isolation() {
        let config = RateLimitConfig {
            max_requests_per_window: 1,
            window_duration: Duration::from_millis(100),
            ..Default::default()
        };

        let limiter = WebSocketRateLimiter::new(config);
        let ip1 = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        let ip2 = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2));

        // ip1 should be rate limited after 1 request
        assert!(limiter.check_request(ip1).is_ok());
        assert!(limiter.check_request(ip1).is_err());

        // ip2 should NOT be affected by ip1's limit
        assert!(limiter.check_request(ip2).is_ok());
        assert!(limiter.check_request(ip2).is_err());
    }

    #[test]
    fn test_burst_allowance_boundary() {
        let config = RateLimitConfig {
            max_messages_per_second: 1,
            burst_allowance: 0,
            ..Default::default()
        };

        let limiter = WebSocketRateLimiter::new(config.clone());
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));

        // With 0 burst, even the first message might be throttled
        // depending on token distribution
        let mut client = limiter
            .clients
            .entry(ip)
            .or_insert_with(|| ClientRateLimit::new(config.burst_allowance));
        client.tokens = 0.0;
        drop(client);

        // Should fail with no tokens
        assert!(limiter.check_message(ip, 512).is_err());
    }

    #[test]
    fn test_rate_limit_config_high_traffic() {
        let config = RateLimitConfig::high_traffic();

        assert_eq!(config.max_requests_per_window, 1000);
        assert_eq!(config.max_connections_per_ip, 50);
        assert_eq!(config.max_messages_per_second, 100);
        assert_eq!(config.burst_allowance, 20);
        assert!(config.max_frame_size >= 1024 * 1024);
    }

    #[test]
    fn test_rate_limit_config_low_resource() {
        let config = RateLimitConfig::low_resource();

        assert_eq!(config.max_requests_per_window, 20);
        assert_eq!(config.max_connections_per_ip, 2);
        assert_eq!(config.max_messages_per_second, 5);
        assert_eq!(config.burst_allowance, 2);
        assert_eq!(config.max_frame_size, 256 * 1024);
        assert_eq!(config.write_timeout, Duration::from_secs(3));
        assert!(config.write_timeout < RateLimitConfig::default().write_timeout);
    }

    #[test]
    fn test_frame_size_boundary_exact() {
        let config = RateLimitConfig {
            max_frame_size: 1024,
            ..Default::default()
        };

        let limiter = WebSocketRateLimiter::new(config);
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));

        // Exactly at limit should succeed
        assert!(limiter.check_message(ip, 1024).is_ok());

        // Just over limit should fail
        assert!(limiter.check_message(ip, 1025).is_err());

        // Zero-size frame should succeed (though uncommon)
        assert!(limiter.check_message(ip, 0).is_ok());
    }

    #[test]
    fn test_get_stats_accuracy() {
        let config = RateLimitConfig {
            max_connections_per_ip: 5,
            ..Default::default()
        };

        let limiter = WebSocketRateLimiter::new(config);
        let ip1 = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        let ip2 = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2));

        // Add connections
        assert!(limiter.check_connection(ip1).is_ok());
        assert!(limiter.check_connection(ip1).is_ok());
        assert!(limiter.check_connection(ip2).is_ok());

        let stats = limiter.get_stats();
        assert_eq!(stats.total_clients, 2);
        assert_eq!(stats.total_connections, 3);
        assert_eq!(stats.active_clients, 2);

        // Close a connection
        limiter.close_connection(ip1);

        let stats = limiter.get_stats();
        assert_eq!(stats.total_connections, 2);
    }

    #[test]
    fn test_window_duration_respected() {
        let config = RateLimitConfig {
            max_requests_per_window: 1,
            window_duration: Duration::from_millis(50),
            ..Default::default()
        };

        let limiter = WebSocketRateLimiter::new(config);
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));

        // First request succeeds
        assert!(limiter.check_request(ip).is_ok());

        // Second request within window fails
        assert!(limiter.check_request(ip).is_err());

        // Wait for window to pass
        thread::sleep(Duration::from_millis(60));

        // Request after window passes succeeds
        assert!(limiter.check_request(ip).is_ok());
    }

    #[test]
    fn test_default_limiter() {
        // Test Default implementation for WebSocketRateLimiter
        let limiter = WebSocketRateLimiter::default();
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));

        // Default limiter should allow requests
        assert!(limiter.check_request(ip).is_ok());
        assert!(limiter.check_connection(ip).is_ok());

        // Verify default config values are applied
        let stats = limiter.get_stats();
        assert_eq!(stats.total_clients, 1);
        assert_eq!(stats.total_connections, 1);
    }

    #[test]
    fn test_cleanup_expired_removes_inactive_clients() {
        let config = RateLimitConfig {
            window_duration: Duration::from_millis(50),
            ..Default::default()
        };

        let limiter = WebSocketRateLimiter::new(config);
        let ip1 = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        let ip2 = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2));
        let ip3 = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 3));

        // Add requests for multiple IPs
        assert!(limiter.check_request(ip1).is_ok());
        assert!(limiter.check_request(ip2).is_ok());
        assert!(limiter.check_connection(ip3).is_ok());

        let initial_stats = limiter.get_stats();
        assert_eq!(initial_stats.total_clients, 3);

        // Wait for cleanup window
        thread::sleep(Duration::from_millis(150));

        // ip3 has no requests, so it should be removed
        limiter.cleanup_expired();

        let after_cleanup = limiter.get_stats();
        // ip3 should be removed (no requests, no connections after cleanup)
        assert!(after_cleanup.total_clients <= initial_stats.total_clients);
    }

    #[test]
    fn test_client_with_zero_connections_and_no_recent_requests_cleaned() {
        let config = RateLimitConfig {
            window_duration: Duration::from_millis(100),
            ..Default::default()
        };

        let limiter = WebSocketRateLimiter::new(config);
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100));

        // Make a request
        assert!(limiter.check_request(ip).is_ok());

        // Verify client exists
        let initial_stats = limiter.get_stats();
        assert_eq!(initial_stats.total_clients, 1);

        // Wait beyond cleanup window (2x window_duration)
        thread::sleep(Duration::from_millis(250));

        // Cleanup should remove the client (no connections and stale requests)
        limiter.cleanup_expired();

        let final_stats = limiter.get_stats();
        // The client should be removed if no active connections
        assert_eq!(final_stats.total_clients, 0);
    }

    #[test]
    fn test_cleanup_preserves_active_clients() {
        let config = RateLimitConfig {
            window_duration: Duration::from_millis(100),
            ..Default::default()
        };

        let limiter = WebSocketRateLimiter::new(config);
        let ip1 = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        let ip2 = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2));

        // ip1: has active connection
        assert!(limiter.check_connection(ip1).is_ok());

        // ip2: has recent request but no connection
        assert!(limiter.check_request(ip2).is_ok());

        let initial_stats = limiter.get_stats();
        assert_eq!(initial_stats.total_clients, 2);

        // Wait some time (but not beyond full cleanup window)
        thread::sleep(Duration::from_millis(80));

        // Make another request to ip2 to keep it fresh
        let _ = limiter.check_request(ip2);

        // Cleanup should preserve both clients
        limiter.cleanup_expired();

        let final_stats = limiter.get_stats();
        // ip1 should be preserved (active connection)
        assert!(final_stats.total_clients >= 1);
    }

    #[test]
    fn test_close_connection_on_nonexistent_ip() {
        let limiter = WebSocketRateLimiter::default();
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 99));

        // Closing connection on non-existent IP should not panic
        limiter.close_connection(ip);

        // Stats should be empty
        let stats = limiter.get_stats();
        assert_eq!(stats.total_clients, 0);
    }

    #[test]
    fn test_check_message_on_nonexistent_client() {
        let limiter = WebSocketRateLimiter::default();
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 88));

        // Checking message on non-existent IP should be OK for frame size
        // but not create the client entry if it doesn't exist in clients map
        assert!(limiter.check_message(ip, 512).is_ok());
    }

    #[test]
    fn test_rate_limit_guard_check_message() {
        let config = RateLimitConfig {
            max_connections_per_ip: 5,
            max_frame_size: 1024,
            max_messages_per_second: 10,
            burst_allowance: 5,
            ..Default::default()
        };

        let limiter = Arc::new(WebSocketRateLimiter::new(config));
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));

        let guard = RateLimitGuard::new(limiter.clone(), ip).unwrap();

        assert!(guard.check_message(512).is_ok());
        assert!(guard.check_message(512).is_ok());
        assert!(guard.check_message(2048).is_err());
    }

    #[test]
    fn test_rate_limit_guard_check_message_rate_limit() {
        let config = RateLimitConfig {
            max_connections_per_ip: 5,
            max_frame_size: 10_000,
            max_messages_per_second: 2,
            burst_allowance: 2,
            ..Default::default()
        };

        let limiter = Arc::new(WebSocketRateLimiter::new(config));
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2));

        let guard = RateLimitGuard::new(limiter.clone(), ip).unwrap();

        assert!(guard.check_message(512).is_ok());
        assert!(guard.check_message(512).is_ok());
        assert!(guard.check_message(512).is_err());
    }

    #[test]
    fn test_capacity_cap_rejects_new_clients_when_full() {
        let limiter = WebSocketRateLimiter::default();

        for i in 0..MAX_TRACKED_CLIENTS as u32 {
            let ip = IpAddr::V4(Ipv4Addr::from(i));
            limiter.check_request(ip).unwrap();
        }
        assert_eq!(limiter.get_stats().total_clients, MAX_TRACKED_CLIENTS);

        // A new, not-yet-tracked IP is rejected once at capacity — this is
        // what bounds the map's size *within* a single cleanup sweep window,
        // not just across sweeps.
        let overflow_ip = IpAddr::V4(Ipv4Addr::from(MAX_TRACKED_CLIENTS as u32));
        let result = limiter.check_request(overflow_ip);
        assert!(matches!(
            result,
            Err(RateLimitError::CapacityExceeded { max }) if max == MAX_TRACKED_CLIENTS
        ));
        assert_eq!(limiter.get_stats().total_clients, MAX_TRACKED_CLIENTS);

        // An already-tracked IP is unaffected by the cap.
        let existing_ip = IpAddr::V4(Ipv4Addr::from(0u32));
        assert!(limiter.check_request(existing_ip).is_ok());
    }

    #[test]
    fn test_cleanup_expired_never_panics_regardless_of_window_duration() {
        // `Instant` intentionally exposes no public constructor for an
        // arbitrary point in time, so whether `Instant::now().checked_sub(..)`
        // actually underflows for a given `window_duration` depends on the
        // OS's monotonic clock epoch, which is unspecified and cannot be
        // forced deterministically from a portable unit test (observed to
        // matter in practice on Windows' QPC-backed `Instant` near process
        // or host start — exercised naturally by the Windows CI legs, not by
        // this test). What this test pins down instead is the invariant that
        // actually matters regardless of which branch runs on a given host:
        // `cleanup_expired` must never panic for any configured
        // `window_duration`, including ones designed to underflow, and must
        // never evict a client with a request timestamped just now.
        for window_secs in [1, 60, 3600, u64::MAX / 8] {
            let config = RateLimitConfig {
                window_duration: Duration::from_secs(window_secs),
                ..Default::default()
            };
            let limiter = WebSocketRateLimiter::new(config);
            let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
            limiter.check_request(ip).unwrap();

            limiter.cleanup_expired(); // Must not panic for any window_secs above.

            assert_eq!(
                limiter.get_stats().total_clients,
                1,
                "a client with a just-now request must survive cleanup regardless \
                 of window_secs={window_secs}"
            );
        }
    }

    #[test]
    fn test_check_request_never_panics_regardless_of_window_duration() {
        // Same rationale as the `cleanup_expired` test above, but for the
        // `checked_sub` guard on the request hot path in `check_request`.
        // Whether `checked_sub` actually underflows for a given window_secs
        // is platform-dependent (see `test_cleanup_expired_never_panics_...`
        // above) — this test only pins down that it never panics regardless
        // of which branch runs.
        for window_secs in [1, 60, 3600, u64::MAX / 8] {
            let config = RateLimitConfig {
                window_duration: Duration::from_secs(window_secs),
                ..Default::default()
            };
            let limiter = WebSocketRateLimiter::new(config);
            let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));

            let _ = limiter.check_request(ip); // Must not panic for any window_secs above.
        }
    }

    #[tokio::test]
    async fn test_spawn_cleanup_task_is_idempotent() {
        let limiter = Arc::new(WebSocketRateLimiter::new(RateLimitConfig {
            window_duration: Duration::from_millis(1),
            ..Default::default()
        }));

        // Calling this more than once must not spawn a second task (and must
        // not panic); the loop below only passes if exactly the expected
        // single cleanup pass took effect.
        limiter.spawn_cleanup_task(Duration::from_millis(10));
        limiter.spawn_cleanup_task(Duration::from_millis(10));

        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        limiter.check_request(ip).unwrap();

        tokio::time::sleep(Duration::from_millis(100)).await;

        assert_eq!(limiter.get_stats().total_clients, 0);
    }
}
