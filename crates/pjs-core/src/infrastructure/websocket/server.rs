//! WebSocket server implementation for Axum

use super::{
    AdaptiveStreamController, ConnectionId, CreateSessionError, WriteFailure, WsMessage,
    controller::{connection_plan_budget_for, spawn_task},
    planner::MAX_QUEUED_OUTGOING_BYTES,
    protocol::{ServerReply, bounded_log_text, invalid_stream_option},
    send_with_write_timeout,
};
use crate::{
    Error as PjsError, Result as PjsResult,
    domain::SessionId,
    error::{ErrorChain, SessionIdViolation},
    infrastructure::{
        bounded_channel::{self, byte_bounded_channel},
        http::ShutdownSignal,
    },
    security::{ClientRateLimiter, RateLimitConfig, RateLimitGuard},
};
use axum::{
    extract::{
        ConnectInfo, State, WebSocketUpgrade,
        ws::{CloseFrame, Message, WebSocket},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
};
use futures::{
    StreamExt,
    stream::{SplitSink, SplitStream},
};
use std::net::{IpAddr, SocketAddr};
use std::num::NonZeroUsize;
use std::ops::ControlFlow;
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{Instrument, debug, error, info, warn};

/// Close code (RFC 6455) for a connection that violated the server's policy.
const CLOSE_POLICY_VIOLATION: u16 = 1008;

/// Close code (RFC 6455) sent to every open connection on server shutdown.
const CLOSE_GOING_AWAY: u16 = 1001;

/// A reply to an inbound message. The connection loop writes it straight to
/// the socket, so a full frame queue cannot drop it.
#[derive(Debug, Clone, Copy)]
enum Reply {
    Error {
        session_id: Option<SessionId>,
        reply: ServerReply,
    },
    Pong {
        timestamp: u64,
    },
}

impl Reply {
    const fn error(session_id: Option<SessionId>, reply: ServerReply) -> Self {
        Self::Error { session_id, reply }
    }
}

impl From<Reply> for WsMessage {
    fn from(reply: Reply) -> Self {
        match reply {
            Reply::Error { session_id, reply } => WsMessage::Error {
                session_id,
                error: reply.reason().to_owned(),
                code: reply.code(),
            },
            Reply::Pong { timestamp } => WsMessage::Pong { timestamp },
        }
    }
}

/// Result of handling one inbound message: the reply to send, if any, and
/// whether the message was accepted.
struct Handled {
    reply: Option<Reply>,
    outcome: PjsResult<()>,
}

impl Handled {
    const fn ok() -> Self {
        Self {
            reply: None,
            outcome: Ok(()),
        }
    }

    const fn replied(reply: Reply) -> Self {
        Self {
            reply: Some(reply),
            outcome: Ok(()),
        }
    }

    fn rejected(reply: Reply, error: impl Into<PjsError>) -> Self {
        Self {
            reply: Some(reply),
            outcome: Err(error.into()),
        }
    }

    const fn failed(error: PjsError) -> Self {
        Self {
            reply: None,
            outcome: Err(error),
        }
    }
}

/// What [`AxumWebSocketTransport::send_frame`] did with a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
#[non_exhaustive]
pub enum SendOutcome {
    /// The frame is queued on the connection's outgoing channel.
    Queued,
    /// The frame was discarded for the given reason.
    Dropped(DropReason),
}

/// Why [`AxumWebSocketTransport::send_frame`] discarded a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DropReason {
    /// The connection has no outgoing channel.
    NoConnection,
    /// The message could not be serialized.
    Unserializable,
    /// Queuing the frame would exceed the connection's byte budget.
    OverBudget,
    /// The outgoing channel is full or closed.
    ChannelUnavailable,
}

/// Capacity of each per-connection outgoing message channel.
///
/// Bounds how many frames can queue for a slow client before `send_frame`
/// drops further frames rather than growing memory without limit (see
/// `send_frame`'s doc for why it drops instead of awaiting capacity).
/// This is a message-count bound only; [`MAX_QUEUED_OUTGOING_BYTES`]
/// additionally bounds cumulative queued bytes, so a connection queuing
/// many large frames is capped well before it could reach
/// `OUTGOING_QUEUE_CAPACITY * max_frame_size`.
const OUTGOING_QUEUE_CAPACITY: usize = 1000;

/// How often the background sweep started by the first WebSocket upgrade checks for streaming
/// sessions older than [`SESSION_MAX_AGE`].
const SESSION_CLEANUP_INTERVAL: Duration = Duration::from_mins(1);

/// Maximum age of a controller-tracked streaming session before the
/// background sweep removes it (aborting its streaming task) even if the
/// owning connection's teardown never ran, e.g. a session created but never
/// associated with a live connection.
const SESSION_MAX_AGE: Duration = Duration::from_hours(1);

/// Pre-resolved `Origin` allow-list policy for [`AxumWebSocketTransport::upgrade_handler`].
///
/// Resolved once at construction (see [`AxumWebSocketTransport::with_allowed_origins`])
/// from a `Vec<String>` with the same config semantics as
/// [`crate::infrastructure::http::axum_adapter::build_cors_layer_from_origins`]'s
/// `allowed_origins`, rather than re-parsing the list on every upgrade request.
#[derive(Debug, Clone)]
enum OriginAllowList {
    /// `[]` — deny all cross-origin upgrades (fail-closed default).
    DenyAll,
    /// `["*"]` — allow any origin.
    ///
    /// **This is more permissive on a WebSocket endpoint than the
    /// equivalent CORS `Any` on an HTTP endpoint.** Browsers refuse to
    /// send credentials (cookies) on a CORS request whose response is
    /// `Access-Control-Allow-Origin: *`, so wildcard CORS can't itself be
    /// used to steal a credentialed session. WebSocket has no such rule —
    /// the browser attaches ambient credentials to the handshake
    /// regardless of what the server's `Origin` policy turns out to be.
    /// Setting this fully re-enables the CSWSH this allow-list exists to
    /// prevent; only use it for endpoints that perform their own
    /// authentication and don't rely on browser ambient credentials.
    Any,
    /// Explicit origin list, matched by case-sensitive byte equality
    /// against the `Origin` header value.
    Explicit(Vec<axum::http::HeaderValue>),
}

impl OriginAllowList {
    /// Resolve a raw `allowed_origins` list into a policy.
    ///
    /// Mixing `"*"` with explicit origins is treated as [`Self::DenyAll`]
    /// (fail-closed) rather than a construction error: unlike
    /// `build_cors_layer_from_origins`, this is called from a builder that
    /// returns `Self`, not `Result`.
    fn resolve(allowed_origins: &[String]) -> Self {
        let has_wildcard = allowed_origins.iter().any(|o| o == "*");
        let has_explicit = allowed_origins.iter().any(|o| o != "*");

        match (allowed_origins.is_empty(), has_wildcard, has_explicit) {
            (true, _, _) => OriginAllowList::DenyAll,
            (_, true, true) => OriginAllowList::DenyAll,
            (_, true, false) => OriginAllowList::Any,
            (_, false, _) => OriginAllowList::Explicit(
                allowed_origins
                    .iter()
                    .filter_map(|o| Self::parse_explicit_origin(o))
                    .collect(),
            ),
        }
    }

