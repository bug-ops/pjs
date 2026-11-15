//! WebSocket server implementation for Axum

use super::{
    AdaptiveStreamController, ConnectionId, CreateSessionError, WriteFailure, WsMessage,
    controller::{connection_plan_budget_for, spawn_task},
    planner::MAX_QUEUED_OUTGOING_BYTES,
    protocol::{CODE_CONFLICT, FixedError},
    send_with_write_timeout,
};
use crate::{
    Error as PjsError, Result as PjsResult,
    domain::SessionId,
    error::{ErrorChain, PlanError, SessionIdViolation},
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
    FutureExt, StreamExt,
    stream::{SplitSink, SplitStream},
};
use std::net::{IpAddr, SocketAddr};
use std::num::NonZeroUsize;
use std::ops::ControlFlow;
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

/// `Error` reply code for a request the server cannot parse or accept.
const CODE_BAD_REQUEST: u16 = 400;

/// Close code (RFC 6455) for a connection that violated the server's policy.
const CLOSE_POLICY_VIOLATION: u16 = 1008;

/// Close code (RFC 6455) sent to every open connection on server shutdown.
const CLOSE_GOING_AWAY: u16 = 1001;

/// Fixed reply text for undecodable input. Never built from the decode error:
/// that text echoes attacker-chosen content back to the peer.
const MALFORMED_MESSAGE: &str = "malformed message";

/// Fixed reply text for a `StreamInit` carrying the nil UUID.
const NIL_SESSION_ID: &str = "nil session id is not allowed";

/// `Error` reply code for a `StreamInit` over the per-connection session cap.
const CODE_TOO_MANY_SESSIONS: u16 = 429;

/// Fixed reply text for a `StreamInit` over the per-connection session cap.
const TOO_MANY_SESSIONS: &str = "too many live sessions on this connection";

/// Fixed reply text for a `StreamInit` whose stream options fail validation.
const INVALID_STREAM_OPTIONS: &str = "invalid stream options";

/// Fixed reply text for a `StreamInit` whose id belongs to an existing session.
const DUPLICATE_SESSION_ID: &str = "session id already in use";

/// Reply for a rejected plan: the connection budget is transient (429), a
/// document over the limits is permanent for that document (413), and a plan
/// that cannot be serialized is a server fault (500).
fn plan_rejection_reply(error: PlanError) -> FixedError {
    match error {
        PlanError::ConnectionBudgetExhausted { .. } => FixedError::PLAN_BUDGET_EXHAUSTED,
        PlanError::UnitExceedsBudget { .. } => FixedError::FRAME_EXCEEDS_BUDGET,
        PlanError::TooManyFrames { .. }
        | PlanError::PlanBytesExceeded { .. }
        | PlanError::PathTooDeep { .. } => FixedError::PLAN_TOO_LARGE,
        PlanError::Unserializable => FixedError::PLAN_UNSERIALIZABLE,
    }
}

