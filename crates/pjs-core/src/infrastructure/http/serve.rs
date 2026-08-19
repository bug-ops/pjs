//! Connection-level protection for servers hosting this crate's [`Router`]s.
//!
//! [`apply_common_layers`](super::axum_adapter) bounds handler-execution
//! concurrency and pre-response request time, but — as documented on
//! `RESPONSE_BODY_IDLE_TIMEOUT` in `axum_adapter.rs` — none of those tower
//! layers can detect a client that stops reading the response socket: once
//! hyper's write buffer fills and `poll_flush` parks waiting on socket
//! writability, the response body is never polled again, so a
//! poll-driven idle timeout never fires. Closing that gap requires owning
//! the accept loop and the raw connection, which is what [`serve_with_limits`]
//! does in place of `axum::serve`.

use std::{
    io,
    net::{IpAddr, Ipv6Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use axum::{Extension, Router, extract::ConnectInfo};
use hyper_util::{
    rt::{TokioExecutor, TokioIo, TokioTimer},
    server::conn::auto::Builder,
    service::TowerToHyperService,
};
use tokio::{net::TcpListener, sync::Semaphore};
use tower::Layer;
use tracing::{debug, error, warn};

use crate::security::rate_limit::{
    RateLimitConfig, RateLimitError, RateLimitGuard, WebSocketRateLimiter,
};

/// Interval between HTTP/2 keep-alive `PING` frames sent on an established
/// connection.
///
/// Bounds an *unresponsive* HTTP/2 connection below `max_connection_duration`
/// by detecting a peer that stops answering pings. This is a responsiveness
/// check only, not an idleness check: a peer that keeps acknowledging pings
/// every interval survives the full `max_connection_duration` ceiling
/// regardless of how idle the connection otherwise is. Inert unless an
/// interval is set, since hyper's h2 keep-alive defaults to disabled.
const H2_KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(30);

/// Deadline for a peer to acknowledge an HTTP/2 keep-alive `PING` before the
/// connection is dropped as unresponsive.
///
/// 20s matches hyper's own current default (`hyper::proto::h2::server`);
/// pinned explicitly here (rather than left implicit) so a future change to
/// that upstream default doesn't silently change this crate's behavior.
const H2_KEEP_ALIVE_TIMEOUT: Duration = Duration::from_secs(20);

/// Connection-level limits enforced by [`serve_with_limits`], independent of
/// (and in addition to) the request-level tower layers applied by this
/// crate's router constructors.
///
/// Constructed via [`ConnectionLimits::default`] and overridden per field;
/// `#[non_exhaustive]` so new limits can be added without a breaking change.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use pjson_rs::infrastructure::http::ConnectionLimits;
///
/// // Long-lived WebSocket listeners disable the connection-duration ceiling
/// // (see that field's docs) while keeping the other defaults.
/// let mut limits = ConnectionLimits::default();
/// limits.max_connection_duration = None;
/// assert_eq!(limits.header_read_timeout, Some(Duration::from_secs(10)));
/// ```
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ConnectionLimits {
    /// Deadline for a client to finish sending request headers after the
    /// connection is accepted, `None` to disable.
    ///
    /// Defaults to 10s. Hyper's own default is 30s and nginx uses 60s;
    /// this crate picks 10s to cut the cost of a slowloris-style
    /// header-trickle attack roughly 6x relative to hyper's default while
    /// remaining far above the time any real client needs to send headers.
    ///
    /// Also gates [`serve_with_limits`]'s preface-read wait (see that
    /// function's implementation) — setting this to `None` together with
    /// `max_connection_duration: None` lets a connection that never sends a
    /// byte hold its `max_connections` slot (and, if assigned one, its
    /// `max_connections_per_ip` slot) indefinitely; at least one of the two
    /// should normally stay `Some`.
    pub header_read_timeout: Option<Duration>,

    /// Hard ceiling on a single connection's total lifetime, `None` to
    /// disable.
    ///
    /// Defaults to 300s (5 minutes). Response payload size is bounded by
    /// `MAX_FRAMES_PER_REQUEST` (see `domain::config::limits`) together with
    /// the 10MB `DefaultBodyLimit` applied in `apply_common_layers`, which
    /// implies a real client only ever needs to sustain roughly 33 KB/s to
    /// finish reading within this window — far under any real client's
    /// throughput and far over what a stalling client can fake.
    ///
    /// **WebSocket caveat**: this is a hard deadline on the whole
    /// connection, including any upgraded protocol — it will terminate a
    /// legitimate long-lived WebSocket session just as readily as a
    /// stalling one. A listener that serves WebSocket upgrade routes should
    /// set this to `None` and rely on WS-level idle/ping timeouts instead;
    /// `crates/pjs-demo/src/servers/websocket_streaming.rs` does exactly
    /// that. In particular, if serving a router that mounts this crate's own
    /// `/pjs/ws/{session_id}` upgrade route (see `infrastructure::websocket`),
    /// set this to `None` — the default 300s ceiling will otherwise kill
    /// every WebSocket session it outlives.
    pub max_connection_duration: Option<Duration>,

    /// Maximum number of concurrently open connections.
    ///
    /// Defaults to 1024: conservative, overridable, and comfortably under
    /// the file-descriptor soft limit on typical deployment targets. This
    /// bounds accept-loop backpressure (how many connections are being
    /// served at once), not per-request concurrency, which is a separate
    /// concern already covered by `MAX_CONCURRENT_REQUESTS` in
    /// `apply_common_layers`.
    pub max_connections: usize,

    /// Hard cap on concurrently open HTTP/2 streams per connection, `None`
    /// to leave hyper's own default in place.
    ///
    /// Defaults to `Some(128)`. Hyper's own default is `Some(200)` and is
    /// documented as explicitly unstable ("not part of the stability of
    /// hyper... encouraged to set your own limit") — 128 sits strictly
    /// below that default while remaining far above what any legitimate
    /// browser or client needs. Combined with `max_connections`, this
    /// bounds the worst case at `max_connections * max_concurrent_streams`
    /// in-flight streams before `MAX_CONCURRENT_REQUESTS` (see
    /// `apply_common_layers`) parks the rest.
    pub max_concurrent_streams: Option<u32>,

    /// Hard cap on concurrently open connections from a single accept-level
    /// source IP, `None` to disable.
    ///
    /// Defaults to `Some(64)` — 1/16th of the default `max_connections`
    /// pool, so at least 16 distinct source IPs are needed to fully exhaust
    /// it. All three `pjs-demo` servers bind `127.0.0.1`, so every local
    /// connection (including load/CI test loops) shares this one budget;
    /// 64 concurrent connections from a single source is still far above
    /// realistic demo or local-test load, so this is not special-cased.
    ///
    /// Enforced by a private `WebSocketRateLimiter` instance owned by
    /// [`serve_with_limits`], independent of (and never sharing state with)
    /// any `RateLimitMiddleware` the router itself may apply. `None`
    /// disables the cap entirely — no limiter instance is constructed, no
    /// cleanup task is spawned, and no per-connection map entry is made, so
    /// a reverse-proxy deployment that sets this to `None` (see below) pays
    /// no cost for it.
    ///
    /// **Reverse-proxy caveat**: like `max_connection_duration`, this is
    /// enforced at the accept level, before any HTTP request is parsed — no
    /// headers exist yet, so `X-Forwarded-For`/trusted-proxy configuration
    /// cannot apply here. Every connection arriving through a
    /// connection-pooling reverse proxy (nginx, a load balancer, etc.)
    /// shares that proxy's single source IP, so this cap would apply to all
    /// of them combined rather than to each real client individually. A
    /// deployment behind such a proxy must set this to `None` and rely on
    /// the proxy's own per-client limiting instead.
    pub max_connections_per_ip: Option<usize>,
}

impl Default for ConnectionLimits {
    fn default() -> Self {
        Self {
            header_read_timeout: Some(Duration::from_secs(10)),
            max_connection_duration: Some(Duration::from_secs(300)),
            max_connections: 1024,
            max_concurrent_streams: Some(128),
            max_connections_per_ip: Some(64),
        }
    }
}

/// Serve `router` on `listener`, enforcing `limits` at the connection level.
///
/// A drop-in replacement for `axum::serve(listener, router).await?` that
/// additionally protects against the class of client that establishes a
/// connection and then never finishes sending a request, or stops reading
/// the response — neither of which the request-level tower layers in
/// `apply_common_layers` can detect (see the module docs). Each accepted
/// connection is served on its own task via
/// [`serve_connection_with_upgrades`](hyper_util::server::conn::auto::Builder::serve_connection_with_upgrades),
/// so WebSocket upgrade routes keep working; the peer address obtained from
/// `accept()` is injected into each connection's request extensions as
/// [`ConnectInfo<SocketAddr>`](axum::extract::ConnectInfo), matching what
/// `axum::serve(listener, router.into_make_service_with_connect_info())`
/// would provide, so `ConnectInfo`-based extractors (this crate's own
/// WebSocket upgrade handler and per-IP rate limiter included) keep working.
///
/// A single slow or misbehaving *connection* is dropped (and logged at
/// debug level), never propagated out of this function, so it cannot bring
/// down the accept loop. A transient error from [`TcpListener::accept`]
/// itself (e.g. `EMFILE`, or a peer that reset the connection between the
/// kernel accepting it and userspace calling `accept()`) does not terminate
/// the loop either — mirroring axum's own `Listener::accept` retry
/// behavior, connection-class errors (`ConnectionRefused`,
/// `ConnectionAborted`, `ConnectionReset`) are retried immediately and any
/// other error is logged and retried after a 1s backoff. In practice this
/// function only returns if a future revision adds an explicit exit path;
/// today it runs until the process is torn down, the same as
/// `axum::serve(listener, app).await` today.
///
/// # Examples
///
/// ```no_run
/// # async fn run() -> std::io::Result<()> {
/// use axum::Router;
/// use pjson_rs::infrastructure::http::{ConnectionLimits, serve_with_limits};
/// use tokio::net::TcpListener;
///
/// let listener = TcpListener::bind("127.0.0.1:0").await?;
/// let router = Router::new();
/// serve_with_limits(listener, router, ConnectionLimits::default()).await?;
/// # Ok(())
/// # }
/// ```
pub async fn serve_with_limits(
    listener: TcpListener,
    router: Router,
    limits: ConnectionLimits,
) -> std::io::Result<()> {
    let mut builder = Builder::new(TokioExecutor::new());
    builder.http1().timer(TokioTimer::new());
    builder.http2().timer(TokioTimer::new());
    // Gated rather than passed through unconditionally: hyper's
    // `max_concurrent_streams(None)` means "remove the limit entirely", the
    // opposite of this field's `None` = "leave hyper's own default in
    // place" contract — skipping the call when `None` is what actually
    // preserves hyper's default of 200.
    if let Some(max_concurrent_streams) = limits.max_concurrent_streams {
        builder
            .http2()
            .max_concurrent_streams(max_concurrent_streams);
    }
    builder
        .http2()
        .keep_alive_interval(Some(H2_KEEP_ALIVE_INTERVAL))
        .keep_alive_timeout(H2_KEEP_ALIVE_TIMEOUT);
    if let Some(header_read_timeout) = limits.header_read_timeout {
        builder.http1().header_read_timeout(header_read_timeout);
    }
    let builder = Arc::new(builder);
    let semaphore = Arc::new(Semaphore::new(limits.max_connections));
    let max_connection_duration = limits.max_connection_duration;
    let header_read_timeout = limits.header_read_timeout;

    // Separate from any `RateLimitMiddleware` the router itself may apply:
    // sharing one `Arc<WebSocketRateLimiter>` would let an accept-level IP
    // flood saturate the middleware's tracked-client map and start
    // rejecting legitimate new clients at the HTTP layer.
    //
    // `None` skips construction entirely (rather than substituting
    // `usize::MAX`), so a deployment that disables this cap — e.g. behind a
    // connection-pooling reverse proxy, see the field's doc — pays no cost
    // for it: no limiter, no cleanup task, no per-connection map entry.
    let per_ip_limiter = limits.max_connections_per_ip.map(|max_connections_per_ip| {
        let limiter = Arc::new(WebSocketRateLimiter::new(RateLimitConfig {
            max_connections_per_ip,
            ..Default::default()
        }));
        limiter.spawn_cleanup_task(Duration::from_secs(60));
        limiter
    });

    loop {
        // Acquired before accept() so a full connection pool applies
        // backpressure at the accept loop rather than inside hyper. The
        // semaphore is never closed, so `Err` here is unreachable in
        // practice; treat it as a benign "stop serving" signal rather than
        // panicking on an accept-loop hot path.
        let Ok(permit) = Arc::clone(&semaphore).acquire_owned().await else {
            return Ok(());
        };
        let (stream, peer_addr) = accept_with_retry(&listener).await;

        // Rejects fast, before the stream is ever wrapped or a task
        // spawned — this mitigates (does not fully close: `permit` above is
        // still acquired before this check runs, so >=16 distinct source
        // IPs, or `max_connections_per_ip: None`, still reproduce the
        // symptom) the accept-level backlog-hang, since a single stalling
        // source IP's connections are rejected immediately by `continue`
        // without ever reaching hyper. Only `ConnectionLimitExceeded` (the
        // per-IP cap itself) rejects; every other error — today only
        // `CapacityExceeded` (the limiter's own tracked-client map is full),
        // but `RateLimitError` is not `#[non_exhaustive]` so this stays
        // exhaustive-by-intent rather than by variant count — fails *open*
        // instead of reusing `WebSocketRateLimiter`'s fail-closed default:
        // at accept level, fail-closed would mean total lockout of every
        // new source IP once the map fills, whereas the global
        // `max_connections` semaphore already bounds the worst case.
        let guard = match &per_ip_limiter {
            Some(limiter) => {
                let ip_key = accept_rate_limit_key(peer_addr.ip());
                match RateLimitGuard::new(Arc::clone(limiter), ip_key) {
                    Ok(guard) => Some(guard),
                    Err(RateLimitError::ConnectionLimitExceeded { .. }) => {
                        warn!(%peer_addr, "per-IP connection limit exceeded, dropping connection");
                        continue;
                    }
                    Err(error) => {
                        debug!(
                            %peer_addr, %error,
                            "per-IP rate limiter rejected connection for a reason other \
                             than the per-IP cap; admitting"
                        );
                        None
                    }
                }
            }
            None => None,
        };

        let peer_service = Extension(ConnectInfo(peer_addr)).layer(router.clone());
        let service = TowerToHyperService::new(peer_service);
        let builder = Arc::clone(&builder);

        tokio::spawn(async move {
            let _permit = permit;
            let _guard = guard;

            // Closes the *zero-byte* gap in hyper-util's `auto::Builder`,
            // whose `ReadVersion` preface-sniffing step runs before any
            // protocol builder engages and has no timer of its own —
            // `header_read_timeout` only arms once >=1 byte classifies the
            // connection as H1, so a client that never sends anything was
            // otherwise bounded solely by `max_connection_duration` (300s
            // default). `readable()` is readiness-only and consumes no
            // bytes, so hyper still sees the full stream afterward; this
            // check is protocol-agnostic and applies to h1 and h2 alike.
            //
            // Residual, not fully closed: a client that sends exactly one
            // byte matching the H2 preface, then stalls, still passes this
            // gate (`readable()` only requires *some* data to have arrived)
            // and rides out the full `max_connection_duration` before
            // `ReadVersion` itself gives up — this raises the attacker's
            // cost from 0 bytes to 1 byte, it does not add a timer to
            // `ReadVersion`. `max_connections_per_ip` and `max_connections`
            // still bound that window.
            if let Some(timeout) = header_read_timeout
                && tokio::time::timeout(timeout, stream.readable())
                    .await
                    .is_err()
            {
                debug!(%peer_addr, "no bytes received within header_read_timeout, dropping");
                return;
            }

            let io = TokioIo::new(stream);
            let conn = builder.serve_connection_with_upgrades(io, service);
            let result = match max_connection_duration {
                Some(deadline) => match tokio::time::timeout(deadline, conn).await {
                    Ok(result) => result,
                    Err(_elapsed) => {
                        debug!("connection exceeded max_connection_duration, dropping");
                        return;
                    }
                },
                None => conn.await,
            };
            if let Err(error) = result {
                debug!(%error, "connection closed with error");
            }
        });
    }
}

/// Masks an IPv6 address down to its /64 network prefix for accept-level
/// per-IP rate-limit keying; IPv4 addresses pass through unchanged.
///
/// Without this masking, an attacker holding a routed /64 (trivially
/// available from many providers) could rotate addresses within that prefix
/// to both bypass `ConnectionLimits::max_connections_per_ip` and exhaust
/// `WebSocketRateLimiter`'s tracked-client map. This intentionally diverges
/// from `RateLimitMiddleware`'s exact-IP keying, which governs a different
/// (request-level) layer and is left unchanged.
///
/// Two cases are normalized *before* the /64 mask is applied, both because
/// their top 64 bits are all zero and would otherwise collapse onto the
/// same `::/64` key as every other such address:
/// - **IPv4-mapped addresses** (`::ffff:a.b.c.d`) are unwrapped to their
///   plain `IpAddr::V4` form. A dual-stack `[::]:port` listener reports
///   every IPv4 peer this way by default on Linux (`bindv6only=0`);
///   without unwrapping, all IPv4 traffic would share one 64-connection
///   budget — a new DoS the accept-level cap would itself introduce.
/// - **Loopback** (`::1`) passes through unmasked, since it is not
///   IPv4-mapped and would otherwise mask to the same `::/64` key as the
///   unspecified address and other zero-prefix addresses. Reachable only
///   locally, so this is lower stakes than the IPv4-mapped case, but kept
///   explicit rather than left as an incidental collision.
fn accept_rate_limit_key(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(_) => ip,
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return IpAddr::V4(v4);
            }
            if v6.is_loopback() {
                return ip;
            }
            let mut octets = v6.octets();
            octets[8..].fill(0);
            IpAddr::V6(Ipv6Addr::from(octets))
        }
    }
}