    /// Parse one explicit `allowed_origins` entry, warning about entries
    /// that can never match a real browser `Origin` header instead of
    /// silently accepting them.
    ///
    /// A real `Origin` value is always a lowercase `scheme://host[:port]`
    /// with no path. An entry like `"example.com"` (missing scheme),
    /// `"https://example.com/"` (trailing path), or
    /// `"HTTPS://Example.com"` (uppercase) still parses as a valid
    /// `HeaderValue` and is kept fail-closed, but can never equal an
    /// actual `Origin` header byte-for-byte — making that entry an
    /// effective silent deny with no other diagnostic, unlike
    /// `build_cors_layer_from_origins`, which hard-errors on unparseable
    /// origins. An entry that fails to parse as a `HeaderValue` at all is
    /// dropped (it could never match anything).
    fn parse_explicit_origin(origin: &str) -> Option<axum::http::HeaderValue> {
        // `"null"` is a legitimate `Origin` value per the Fetch/HTML spec —
        // sent by sandboxed iframes, `file://` pages, and some redirected
        // requests — not a malformed entry, so it's exempt from the
        // shape check below.
        let looks_like_origin = origin == "null"
            || origin.split_once("://").is_some_and(|(scheme, rest)| {
                !rest.contains('/')
                    && !scheme.bytes().any(|b| b.is_ascii_uppercase())
                    && !rest.bytes().any(|b| b.is_ascii_uppercase())
            });
        if !looks_like_origin {
            warn!(
                "WebSocket allowed_origins entry {origin:?} does not look like a real Origin \
                 (expected lowercase `scheme://host[:port]` with no path) and will likely never \
                 match a real request"
            );
        }

        match origin.parse::<axum::http::HeaderValue>() {
            Ok(value) => Some(value),
            Err(e) => {
                warn!(
                    "WebSocket allowed_origins entry {origin:?} is not a valid header value \
                     and is being dropped: {e}"
                );
                None
            }
        }
    }

    /// Whether a present `Origin` header value is allowed.
    fn allows(&self, origin: &axum::http::HeaderValue) -> bool {
        match self {
            OriginAllowList::DenyAll => false,
            OriginAllowList::Any => true,
            OriginAllowList::Explicit(list) => list.iter().any(|o| o == origin),
        }
    }
}

/// Axum WebSocket transport implementation
pub struct AxumWebSocketTransport {
    controller: Arc<AdaptiveStreamController>,
    /// Per-IP rate limiter applied to upgrade requests, connection establishment,
    /// and inbound application-level messages.
    rate_limiter: Arc<ClientRateLimiter>,
    /// `Origin` allow-list applied to WebSocket upgrades, to block
    /// cross-site WebSocket hijacking (CSWSH) from browser clients. See
    /// [`Self::with_allowed_origins`].
    allowed_origins: OriginAllowList,
    shutdown: Option<ShutdownSignal>,
    max_sessions_per_connection: NonZeroUsize,
    /// Set once the background session-cleanup sweep has been started.
    sweep_started: OnceLock<()>,
    #[cfg(test)]
    upgrade_callback_delay: Option<Duration>,
}

impl AxumWebSocketTransport {
    /// Create a transport with the default rate-limit configuration.
    ///
    /// See [`RateLimitConfig::default`] for the limits applied.
    pub fn new() -> Self {
        Self::with_rate_limit_config(RateLimitConfig::default())
    }

    /// Create a transport with an explicit rate-limit configuration.
    ///
    /// Use [`RateLimitConfig::high_traffic`] or [`RateLimitConfig::low_resource`]
    /// for preset profiles, or construct a custom [`RateLimitConfig`].
    ///
    /// Construction needs no Tokio runtime. The background sweep that
    /// periodically aborts streaming sessions older than `SESSION_MAX_AGE`
    /// (via [`AdaptiveStreamController::cleanup_expired_sessions`]) starts on
    /// the first WebSocket upgrade.
    ///
    /// # Examples
    ///
    /// ```
    /// use pjson_rs::infrastructure::websocket::AxumWebSocketTransport;
    /// use pjson_rs::security::RateLimitConfig;
    ///
    /// // No runtime is running here.
    /// let _transport = AxumWebSocketTransport::with_rate_limit_config(RateLimitConfig::default());
    /// ```
    pub fn with_rate_limit_config(config: RateLimitConfig) -> Self {
        Self {
            controller: Arc::new(
                AdaptiveStreamController::new()
                    .with_connection_plan_budget(connection_plan_budget_for(config.max_frame_size)),
            ),
            rate_limiter: Arc::new(ClientRateLimiter::new(config)),
            allowed_origins: OriginAllowList::DenyAll,
            shutdown: None,
            max_sessions_per_connection: super::controller::DEFAULT_MAX_SESSIONS_PER_CONNECTION,
            sweep_started: OnceLock::new(),
            #[cfg(test)]
            upgrade_callback_delay: None,
        }
    }

    /// Starts the session-cleanup sweep the first time it is called.
    ///
    /// Must run inside a Tokio runtime; the first upgrade request provides
    /// one, which is why construction does not spawn. The sweep holds only a
    /// [`std::sync::Weak`] reference to the controller, so it exits once every
    /// `Arc<AdaptiveStreamController>` is dropped. A panic inside one sweep
    /// pass is logged and the next tick still runs.
    fn start_cleanup_sweep_once(&self) {
        self.sweep_started.get_or_init(|| {
            let weak_controller = Arc::downgrade(&self.controller);
            let signal = self.shutdown.clone();
            spawn_task(self.shutdown.as_ref(), async move {
                let mut interval = tokio::time::interval(SESSION_CLEANUP_INTERVAL);
                loop {
                    tokio::select! {
                        () = wait_for_shutdown(signal.as_ref()) => break,
                        _ = interval.tick() => {}
                    }
                    let Some(controller) = weak_controller.upgrade() else {
                        break;
                    };
                    let pass = std::panic::catch_unwind(AssertUnwindSafe(|| {
                        controller.cleanup_expired_sessions(SESSION_MAX_AGE);
                    }));
                    if pass.is_err() {
                        error!("session cleanup sweep panicked; continuing");
                    }
                }
            });
        });
    }

    /// Ties the transport to a [`ShutdownSignal`] shared with
    /// [`serve_with_limits`](crate::infrastructure::http::serve_with_limits).
    ///
    /// Serve and the transport must hold clones of one signal: with a
    /// different instance the serve drain does not wait for this transport's
    /// tasks. Once the signal fires, every open connection receives a Close
    /// frame with code 1001 and ends; the drain also waits for session tasks
    /// and force-drops them on timeout.
    ///
    /// Call this while building the transport, before the controller is used:
    /// it replaces the controller with one that spawns on the signal's
    /// tracker. Constructing the transport has no side effects until the
    /// first upgrade.
    ///
    /// # Examples
    ///
    /// ```
    /// use pjson_rs::infrastructure::http::ShutdownSignal;
    /// use pjson_rs::infrastructure::websocket::AxumWebSocketTransport;
    ///
    /// let signal = ShutdownSignal::new();
    /// let _transport = AxumWebSocketTransport::new().with_shutdown(signal.clone());
    /// ```
    #[must_use]
    pub fn with_shutdown(mut self, signal: ShutdownSignal) -> Self {
        self.shutdown = Some(signal);
        self.rebuild_controller();
        self
    }

