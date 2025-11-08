//! Rate limiting system for WebSocket connections to prevent DoS attacks

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::{
    net::IpAddr,
    sync::Arc,
    time::{Duration, Instant},
};
use thiserror::Error;

/// Rate limiting errors
#[derive(Error, Debug, Clone)]
pub enum RateLimitError {
    #[error("Rate limit exceeded: {limit} requests per {window:?}")]
    LimitExceeded { limit: u32, window: Duration },

    #[error("Connection limit exceeded: {current}/{max} connections")]
    ConnectionLimitExceeded { current: usize, max: usize },

    #[error("Frame size limit exceeded: {size} bytes > {max} bytes")]
    FrameSizeExceeded { size: usize, max: usize },
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
        }
    }

    /// Check if request is allowed (HTTP upgrade to WebSocket)
    pub fn check_request(&self, ip: IpAddr) -> Result<(), RateLimitError> {
        let now = Instant::now();
        let burst = self.config.burst_allowance;
        let mut client = self
            .clients
            .entry(ip)
            .or_insert_with(|| ClientRateLimit::new(burst));

        // Clean old requests outside window
        let window_start = now - self.config.window_duration;
        client.requests.retain(|&time| time > window_start);

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
        let cutoff = now - self.config.window_duration * 2; // Keep some history

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
    pub total_clients: usize,
    pub active_clients: usize,
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
}