/// Builds an `Error` reply with a fixed, non-echoing message.
fn error_reply(session_id: Option<SessionId>, code: u16, error: &'static str) -> WsMessage {
    WsMessage::Error {
        session_id,
        error: error.to_owned(),
        code,
    }
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
const SESSION_CLEANUP_INTERVAL: Duration = Duration::from_secs(60);

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
                    let pass = controller.cleanup_expired_sessions(SESSION_MAX_AGE);
                    if AssertUnwindSafe(pass).catch_unwind().await.is_err() {
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
            warn!("WebSocket upgrade denied for IP {}: {}", client_ip, e);
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
                "WebSocket upgrade rejected for IP {}: disallowed Origin {:?}",
                client_ip, origin
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
        info!("New WebSocket connection established from {}", client_ip);

        let write_timeout = self.rate_limiter.config().write_timeout;

        let guard = match RateLimitGuard::new(self.rate_limiter.clone(), client_ip) {
            Ok(guard) => guard,
            Err(e) => {
                warn!(
                    "WebSocket connection rejected for IP {} (rate limit): {}",
                    client_ip, e
                );
                let (mut sender, _) = socket.split();
                close_policy_violation(&mut sender, e.to_string(), write_timeout).await;
                return;
            }
        };

        let connection_id = ConnectionId::new();
        let (outgoing_tx, outgoing_rx) =
            byte_bounded_channel::<String>(OUTGOING_QUEUE_CAPACITY, MAX_QUEUED_OUTGOING_BYTES);
        self.controller
            .attach_connection(connection_id, outgoing_tx);
        let _cleanup = ConnectionCleanup {
            transport: Arc::clone(&self),
            connection_id,
        };

        let (sender, receiver) = socket.split();
        ConnectionLoop {
            transport: &self,
            connection_id,
            client_ip,
            write_timeout,
            guard: &guard,
            sender,
        }
        .run(outgoing_rx, receiver)
        .await;

        info!("WebSocket connection closed for {}", client_ip);
    }

    /// Returns a shared handle to the underlying [`AdaptiveStreamController`].
    pub fn controller(&self) -> Arc<AdaptiveStreamController> {
        self.controller.clone()
    }

    /// Returns the number of currently active WebSocket connections.
    ///
    /// Useful for observability, health endpoints, and integration tests.
    pub async fn active_connection_count(&self) -> usize {
        self.controller.connection_count()
    }

    /// Queues a fixed-message `Error` reply for `connection`.
    ///
    /// Goes through [`Self::send_frame`], which never blocks, so it is safe to
    /// call from the connection's own message-handling path.
    async fn reply_error(
        &self,
        connection: ConnectionId,
        session_id: Option<SessionId>,
        code: u16,
        error: &'static str,
    ) {
        let reply = error_reply(session_id, code, error);
        if let Err(e) = self.send_frame(connection, reply).await {
            warn!(
                "Failed to queue error reply for connection {}: {}",
                connection,
                ErrorChain(&e)
            );
        }
    }

    /// Queues `message` for `connection`'s outgoing channel without blocking.
    ///
    /// The channel is drained by the same `tokio::select!` loop that also
    /// awaits [`Self::handle_message`] inline. Calling this from within that
    /// inline handling path (directly or transitively) must therefore never
    /// block: a blocking send would wait forever on a receiver that cannot
    /// run until the in-flight branch finishes. Using `try_send` keeps that
    /// hazard from becoming a real deadlock.
    ///
    /// Always returns `Ok(())` even when the frame is dropped (channel
    /// full, or larger than `MAX_QUEUED_OUTGOING_BYTES`) — this mirrors
    /// the underlying channel's own fire-and-forget delivery guarantee. Both
    /// drop reasons are logged via `warn!`.
    pub async fn send_frame(&self, connection: ConnectionId, message: WsMessage) -> PjsResult<()> {
        let tx = self.controller.outgoing_for(connection);
        let Some(tx) = tx else {
            warn!("send_frame: no outgoing channel for connection {connection}");
            return Ok(());
        };
        // Serialized once here, rather than in the consuming loop: this is
        // also what the byte-budget check in `try_send` measures, so the
        // queued-bytes accounting matches the actual bytes held in memory.
        match serde_json::to_string(&message) {
            Ok(json_str) => {
                let len = json_str.len();
                match tx.try_send(json_str, len) {
                    Ok(()) => {}
                    Err(bounded_channel::TrySendError::BudgetExceeded(_)) => {
                        warn!(
                            "send_frame: dropping frame for connection {connection} (byte budget exceeded, {len} bytes)"
                        );
                    }
                    Err(bounded_channel::TrySendError::Channel(_)) => {
                        warn!(
                            "send_frame: dropping frame for connection {connection} (channel full or closed)"
                        );
                    }
                }
            }
            Err(e) => {
                warn!("send_frame: failed to serialize frame for connection {connection}: {e}");
            }
        }
        Ok(())
    }

    /// Handles one decoded message from `connection`.
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
        match message {
            WsMessage::StreamInit {
                session_id,
                data,
                options,
            } => {
                if session_id.as_uuid().is_nil() {
                    self.reply_error(connection, None, CODE_BAD_REQUEST, NIL_SESSION_ID)
                        .await;
                    return Err(PjsError::InvalidSessionId(SessionIdViolation::Nil));
                }
                match self
                    .controller
                    .create_session(session_id, connection, data, options)
                    .await
                {
                    Ok(()) => {}
                    Err(e @ CreateSessionError::AlreadyExists(_)) => {
                        self.reply_error(
                            connection,
                            Some(session_id),
                            CODE_CONFLICT,
                            DUPLICATE_SESSION_ID,
                        )
                        .await;
                        return Err(e.into());
                    }
                    Err(e @ CreateSessionError::TooManySessions { .. }) => {
                        self.reply_error(
                            connection,
                            Some(session_id),
                            CODE_TOO_MANY_SESSIONS,
                            TOO_MANY_SESSIONS,
                        )
                        .await;
                        return Err(e.into());
                    }
                    Err(e @ CreateSessionError::InvalidOptions(_)) => {
                        self.reply_error(
                            connection,
                            Some(session_id),
                            CODE_BAD_REQUEST,
                            INVALID_STREAM_OPTIONS,
                        )
                        .await;
                        return Err(e.into());
                    }
                    Err(e @ CreateSessionError::PlanRejected(plan_error)) => {
                        let reply = plan_rejection_reply(plan_error);
                        self.reply_error(connection, Some(session_id), reply.code, reply.reason)
                            .await;
                        return Err(e.into());
                    }
                }
                self.controller
                    .start_streaming(connection, session_id)
                    .await?;
                info!("Created new streaming session for connection {connection}");
            }
            WsMessage::FrameAck {
                session_id,
                frame_id,
                processing_time_ms,
            } => {
                debug!(
                    "Received frame ack: session={:?}, frame={}, time={}ms",
                    session_id, frame_id, processing_time_ms
                );
                if let Err(e) = self
                    .controller
                    .handle_frame_ack(connection, session_id, frame_id, processing_time_ms)
                    .await
                {
                    debug!(
                        "Ignored frame ack for connection {connection}: {}",
                        ErrorChain(&e)
                    );
                }
            }
            WsMessage::StreamCancel { session_id } => {
                if !self.controller.remove_session(connection, session_id).await {
                    debug!(
                        "Ignored cancel of unknown session {session_id} on connection {connection}"
                    );
                }
            }
            WsMessage::Ping { timestamp } => {
                debug!("Received ping with timestamp: {}", timestamp);
                self.send_frame(connection, WsMessage::Pong { timestamp })
                    .await?;
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
                    "Received error from client on connection {}: session_id={:?}, code={}, error_len={}",
                    connection,
                    session_id,
                    code,
                    error.len()
                );
            }
            // Server-to-client messages: a client sending them gets no reply
            // (no amplification); a new variant must be classified here.
            WsMessage::StreamFrame { .. }
            | WsMessage::StreamPatch { .. }
            | WsMessage::StreamComplete { .. }
            | WsMessage::Pong { .. } => {
                debug!(
                    "Ignored server-bound message {} from connection {connection}",
                    message.kind()
                );
            }
        }
        Ok(())
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
        debug!("Failed to send Close frame {code}: {e}");
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
    client_ip: IpAddr,
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
        let message = Message::Text(json_str.into());
        match send_with_write_timeout(&mut self.sender, message, self.write_timeout).await {
            Ok(()) => ControlFlow::Continue(()),
            Err(e) => {
                error!("Failed to send {what} to client: {e}");
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
                warn!(
                    "Inbound {kind} frame rejected for IP {} (rate limit): {}",
                    self.client_ip, e
                );
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
                            "Inbound {} message from connection {}, {} bytes",
                            ws_message.kind(),
                            self.connection_id,
                            text.len()
                        );
                        if let Err(e) = self
                            .transport
                            .handle_message(self.connection_id, ws_message)
                            .await
                        {
                            debug!(
                                "Rejected message from connection {}: {}",
                                self.connection_id,
                                ErrorChain(&e)
                            );
                        }
                        ControlFlow::Continue(())
                    }
                    Err(e) => {
                        warn!(
                            "Failed to parse WebSocket message: {:?} at line {} column {}",
                            e.classify(),
                            e.line(),
                            e.column()
                        );
                        let reply = error_reply(None, CODE_BAD_REQUEST, MALFORMED_MESSAGE);
                        match send_reply(&mut self.sender, &reply, self.write_timeout).await {
                            Ok(()) => ControlFlow::Continue(()),
                            Err(e) => {
                                error!("Failed to send error reply to client: {}", e);
                                ControlFlow::Break(())
                            }
                        }
                    }
                }
            }
            Ok(Message::Binary(data)) => {
                if !self.admit(data.len(), "binary").await {
                    return ControlFlow::Break(());
                }
                debug!("Received binary data: {} bytes", data.len());
                ControlFlow::Continue(())
            }
            Ok(Message::Ping(data)) => {
                let pong = Message::Pong(data);
                match send_with_write_timeout(&mut self.sender, pong, self.write_timeout).await {
                    Ok(()) => ControlFlow::Continue(()),
                    Err(e) => {
                        error!("Failed to send pong: {}", e);
                        ControlFlow::Break(())
                    }
                }
            }
            Ok(Message::Pong(_)) => {
                debug!("Received pong from client");
                ControlFlow::Continue(())
            }
            Ok(Message::Close(_)) => {
                info!("Client closed WebSocket connection");
                ControlFlow::Break(())
            }
            Err(e) => {
                error!("WebSocket error: {}", e);
                ControlFlow::Break(())
            }
        }
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
    send_with_write_timeout(sender, Message::Text(json.into()), write_timeout).await?;
    Ok(())
}

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
            .await
            .unwrap();
    }

    fn stream_init(session_id: SessionId) -> WsMessage {
        WsMessage::StreamInit {
            session_id,
            data: json!({"test": "value"}),
            options: StreamOptions::default(),
        }
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
        assert_eq!(reply_code(rx.try_recv().unwrap()), CODE_CONFLICT);
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
        assert_eq!(reply_code(rx.try_recv().unwrap()), CODE_BAD_REQUEST);
        assert_eq!(transport.controller.session_count(connection), 0);
    }

    #[tokio::test]
    async fn test_handle_message_invalid_options_are_rejected_with_400_and_no_session() {
        let transport = AxumWebSocketTransport::new();
        let connection = ConnectionId::new();
        let mut rx = register_outgoing(&transport, connection);
        let init = WsMessage::StreamInit {
            session_id: SessionId::new(),
            data: json!({}),
            options: StreamOptions {
                max_frame_size: 100,
                ..StreamOptions::default()
            },
        };

        let result = transport.handle_message(connection, init).await;

        assert!(result.is_err());
        let reply = serde_json::from_str::<WsMessage>(&rx.try_recv().unwrap().into_inner());
        assert!(
            matches!(&reply, Ok(WsMessage::Error { code: 400, error, .. }) if error == INVALID_STREAM_OPTIONS),
            "{reply:?}"
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
        assert_eq!(reply_code(rx.try_recv().unwrap()), CODE_TOO_MANY_SESSIONS);
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
        assert!(
            matches!(reply, Ok(WsMessage::Pong { timestamp: 42 })),
            "{reply:?}"
        );
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
            .cleanup_expired_sessions(Duration::ZERO)
            .await;
        while rx.try_recv().is_ok() {}

        transport
            .handle_message(connection, stream_init(session_id))
            .await
            .unwrap();

        let envelope = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("the new session must stream")
            .unwrap();
        assert!(matches!(
            serde_json::from_str::<WsMessage>(&envelope.into_inner()),
            Ok(WsMessage::StreamFrame { session_id: id, .. }) if id == session_id
        ));
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
        assert!(
            matches!(
                result,
                Err(bounded_channel::TrySendError::Channel(
                    tokio::sync::mpsc::error::TrySendError::Full(_)
                ))
            ),
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

        transport
            .send_frame(connection, oversized_message)
            .await
            .expect("send_frame returns Ok even when it drops the frame");

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
            transport
                .send_frame(connection, WsMessage::Ping { timestamp: 0 })
                .await
                .expect("send_frame should accept sends up to channel capacity");
        }

        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            transport.send_frame(connection, WsMessage::Ping { timestamp: 0 }),
        )
        .await
        .expect("send_frame must not block when the outgoing channel is full")
        .expect("send_frame must return Ok even when dropping the overflow frame");

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
        assert_eq!(server.transport.active_connection_count().await, 0);
    }

    #[tokio::test]
    async fn test_forced_drain_before_callback_drops_the_upgraded_socket() {
        use tokio::io::AsyncReadExt;
        let server = start_server(Duration::from_millis(300), Duration::from_millis(50)).await;
        let mut client = upgrade(server.addr).await;

        server.signal.trigger();
        let outcome = server.serve.await.unwrap().unwrap();

        assert!(
            matches!(outcome, ShutdownOutcome::Forced { .. }),
            "{outcome:?}"
        );
        let mut rest = Vec::new();
        tokio::time::timeout(Duration::from_secs(2), client.read_to_end(&mut rest))
            .await
            .expect("the socket reaches EOF")
            .ok();
        assert!(
            rest.is_empty(),
            "no WebSocket task may have started: {rest:?}"
        );
        assert_eq!(server.transport.active_connection_count().await, 0);
    }
}