    /// Caps the live streaming sessions one connection may hold (default 64).
    ///
    /// A session is live from its accepted `StreamInit` until its stream
    /// completes, is cancelled, or the connection closes. A `StreamInit` over
    /// the cap is answered with an `Error` reply (code 429) and creates no
    /// session. Call this while building the transport, before the controller
    /// is used.
    ///
    /// The cap is defence in depth against session accumulation. The default
    /// per-IP message rate limit (`RateLimitConfig::max_messages_per_second`,
    /// 30, plus a burst allowance of 5; every inbound frame counts) stops a
    /// burst well short of 64 sessions, but not a paced client: one that
    /// stays under the rate limit reaches 64 live sessions, provided it
    /// keeps them open for more than about 2 s (or the rate limit is raised),
    /// and only this cap stops it.
    ///
    /// # Examples
    ///
    /// ```
    /// use pjson_rs::infrastructure::websocket::AxumWebSocketTransport;
    /// use std::num::NonZeroUsize;
    ///
    /// let _transport = AxumWebSocketTransport::new()
    ///     .with_max_sessions_per_connection(NonZeroUsize::new(8).unwrap());
    /// ```
    #[must_use]
    pub fn with_max_sessions_per_connection(mut self, limit: NonZeroUsize) -> Self {
        self.max_sessions_per_connection = limit;
        self.rebuild_controller();
        self
    }

    /// Replaces the controller with one built from the current settings, so
    /// the builder methods compose in any order.
    fn rebuild_controller(&mut self) {
        let mut controller = AdaptiveStreamController::new()
            .with_max_sessions_per_connection(self.max_sessions_per_connection)
            .with_connection_plan_budget(connection_plan_budget_for(
                self.rate_limiter.config().max_frame_size,
            ));
        if let Some(signal) = &self.shutdown {
            controller = controller.with_shutdown(signal.clone());
        }
        self.controller = Arc::new(controller);
    }

    #[cfg(test)]
    fn with_upgrade_callback_delay(mut self, delay: Duration) -> Self {
        self.upgrade_callback_delay = Some(delay);
        self
    }

    /// Restrict WebSocket upgrades to the given `Origin` allow-list.
    ///
    /// Reuses the config semantics of
    /// [`HttpServerConfig::allowed_origins`](crate::infrastructure::http::axum_adapter::HttpServerConfig::allowed_origins)'s
    /// CORS allow-list:
    /// - `[]` (the default) — deny all cross-origin upgrades (fail-closed)
    /// - `["*"]` — allow any origin. **More dangerous here than the
    ///   equivalent CORS `Any`**: browsers attach ambient credentials to a
    ///   WebSocket handshake regardless of the server's `Origin` response,
    ///   unlike CORS, so a wildcard here fully re-enables the CSWSH this
    ///   allow-list exists to prevent.
    /// - `"*"` mixed with explicit origins — treated as deny-all (fail
    ///   closed); unlike the CORS layer this cannot be surfaced as a
    ///   construction error, since this builder returns `Self`
    ///
    /// Explicit entries that can never match a real `Origin` header (no
    /// `scheme://`, a trailing path, or uppercase letters) are kept
    /// fail-closed but logged with `warn!`, since they'd otherwise silently
    /// deny every browser connection with no diagnostic.
    ///
    /// This only governs requests that *carry* an `Origin` header. A
    /// request without one is always allowed to upgrade regardless of this
    /// list — see [`Self::upgrade_handler`] for why that is safe.
    pub fn with_allowed_origins(mut self, allowed_origins: Vec<String>) -> Self {
        self.allowed_origins = OriginAllowList::resolve(&allowed_origins);
        self
    }

    /// Handle WebSocket upgrade for Axum.
    ///
    /// Extracts the peer address via [`ConnectInfo`] and rejects upgrade
    /// requests that exceed the per-IP request budget with HTTP 429 before any
    /// WebSocket frames are exchanged.
    ///
    /// Also rejects, with HTTP 403, upgrades carrying an `Origin` header not
    /// in [`Self::with_allowed_origins`]'s allow-list — see that method and
    /// the check's own doc comment below for the CSWSH threat model and why
    /// a missing `Origin` header is allowed.
    ///
    /// Configures axum/tungstenite's transport-level `max_message_size` and
    /// `max_frame_size` from the transport's [`RateLimitConfig::max_frame_size`],
    /// so an oversized frame is rejected during frame assembly instead of
    /// being fully buffered first and only rejected afterward by the
    /// application-level `check_message` call (which remains as
    /// defense-in-depth for messages under the transport cap but still over
    /// policy in other ways).
    ///
    /// The router must be served with
    /// `into_make_service_with_connect_info::<SocketAddr>()` so the peer
    /// address is populated; otherwise the upgrade response is HTTP 500.
    pub async fn upgrade_handler(
        ws: WebSocketUpgrade,
        ConnectInfo(addr): ConnectInfo<SocketAddr>,
        headers: axum::http::HeaderMap,
        State(transport): State<Arc<Self>>,
    ) -> Response {
        let client_ip = addr.ip();
        transport.start_cleanup_sweep_once();

        if let Err(e) = transport.rate_limiter.check_request(client_ip) {
            warn!(%client_ip, error = %e, "WebSocket upgrade denied");
            return (StatusCode::TOO_MANY_REQUESTS, e.to_string()).into_response();
        }

        // Browsers always attach `Origin` to a WebSocket handshake, and
        // CSWSH depends on the browser sending that header along with
        // ambient credentials (cookies). A missing `Origin` therefore
        // cannot be a browser exploiting CSWSH — it's a native client, e.g.
        // `PjsWebSocketClient` or a non-browser tool, none of which send
        // one. Rejecting those would break every native client while
        // gaining no CSWSH protection, so an absent header is always
        // allowed here regardless of `allowed_origins`. Combined with the
        // fail-closed `DenyAll` default, this means browser clients are
        // refused by default while native clients keep working.
        if let Some(origin) = headers.get(axum::http::header::ORIGIN)
            && !transport.allowed_origins.allows(origin)
        {
            warn!(
                %client_ip,
                origin = %bounded_log_text(&String::from_utf8_lossy(origin.as_bytes())),
                "WebSocket upgrade rejected: disallowed Origin"
            );
            return StatusCode::FORBIDDEN.into_response();
        }

        let max_frame_size = transport.rate_limiter.config().max_frame_size;
        let ws = ws
            .max_message_size(max_frame_size)
            .max_frame_size(max_frame_size);

        // Taken before the 101 response leaves, so a drain cannot observe
        // "closed and empty" between the response and the callback below.
        let token = transport.shutdown.as_ref().map(ShutdownSignal::token);
        ws.on_upgrade(move |socket| async move {
            #[cfg(test)]
            if let Some(delay) = transport.upgrade_callback_delay {
                tokio::time::sleep(delay).await;
            }
            match (transport.shutdown.clone(), token) {
                (Some(signal), Some(token)) => {
                    if !signal.is_forced() {
                        signal
                            .spawn_tracked(Arc::clone(&transport).handle_socket(socket, client_ip));
                    }
                    drop(token);
                }
                _ => transport.handle_socket(socket, client_ip).await,
            }
        })
    }