/// Accept a connection, retrying on error instead of propagating it —
/// mirrors `axum::serve`'s own [`Listener::accept`](axum::serve::Listener)
/// behavior (`axum-0.8.9/src/serve/listener.rs`): errors in the
/// `ConnectionRefused`/`ConnectionAborted`/`ConnectionReset` class (a peer
/// that reset the connection between the kernel completing the handshake
/// and userspace calling `accept()`) are retried immediately, and any other
/// error (e.g. `EMFILE`) is logged and retried after a 1s backoff, so a
/// single transient accept failure never kills the accept loop.
async fn accept_with_retry(listener: &TcpListener) -> (tokio::net::TcpStream, SocketAddr) {
    loop {
        match listener.accept().await {
            Ok(accepted) => return accepted,
            Err(error) if is_connection_error(&error) => continue,
            Err(error) => {
                error!(%error, "accept error, retrying after backoff");
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

fn is_connection_error(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::ConnectionRefused
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
    )
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use super::*;

    #[test]
    fn ipv4_addresses_pass_through_unmasked() {
        let ip = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7));
        assert_eq!(accept_rate_limit_key(ip), ip);
    }

    #[test]
    fn ipv6_addresses_sharing_a_64_prefix_mask_to_the_same_key() {
        let a: IpAddr = "2001:db8:1234:5678:aaaa:bbbb:cccc:dddd".parse().unwrap();
        let b: IpAddr = "2001:db8:1234:5678:1111:2222:3333:4444".parse().unwrap();
        assert_eq!(accept_rate_limit_key(a), accept_rate_limit_key(b));
    }

    #[test]
    fn ipv6_addresses_with_different_64_prefixes_mask_to_different_keys() {
        let a: IpAddr = "2001:db8:1234:5678::1".parse().unwrap();
        let b: IpAddr = "2001:db8:1234:5679::1".parse().unwrap();
        assert_ne!(accept_rate_limit_key(a), accept_rate_limit_key(b));
    }

    #[test]
    fn masked_ipv6_key_zeroes_exactly_the_low_64_bits() {
        let ip: IpAddr = "2001:db8:1234:5678:ffff:ffff:ffff:ffff".parse().unwrap();
        let expected: IpAddr = "2001:db8:1234:5678::".parse().unwrap();
        assert_eq!(accept_rate_limit_key(ip), expected);
    }

    /// Regression for critic finding S2: on a dual-stack `[::]:port`
    /// listener, distinct IPv4 clients are reported as distinct
    /// IPv4-mapped IPv6 addresses (`::ffff:a.b.c.d`), which must not
    /// collapse onto a single shared key -- otherwise 64 connections from
    /// one attacker would exhaust the per-IP budget for every IPv4 client
    /// behind the dual-stack listener.
    #[test]
    fn ipv4_mapped_ipv6_addresses_are_not_collapsed_into_one_key() {
        let a: IpAddr = "::ffff:203.0.113.7".parse().unwrap();
        let b: IpAddr = "::ffff:198.51.100.99".parse().unwrap();
        assert_ne!(
            accept_rate_limit_key(a),
            accept_rate_limit_key(b),
            "distinct IPv4-mapped addresses must not share a per-IP rate-limit key"
        );
    }

    /// Regression for critic finding S2: an IPv4-mapped address must key
    /// the same way its plain IPv4 form would, not fall back to the
    /// `::/64` bucket shared by loopback and other zero-prefix addresses.
    #[test]
    fn ipv4_mapped_ipv6_address_keys_like_its_ipv4_form() {
        let mapped: IpAddr = "::ffff:203.0.113.7".parse().unwrap();
        let plain = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7));
        assert_ne!(
            accept_rate_limit_key(mapped),
            accept_rate_limit_key(IpAddr::V6(Ipv6Addr::UNSPECIFIED)),
            "an IPv4-mapped address must not key as the unspecified `::/64` bucket"
        );
        assert_eq!(
            accept_rate_limit_key(mapped),
            accept_rate_limit_key(plain),
            "an IPv4-mapped address should key identically to its plain IPv4 form"
        );
    }
}
