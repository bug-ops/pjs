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

use std::{io, net::SocketAddr, sync::Arc, time::Duration};

use axum::{Extension, Router, extract::ConnectInfo};
use hyper_util::{
    rt::{TokioExecutor, TokioIo, TokioTimer},
    server::conn::auto::Builder,
    service::TowerToHyperService,
};
use tokio::{net::TcpListener, sync::Semaphore};
use tower::Layer;
use tracing::{debug, error};

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
}

impl Default for ConnectionLimits {
    fn default() -> Self {
        Self {
            header_read_timeout: Some(Duration::from_secs(10)),
            max_connection_duration: Some(Duration::from_secs(300)),
            max_connections: 1024,
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
    if let Some(header_read_timeout) = limits.header_read_timeout {
        builder.http1().header_read_timeout(header_read_timeout);
    }
    let builder = Arc::new(builder);
    let semaphore = Arc::new(Semaphore::new(limits.max_connections));
    let max_connection_duration = limits.max_connection_duration;

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

        let io = TokioIo::new(stream);
        let peer_service = Extension(ConnectInfo(peer_addr)).layer(router.clone());
        let service = TowerToHyperService::new(peer_service);
        let builder = Arc::clone(&builder);

        tokio::spawn(async move {
            let _permit = permit;
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