    /// Handle WebSocket connection lifecycle
    pub async fn handle_socket(self: Arc<Self>, socket: WebSocket, client_ip: IpAddr) {
        let connection_id = ConnectionId::new();
        let span = tracing::info_span!("ws_connection", %connection_id, %client_ip);
        self.run_connection(socket, client_ip, connection_id)
            .instrument(span)
            .await;
    }

    async fn run_connection(
        self: Arc<Self>,
        socket: WebSocket,
        client_ip: IpAddr,
        connection_id: ConnectionId,
    ) {
        info!("WebSocket connection established");

        let write_timeout = self.rate_limiter.config().write_timeout;

        let guard = match RateLimitGuard::new(self.rate_limiter.clone(), client_ip) {
            Ok(guard) => guard,
            Err(e) => {
                warn!(error = %e, "WebSocket connection rejected (rate limit)");
                let (mut sender, _) = socket.split();
                close_policy_violation(&mut sender, e.to_string(), write_timeout).await;
                return;
            }
        };

        let (outgoing_tx, outgoing_rx) =
            byte_bounded_channel::<String>(OUTGOING_QUEUE_CAPACITY, MAX_QUEUED_OUTGOING_BYTES);
        self.controller
            .attach_connection(connection_id, outgoing_tx);
        let _cleanup = ConnectionCleanup {
            transport: Arc::clone(&self),
            connection_id,
        };

        #[cfg(feature = "metrics")]
        let _connection_gauge =
            crate::infrastructure::http::metrics::WebSocketConnectionGauge::open();

        let (sender, receiver) = socket.split();
        ConnectionLoop {
            transport: &self,
            connection_id,
            write_timeout,
            guard: &guard,
            sender,
        }
        .run(outgoing_rx, receiver)
        .await;

        info!("WebSocket connection closed");
    }

    /// Returns a shared handle to the underlying [`AdaptiveStreamController`].
    pub fn controller(&self) -> Arc<AdaptiveStreamController> {
        self.controller.clone()
    }

    /// Returns the number of currently active WebSocket connections.
    ///
    /// Useful for observability, health endpoints, and integration tests.
    pub fn active_connection_count(&self) -> usize {
        self.controller.connection_count()
    }

    /// Queues `message` for `connection`'s outgoing channel without blocking.
    ///
    /// The channel is drained by the same `tokio::select!` loop that also
    /// awaits message handling inline. Calling this from within that
    /// inline handling path (directly or transitively) must therefore never
    /// block: a blocking send would wait forever on a receiver that cannot
    /// run until the in-flight branch finishes. Using `try_send` keeps that
    /// hazard from becoming a real deadlock.
    ///
    /// Returns [`SendOutcome::Dropped`] with the reason when the frame is not
    /// queued (no connection, serialization failure, byte budget exceeded, or
    /// channel full or closed); every drop is logged via `warn!`.
    pub fn send_frame(&self, connection: ConnectionId, message: WsMessage) -> SendOutcome {
        let Some(tx) = self.controller.outgoing_for(connection) else {
            warn!(%connection, "send_frame: no outgoing channel");
            return SendOutcome::Dropped(DropReason::NoConnection);
        };
        // Serialized once here, rather than in the consuming loop: this is
        // also what the byte-budget check in `try_send` measures, so the
        // queued-bytes accounting matches the actual bytes held in memory.
        Self::queue_serialized(&tx, connection, serde_json::to_string(&message))
    }

    fn queue_serialized(
        tx: &bounded_channel::ByteBoundedSender<String>,
        connection: ConnectionId,
        serialized: serde_json::Result<String>,
    ) -> SendOutcome {
        let json_str = match serialized {
            Ok(json_str) => json_str,
            Err(e) => {
                warn!(%connection, error = %e, "send_frame: failed to serialize frame");
                return SendOutcome::Dropped(DropReason::Unserializable);
            }
        };
        let len = json_str.len();
        match tx.try_send(json_str, len) {
            Ok(()) => SendOutcome::Queued,
            Err(bounded_channel::TrySendError::BudgetExceeded(_)) => {
                warn!(%connection, len, "send_frame: dropping frame (byte budget exceeded)");
                SendOutcome::Dropped(DropReason::OverBudget)
            }
            Err(bounded_channel::TrySendError::Channel(_)) => {
                warn!(%connection, "send_frame: dropping frame (channel full or closed)");
                SendOutcome::Dropped(DropReason::ChannelUnavailable)
            }
        }
    }

    /// Handles one decoded message from `connection` and queues its reply.
    ///
    /// The reply goes through [`Self::send_frame`] and is dropped when the
    /// connection's queue is full; the connection loop writes replies
    /// directly instead, so a full queue never loses them on the wire.
    ///
    /// The `StreamInit` arm adopts the peer's session id and records
    /// `connection` as its owner, so the connection's teardown can abort it on
    /// disconnect. A nil or already used id is answered with an `Error` reply
    /// (400 and 409) and returned as an error; the existing session is
    /// untouched. `FrameAck`s are accepted from the owner only. An
    /// application-level `Ping` is answered with a `Pong` carrying the same
    /// timestamp.
    ///
    /// Session ids are chosen by the peer and scoped to its connection: two
    /// connections may use the same id without interacting, and an
    /// acknowledgment or cancellation only reaches a session of the
    /// connection that sends it. A duplicate id on one connection is a 409.
    pub async fn handle_message(
        &self,
        connection: ConnectionId,
        message: WsMessage,
    ) -> PjsResult<()> {
        let Handled { reply, outcome } = self.dispatch(connection, message).await;
        if let Some(reply) = reply {
            // A reply the full queue drops is already logged by `send_frame`.
            let _ = self.send_frame(connection, reply.into());
        }
        outcome
    }

    /// Handles one decoded message from `connection` and returns the reply
    /// for the caller to deliver; semantics are documented on
    /// [`Self::handle_message`].
    async fn dispatch(&self, connection: ConnectionId, message: WsMessage) -> Handled {
        match message {
            WsMessage::StreamInit {
                session_id,
                data,
                options,
            } => {
                if session_id.as_uuid().is_nil() {
                    return Handled::rejected(
                        Reply::error(None, ServerReply::NilSessionId),
                        PjsError::InvalidSessionId(SessionIdViolation::Nil),
                    );
                }
                match self
                    .controller
                    .create_session(session_id, connection, data, options)
                    .await
                {
                    Ok(()) => {}
                    Err(e @ CreateSessionError::AlreadyExists(_)) => {
                        return Handled::rejected(
                            Reply::error(Some(session_id), ServerReply::DuplicateSessionId),
                            e,
                        );
                    }
                    Err(e @ CreateSessionError::TooManySessions { .. }) => {
                        return Handled::rejected(
                            Reply::error(Some(session_id), ServerReply::TooManySessions),
                            e,
                        );
                    }
                    Err(e @ CreateSessionError::PlanRejected(plan_error)) => {
                        return Handled::rejected(
                            Reply::error(Some(session_id), plan_error.into()),
                            e,
                        );
                    }
                }
                if let Err(e) = self.controller.start_streaming(connection, session_id) {
                    return Handled::failed(e);
                }
                info!(%connection, %session_id, "Created new streaming session");
            }
            WsMessage::FrameAck {
                session_id,
                frame_id,
                processing_time_ms,
            } => {
                debug!(%session_id, frame_id, processing_time_ms, "Received frame ack");
                if let Err(e) = self.controller.handle_frame_ack(
                    connection,
                    session_id,
                    frame_id,
                    processing_time_ms,
                ) {
                    debug!(%connection, error = %ErrorChain(&e), "Ignored frame ack");
                }
            }
            WsMessage::StreamCancel { session_id } => {
                if !self.controller.remove_session(connection, session_id) {
                    debug!(%connection, %session_id, "Ignored cancel of unknown session");
                }
            }
            WsMessage::Ping { timestamp } => {
                debug!(timestamp, "Received ping");
                return Handled::replied(Reply::Pong { timestamp });
            }
            WsMessage::Error {
                session_id,
                error,
                code,
            } => {
                // `error` is an arbitrary client-supplied string with no
                // length validation on this path — log only its length
                // (plus the connection id for correlation), never the
                // value itself, at WARN (see #415 S1: unbounded WARN
                // amplification from a single rate-limited connection).
                warn!(
                    %connection,
                    ?session_id,
                    code,
                    error_len = error.len(),
                    "Received error from client"
                );
            }
            // Server-to-client messages: a client sending them gets no reply
            // (no amplification); a new variant must be classified here.
            WsMessage::StreamFrame { .. }
            | WsMessage::StreamPatch { .. }
            | WsMessage::StreamComplete { .. }
            | WsMessage::Pong { .. } => {
                debug!(%connection, kind = message.kind(), "Ignored server-bound message");
            }
        }
        Handled::ok()
    }
}

impl Default for AxumWebSocketTransport {
    fn default() -> Self {
        Self::new()
    }
}

/// Removes a connection's channel and sessions when the connection
/// task ends, including when a forced drain drops the task mid-flight.
struct ConnectionCleanup {
    transport: Arc<AxumWebSocketTransport>,
    connection_id: ConnectionId,
}

impl Drop for ConnectionCleanup {
    fn drop(&mut self) {
        self.transport
            .controller
            .detach_connection(self.connection_id);
    }
}

/// Resolves when `signal` fires; never resolves when there is no signal.
async fn wait_for_shutdown(signal: Option<&ShutdownSignal>) {
    match signal {
        Some(signal) => signal.triggered().await,
        None => std::future::pending().await,
    }
}

/// Writes a Close frame and discards a write failure: the connection is
/// ending either way.
async fn send_close(
    sender: &mut SplitSink<WebSocket, Message>,
    code: u16,
    reason: String,
    write_timeout: Duration,
) {
    let close = Message::Close(Some(CloseFrame {
        code,
        reason: reason.into(),
    }));
    if let Err(e) = send_with_write_timeout(sender, close, write_timeout).await {
        debug!(code, error = %e, "Failed to send Close frame");
    }
}

/// Closes the connection with [`CLOSE_POLICY_VIOLATION`].
async fn close_policy_violation(
    sender: &mut SplitSink<WebSocket, Message>,
    reason: String,
    write_timeout: Duration,
) {
    send_close(sender, CLOSE_POLICY_VIOLATION, reason, write_timeout).await;
}

/// Everything one connection's event loop needs besides the receive halves,
/// which stay local to [`Self::run`] so `select!` can borrow them while the
/// handlers borrow `self`.
struct ConnectionLoop<'a> {
    transport: &'a AxumWebSocketTransport,
    connection_id: ConnectionId,
    write_timeout: Duration,
    guard: &'a RateLimitGuard,
    sender: SplitSink<WebSocket, Message>,
}

impl ConnectionLoop<'_> {
    async fn run(
        mut self,
        mut outgoing_rx: mpsc::Receiver<bounded_channel::Envelope<String>>,
        mut receiver: SplitStream<WebSocket>,
    ) {
        loop {
            let flow = tokio::select! {
                () = wait_for_shutdown(self.transport.shutdown.as_ref()) => {
                    send_close(&mut self.sender, CLOSE_GOING_AWAY, String::new(), self.write_timeout).await;
                    ControlFlow::Break(())
                }
                // Already serialized at `send_frame` time. `split` (rather
                // than `into_inner`) keeps the byte budget charged until the
                // write actually completes, not just until the item leaves
                // the channel.
                Some(envelope) = outgoing_rx.recv() => {
                    let (json_str, _budget_permit) = envelope.split();
                    self.send_text(json_str, "outgoing message").await
                }
                Some(msg) = receiver.next() => self.on_inbound(msg).await,
                else => ControlFlow::Break(()),
            };
            if flow.is_break() {
                break;
            }
        }
    }

    async fn send_text(&mut self, json_str: String, what: &str) -> ControlFlow<()> {
        let len = json_str.len();
        let message = Message::Text(json_str.into());
        match send_with_write_timeout(&mut self.sender, message, self.write_timeout).await {
            Ok(()) => {
                record_ws_bytes(len);
                ControlFlow::Continue(())
            }
            Err(e) => {
                error!(what, error = %e, "Failed to send to client");
                ControlFlow::Break(())
            }
        }
    }

    async fn write_reply(&mut self, reply: Reply) -> ControlFlow<()> {
        match send_reply(&mut self.sender, &reply.into(), self.write_timeout).await {
            Ok(()) => ControlFlow::Continue(()),
            Err(e) => {
                error!(error = %e, "Failed to send reply to client");
                ControlFlow::Break(())
            }
        }
    }

    /// Applies the connection's message budget to an inbound frame of `len`
    /// bytes; over budget, closes with 1008 and returns `false`.
    async fn admit(&mut self, len: usize, kind: &str) -> bool {
        match self.guard.check_message(len) {
            Ok(()) => true,
            Err(e) => {
                warn!(kind, error = %e, "Inbound frame rejected (rate limit)");
                record_ws_rate_limited();
                close_policy_violation(&mut self.sender, e.to_string(), self.write_timeout).await;
                false
            }
        }
    }

    async fn on_inbound(&mut self, msg: Result<Message, axum::Error>) -> ControlFlow<()> {
        match msg {
            Ok(Message::Text(text)) => {
                if !self.admit(text.len(), "text").await {
                    return ControlFlow::Break(());
                }
                match serde_json::from_str::<WsMessage>(&text) {
                    Ok(ws_message) => {
                        debug!(
                            kind = ws_message.kind(),
                            bytes = text.len(),
                            "Inbound message"
                        );
                        let Handled { reply, outcome } = self
                            .transport
                            .dispatch(self.connection_id, ws_message)
                            .await;
                        if let Err(e) = outcome {
                            debug!(error = %ErrorChain(&e), "Rejected message");
                        }
                        match reply {
                            Some(reply) => self.write_reply(reply).await,
                            None => ControlFlow::Continue(()),
                        }
                    }
                    Err(e) => {
                        warn!(
                            class = ?e.classify(),
                            line = e.line(),
                            column = e.column(),
                            "Failed to parse WebSocket message"
                        );
                        let reply = invalid_stream_option(&text)
                            .map_or(ServerReply::Malformed, ServerReply::InvalidStreamOption);
                        self.write_reply(Reply::error(None, reply)).await
                    }
                }
            }
            Ok(Message::Binary(data)) => {
                if !self.admit(data.len(), "binary").await {
                    return ControlFlow::Break(());
                }
                debug!(bytes = data.len(), "Received binary data");
                ControlFlow::Continue(())
            }
            Ok(Message::Ping(data)) => {
                if !self.admit(data.len(), "ping").await {
                    return ControlFlow::Break(());
                }
                let pong = Message::Pong(data);
                match send_with_write_timeout(&mut self.sender, pong, self.write_timeout).await {
                    Ok(()) => ControlFlow::Continue(()),
                    Err(e) => {
                        error!(error = %e, "Failed to send pong");
                        ControlFlow::Break(())
                    }
                }
            }
            Ok(Message::Pong(data)) => {
                if !self.admit(data.len(), "pong").await {
                    return ControlFlow::Break(());
                }
                debug!("Received pong from client");
                ControlFlow::Continue(())
            }
            Ok(Message::Close(_)) => {
                info!("Client closed WebSocket connection");
                ControlFlow::Break(())
            }
            Err(e) => {
                match SocketErrorClass::of(&e) {
                    SocketErrorClass::PeerDisconnect => {
                        info!(error = %e, "WebSocket peer disconnected");
                    }
                    SocketErrorClass::PeerProtocol => {
                        warn!(error = %e, "WebSocket peer violated the protocol");
                    }
                    SocketErrorClass::Server => error!(error = %e, "WebSocket error"),
                }
                ControlFlow::Break(())
            }
        }
    }
}

/// Who caused a socket error, deciding its log level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SocketErrorClass {
    /// The peer's transport went away (reset, abort, broken pipe, early EOF).
    PeerDisconnect,
    /// The peer sent something invalid, or reset without a closing handshake.
    PeerProtocol,
    /// Any other I/O failure: not attributable to the peer.
    Server,
}

impl SocketErrorClass {
    /// Classifies by the `io::Error` in the source chain, if any. No downcast
    /// to `tungstenite::Error`: axum links tungstenite 0.29 while this crate
    /// depends on 0.30, so one would silently never match.
    fn of(error: &axum::Error) -> Self {
        let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
        while let Some(current) = source {
            if let Some(io) = current.downcast_ref::<std::io::Error>() {
                return match io.kind() {
                    std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::BrokenPipe
                    | std::io::ErrorKind::UnexpectedEof => Self::PeerDisconnect,
                    _ => Self::Server,
                };
            }
            source = current.source();
        }
        Self::PeerProtocol
    }
}

/// Why [`send_reply`] failed.
#[derive(Debug, thiserror::Error)]
enum ReplyFailure {
    #[error("serialize failed: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error(transparent)]
    Write(#[from] WriteFailure<axum::Error>),
}

/// Serializes `reply` and writes it straight to the socket.
async fn send_reply(
    sender: &mut SplitSink<WebSocket, Message>,
    reply: &WsMessage,
    write_timeout: Duration,
) -> Result<(), ReplyFailure> {
    let json = serde_json::to_string(reply)?;
    let len = json.len();
    send_with_write_timeout(sender, Message::Text(json.into()), write_timeout).await?;
    record_ws_bytes(len);
    Ok(())
}

#[cfg(feature = "metrics")]
fn record_ws_bytes(len: usize) {
    use crate::infrastructure::http::metrics::{Transport, record_bytes_sent};
    record_bytes_sent(Transport::WebSocket, len);
}

#[cfg(not(feature = "metrics"))]
fn record_ws_bytes(_len: usize) {}

#[cfg(feature = "metrics")]
fn record_ws_rate_limited() {
    use crate::infrastructure::http::metrics::{Transport, record_rate_limited};
    record_rate_limited(Transport::WebSocket);
}

#[cfg(not(feature = "metrics"))]
fn record_ws_rate_limited() {}

/// Helper function to create WebSocket router for Axum
pub fn create_websocket_router() -> axum::Router<Arc<AxumWebSocketTransport>> {
    use axum::routing::get;

    axum::Router::new().route("/ws", get(AxumWebSocketTransport::upgrade_handler))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::{
        http::{
            ConnectionLimits, DrainTimeout, ShutdownConfig, ShutdownOutcome, serve_with_limits,
        },
        websocket::StreamOptions,
    };
    use core::assert_matches;
    use serde_json::json;

    #[tokio::test]
    async fn test_transport_creation() {
        let transport = AxumWebSocketTransport::new();
        assert!(Arc::strong_count(&transport.controller) >= 1);
    }

    #[tokio::test]
    async fn test_stream_initialization() {
        let transport = AxumWebSocketTransport::new();
        let data = json!({
            "critical": {"id": 1, "status": "active"},
            "metadata": {"created": "2024-01-15T12:00:00Z"}
        });

        let owner = ConnectionId::new();
        let session_id = SessionId::new();
        transport
            .controller
            .create_session(session_id, owner, data, StreamOptions::default())
            .await
            .unwrap();

        transport
            .controller
            .start_streaming(owner, session_id)
            .unwrap();
    }

    fn stream_init(session_id: SessionId) -> WsMessage {
        WsMessage::StreamInit {
            session_id,
            data: json!({"test": "value"}),
            options: StreamOptions::default(),
        }
    }

    #[test]
    fn test_queue_serialized_reports_unserializable_without_queueing() {
        let (tx, mut rx) = byte_bounded_channel::<String>(4, 1024);
        let failure = serde_json::from_str::<()>("x").unwrap_err();

        let outcome =
            AxumWebSocketTransport::queue_serialized(&tx, ConnectionId::new(), Err(failure));

        assert_eq!(outcome, SendOutcome::Dropped(DropReason::Unserializable));
        assert!(rx.try_recv().is_err());
    }

    fn register_outgoing(
        transport: &AxumWebSocketTransport,
        connection: ConnectionId,
    ) -> tokio::sync::mpsc::Receiver<bounded_channel::Envelope<String>> {
        let (tx, rx) =
            byte_bounded_channel::<String>(OUTGOING_QUEUE_CAPACITY, MAX_QUEUED_OUTGOING_BYTES);
        transport.controller.attach_connection(connection, tx);
        rx
    }

    fn reply_code(envelope: bounded_channel::Envelope<String>) -> u16 {
        match serde_json::from_str::<WsMessage>(&envelope.into_inner()).unwrap() {
            WsMessage::Error { code, .. } => code,
            other => panic!("expected an Error reply, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_handle_message_stream_init_adopts_client_id_and_records_owner() {
        let transport = AxumWebSocketTransport::new();
        let connection = ConnectionId::new();
        let session_id = SessionId::new();

        transport
            .handle_message(connection, stream_init(session_id))
            .await
            .unwrap();

        assert_eq!(transport.controller.session_count(connection), 1);
    }

    #[tokio::test]
    async fn test_handle_message_duplicate_stream_init_on_one_connection_is_rejected_with_409() {
        let transport = AxumWebSocketTransport::new();
        let connection = ConnectionId::new();
        let mut rx = register_outgoing(&transport, connection);
        let session_id = SessionId::new();
        transport
            .handle_message(connection, stream_init(session_id))
            .await
            .unwrap();

        let result = transport
            .handle_message(connection, stream_init(session_id))
            .await;

        assert!(result.is_err());
        assert_eq!(
            reply_code(rx.try_recv().unwrap()),
            ServerReply::DuplicateSessionId.code()
        );
        assert_eq!(transport.controller.session_count(connection), 1);
    }

    #[tokio::test]
    async fn test_handle_message_same_id_on_another_connection_is_accepted() {
        let transport = AxumWebSocketTransport::new();
        let (first, second) = (ConnectionId::new(), ConnectionId::new());
        let session_id = SessionId::new();

        for connection in [first, second] {
            transport
                .handle_message(connection, stream_init(session_id))
                .await
                .unwrap();
        }

        assert_eq!(transport.controller.session_count(first), 1);
        assert_eq!(transport.controller.session_count(second), 1);
    }

    #[tokio::test]
    async fn test_handle_message_nil_stream_init_is_rejected_with_400() {
        let transport = AxumWebSocketTransport::new();
        let connection = ConnectionId::new();
        let mut rx = register_outgoing(&transport, connection);
        let nil = SessionId::from_uuid(uuid::Uuid::nil());

        let result = transport.handle_message(connection, stream_init(nil)).await;

        assert!(result.is_err());
        assert_eq!(
            reply_code(rx.try_recv().unwrap()),
            ServerReply::NilSessionId.code()
        );
        assert_eq!(transport.controller.session_count(connection), 0);
    }

    #[tokio::test]
    async fn test_handle_message_over_the_live_session_cap_is_rejected_with_429() {
        let transport = AxumWebSocketTransport::new()
            .with_max_sessions_per_connection(NonZeroUsize::new(64).unwrap());
        let connection = ConnectionId::new();
        let mut rx = register_outgoing(&transport, connection);
        for _ in 0..64 {
            let id = SessionId::new();
            transport
                .controller
                .create_session(id, connection, json!({}), StreamOptions::default())
                .await
                .unwrap();
        }

        let result = transport
            .handle_message(connection, stream_init(SessionId::new()))
            .await;

        assert!(result.is_err());
        assert_eq!(
            reply_code(rx.try_recv().unwrap()),
            ServerReply::TooManySessions.code()
        );
        assert_eq!(transport.controller.session_count(connection), 64);
    }

    #[tokio::test]
    async fn test_default_cap_is_64_live_sessions() {
        let transport = AxumWebSocketTransport::new();
        let connection = ConnectionId::new();
        let _rx = register_outgoing(&transport, connection);
        for _ in 0..64 {
            transport
                .controller
                .create_session(
                    SessionId::new(),
                    connection,
                    json!({}),
                    StreamOptions::default(),
                )
                .await
                .unwrap();
        }
        let over = transport
            .controller
            .create_session(
                SessionId::new(),
                connection,
                json!({}),
                StreamOptions::default(),
            )
            .await;
        assert!(over.is_err());
    }

    #[tokio::test]
    async fn test_stream_cancel_removes_the_session_and_unknown_id_is_a_no_op() {
        let transport = AxumWebSocketTransport::new();
        let connection = ConnectionId::new();
        let session_id = SessionId::new();
        transport
            .handle_message(connection, stream_init(session_id))
            .await
            .unwrap();

        transport
            .handle_message(connection, WsMessage::StreamCancel { session_id })
            .await
            .unwrap();
        transport
            .handle_message(connection, WsMessage::StreamCancel { session_id })
            .await
            .unwrap();

        assert_eq!(transport.controller.session_count(connection), 0);
    }

    #[tokio::test]
    async fn test_stream_cancel_from_another_connection_leaves_the_session() {
        let transport = AxumWebSocketTransport::new();
        let owner = ConnectionId::new();
        let session_id = SessionId::new();
        transport
            .handle_message(owner, stream_init(session_id))
            .await
            .unwrap();

        transport
            .handle_message(ConnectionId::new(), WsMessage::StreamCancel { session_id })
            .await
            .unwrap();

        assert_eq!(transport.controller.session_count(owner), 1);
    }

    #[tokio::test]
    async fn test_handle_message_ping_is_answered_with_pong_of_same_timestamp() {
        let transport = AxumWebSocketTransport::new();
        let connection = ConnectionId::new();
        let mut rx = register_outgoing(&transport, connection);

        transport
            .handle_message(connection, WsMessage::Ping { timestamp: 42 })
            .await
            .unwrap();

        let reply = serde_json::from_str::<WsMessage>(&rx.try_recv().unwrap().into_inner());
        assert_matches!(reply, Ok(WsMessage::Pong { timestamp: 42 }), "{reply:?}");
    }

    #[tokio::test]
    async fn test_handle_message_frame_ack_from_another_connection_changes_nothing() {
        let transport = AxumWebSocketTransport::new();
        let owner = ConnectionId::new();
        let session_id = SessionId::new();
        transport
            .handle_message(owner, stream_init(session_id))
            .await
            .unwrap();

        let result = transport
            .handle_message(
                ConnectionId::new(),
                WsMessage::FrameAck {
                    session_id,
                    frame_id: 0,
                    processing_time_ms: 5,
                },
            )
            .await;

        assert!(
            result.is_ok(),
            "an ack for a session not live here is ignored"
        );
        let acked = transport
            .controller
            .with_session_mut(owner, session_id, |s| s.client_metrics.frames_acknowledged)
            .unwrap();
        assert_eq!(acked, 0);
    }

    #[tokio::test]
    async fn test_reinitialising_a_cleaned_up_id_streams_to_the_new_session() {
        let transport = AxumWebSocketTransport::new();
        let connection = ConnectionId::new();
        let mut rx = register_outgoing(&transport, connection);
        let session_id = SessionId::new();
        transport
            .handle_message(connection, stream_init(session_id))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
        transport
            .controller
            .cleanup_expired_sessions(Duration::ZERO);
        while rx.try_recv().is_ok() {}

        transport
            .handle_message(connection, stream_init(session_id))
            .await
            .unwrap();

        let envelope = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("the new session must stream")
            .unwrap();
        assert_matches!(
            serde_json::from_str::<WsMessage>(&envelope.into_inner()),
            Ok(WsMessage::StreamFrame { session_id: id, .. }) if id == session_id
        );
    }

    #[tokio::test]
    async fn test_outgoing_channel_is_bounded() {
        // Regression test for #314: the per-connection outgoing channel
        // used to be unbounded, so a stalled consumer let it grow without
        // limit. It must now reject sends once `OUTGOING_QUEUE_CAPACITY`
        // is reached instead of growing memory indefinitely.
        let (tx, mut rx) =
            byte_bounded_channel::<String>(OUTGOING_QUEUE_CAPACITY, MAX_QUEUED_OUTGOING_BYTES);

        for _ in 0..OUTGOING_QUEUE_CAPACITY {
            tx.try_send("ping".to_string(), 4)
                .expect("channel should accept sends up to its capacity");
        }

        let result = tx.try_send("ping".to_string(), 4);
        assert_matches!(
            result,
            Err(bounded_channel::TrySendError::Channel(
                tokio::sync::mpsc::error::TrySendError::Full(_)
            )),
            "channel must reject sends past capacity instead of growing unbounded"
        );

        // Draining a slot frees capacity again — this is the flow-control
        // behavior an unbounded channel could never provide.
        rx.recv().await.expect("receiver should still be open");
        tx.try_send("ping".to_string(), 4)
            .expect("channel should accept a send after capacity is freed");
    }

    #[tokio::test]
    async fn test_send_frame_drops_when_byte_budget_exceeded() {
        // Regression test for #349: a message-count bound alone doesn't
        // bound queued bytes. A single frame larger than
        // `MAX_QUEUED_OUTGOING_BYTES` must be dropped even though the
        // channel is nowhere near its message-count capacity.
        let transport = AxumWebSocketTransport::new();
        let connection_id = ConnectionId::new();
        let (tx, mut rx) =
            byte_bounded_channel::<String>(OUTGOING_QUEUE_CAPACITY, MAX_QUEUED_OUTGOING_BYTES);
        transport.controller.attach_connection(connection_id, tx);

        let connection = connection_id;
        let oversized_message = WsMessage::Error {
            session_id: None,
            error: "x".repeat(MAX_QUEUED_OUTGOING_BYTES + 1),
            code: 0,
        };

        assert_eq!(
            transport.send_frame(connection, oversized_message),
            SendOutcome::Dropped(DropReason::OverBudget)
        );

        assert!(
            rx.try_recv().is_err(),
            "an over-budget frame must be dropped, not queued"
        );
    }

    #[tokio::test]
    async fn test_send_frame_drops_on_full_channel_without_blocking() {
        // Regression test for S3: exercises the real `send_frame` code
        // path (registered channel + read-lock clone) rather than an
        // isolated mpsc channel, proving that a full outgoing channel
        // makes `send_frame` drop-and-log via `try_send` instead of
        // blocking. `try_send` never awaits, so it also makes the earlier
        // deadlock hazard (this connection's own loop being both the
        // sender and the only consumer) moot; the sender is still cloned
        // out of the lock before sending for lock hygiene.
        let transport = AxumWebSocketTransport::new();
        let connection_id = ConnectionId::new();
        let (tx, mut rx) =
            byte_bounded_channel::<String>(OUTGOING_QUEUE_CAPACITY, MAX_QUEUED_OUTGOING_BYTES);
        transport.controller.attach_connection(connection_id, tx);

        let connection = connection_id;
        for _ in 0..OUTGOING_QUEUE_CAPACITY {
            assert_eq!(
                transport.send_frame(connection, WsMessage::Ping { timestamp: 0 }),
                SendOutcome::Queued
            );
        }

        assert_eq!(
            transport.send_frame(connection, WsMessage::Ping { timestamp: 0 }),
            SendOutcome::Dropped(DropReason::ChannelUnavailable)
        );

        rx.close();
        let mut drained = 0;
        while rx.try_recv().is_ok() {
            drained += 1;
        }
        assert_eq!(
            drained, OUTGOING_QUEUE_CAPACITY,
            "the overflow frame must have been dropped, not queued"
        );
    }

    struct RunningServer {
        addr: SocketAddr,
        signal: ShutdownSignal,
        transport: Arc<AxumWebSocketTransport>,
        serve: tokio::task::JoinHandle<std::io::Result<ShutdownOutcome>>,
    }

    async fn start_server(callback_delay: Duration, drain_timeout: Duration) -> RunningServer {
        let signal = ShutdownSignal::new();
        let transport = Arc::new(
            AxumWebSocketTransport::new()
                .with_shutdown(signal.clone())
                .with_upgrade_callback_delay(callback_delay),
        );
        let router = create_websocket_router().with_state(transport.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let config = ShutdownConfig {
            signal: signal.clone(),
            drain_timeout: DrainTimeout::try_from(drain_timeout).unwrap(),
        };
        let serve = tokio::spawn(serve_with_limits(
            listener,
            router,
            ConnectionLimits::default(),
            config,
        ));
        RunningServer {
            addr,
            signal,
            transport,
            serve,
        }
    }

    /// Performs the upgrade handshake by hand and returns once the `101`
    /// response headers are read, so no frame byte is consumed.
    async fn upgrade(addr: SocketAddr) -> tokio::net::TcpStream {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(
                b"GET /ws HTTP/1.1\r\nHost: test\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
                  Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n",
            )
            .await
            .unwrap();
        let mut head = Vec::new();
        while !head.ends_with(b"\r\n\r\n") {
            head.push(stream.read_u8().await.unwrap());
        }
        assert!(head.starts_with(b"HTTP/1.1 101"), "{head:?}");
        stream
    }

    /// Reads one unmasked server frame and returns its Close status code.
    async fn read_close_code(stream: &mut tokio::net::TcpStream) -> u16 {
        use tokio::io::AsyncReadExt;
        let opcode = stream.read_u8().await.unwrap() & 0x0f;
        let len = usize::from(stream.read_u8().await.unwrap() & 0x7f);
        let mut payload = vec![0; len];
        stream.read_exact(&mut payload).await.unwrap();
        assert_eq!(opcode, 0x8, "expected a Close frame");
        u16::from_be_bytes([payload[0], payload[1]])
    }

    #[tokio::test]
    async fn test_trigger_between_101_and_callback_does_not_drain_before_the_connection_closes() {
        let server = start_server(Duration::from_millis(300), Duration::from_secs(5)).await;
        let mut client = upgrade(server.addr).await;

        server.signal.trigger();
        let outcome = server.serve.await.unwrap().unwrap();

        assert_eq!(outcome, ShutdownOutcome::Drained);
        let code = tokio::time::timeout(Duration::from_millis(50), read_close_code(&mut client))
            .await
            .expect("the Close frame was sent before the drain completed");
        assert_eq!(code, CLOSE_GOING_AWAY);
        assert_eq!(server.transport.active_connection_count(), 0);
    }

    #[tokio::test]
    async fn test_forced_drain_before_callback_drops_the_upgraded_socket() {
        use tokio::io::AsyncReadExt;
        let server = start_server(Duration::from_millis(300), Duration::from_millis(50)).await;
        let mut client = upgrade(server.addr).await;

        server.signal.trigger();
        let outcome = server.serve.await.unwrap().unwrap();

        assert_matches!(outcome, ShutdownOutcome::Forced { .. }, "{outcome:?}");
        let mut rest = Vec::new();
        tokio::time::timeout(Duration::from_secs(2), client.read_to_end(&mut rest))
            .await
            .expect("the socket reaches EOF")
            .ok();
        assert!(
            rest.is_empty(),
            "no WebSocket task may have started: {rest:?}"
        );
        assert_eq!(server.transport.active_connection_count(), 0);
    }
}
