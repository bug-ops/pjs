//! WebSocket server implementation for Axum

#[cfg(feature = "http-server")]
use super::{AdaptiveStreamController, StreamOptions, WebSocketTransport, WsMessage};
use crate::{
    Result as PjsResult,
    infrastructure::bounded_channel::{self, ByteBoundedSender, byte_bounded_channel},
    security::{RateLimitConfig, RateLimitGuard, WebSocketRateLimiter},
};
#[cfg(feature = "http-server")]
use axum::{
    extract::{
        ConnectInfo, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
};
use futures::StreamExt;
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::sync::broadcast::error::RecvError;
use tracing::{debug, error, info, warn};
use uuid;

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

/// Cumulative byte budget for a single connection's outgoing message
/// channel, on top of [`OUTGOING_QUEUE_CAPACITY`]'s message-count bound.
///
/// Without this, `OUTGOING_QUEUE_CAPACITY` alone bounds queue depth but
/// not queued bytes: at the default 16 MiB `max_websocket_frame_size`, a
/// fully-queued connection could hold up to `1000 * 16 MiB` ≈ 16 GiB.
/// 16 MiB keeps worst-case per-connection queued memory a small,
/// predictable constant regardless of individual message size.
const MAX_QUEUED_OUTGOING_BYTES: usize = 16 * 1024 * 1024;

/// How often the background sweep spawned by
/// [`AxumWebSocketTransport::with_rate_limit_config`] checks for streaming
/// sessions older than [`SESSION_MAX_AGE`].
const SESSION_CLEANUP_INTERVAL: Duration = Duration::from_secs(60);

/// Maximum age of a controller-tracked streaming session before the
/// background sweep removes it (aborting its streaming task) even if the
/// owning connection's teardown never ran, e.g. a broadcast-lagged frame
/// receiver or a session created but never associated with a live
/// connection.
const SESSION_MAX_AGE: Duration = Duration::from_secs(3600);

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
    /// Active connection IDs for tracking open sockets
    active_connections: Arc<RwLock<Vec<String>>>,
    /// Per-connection outgoing senders; keyed by connection ID
    outgoing_channels: Arc<RwLock<HashMap<String, ByteBoundedSender<String>>>>,
    /// Streaming session IDs created by each connection (via `StreamInit`),
    /// so [`Self::handle_socket`]'s teardown can abort the right sessions'
    /// streaming tasks when the connection closes.
    connection_sessions: Arc<RwLock<HashMap<String, Vec<String>>>>,
    /// Per-IP rate limiter applied to upgrade requests, connection establishment,
    /// and inbound application-level messages.
    rate_limiter: Arc<WebSocketRateLimiter>,
    /// `Origin` allow-list applied to WebSocket upgrades, to block
    /// cross-site WebSocket hijacking (CSWSH) from browser clients. See
    /// [`Self::with_allowed_origins`].
    allowed_origins: OriginAllowList,
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
    /// Spawns a background sweep that periodically aborts streaming
    /// sessions older than `SESSION_MAX_AGE` via
    /// [`AdaptiveStreamController::cleanup_expired_sessions`]; the sweep
    /// holds only a [`std::sync::Weak`] reference to the controller, so it
    /// exits once every `Arc<AdaptiveStreamController>` (including this
    /// transport's own) is dropped, instead of keeping the controller alive
    /// forever.
    pub fn with_rate_limit_config(config: RateLimitConfig) -> Self {
        let controller = Arc::new(AdaptiveStreamController::new());

        let weak_controller = Arc::downgrade(&controller);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(SESSION_CLEANUP_INTERVAL);
            loop {
                interval.tick().await;
                let Some(controller) = weak_controller.upgrade() else {
                    break;
                };
                controller.cleanup_expired_sessions(SESSION_MAX_AGE).await;
            }
        });

        Self {
            controller,
            active_connections: Arc::new(RwLock::new(Vec::new())),
            outgoing_channels: Arc::new(RwLock::new(HashMap::new())),
            connection_sessions: Arc::new(RwLock::new(HashMap::new())),
            rate_limiter: Arc::new(WebSocketRateLimiter::new(config)),
            allowed_origins: OriginAllowList::DenyAll,
        }
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

        ws.on_upgrade(move |socket| transport.handle_socket(socket, client_ip))
    }

    /// Handle WebSocket connection lifecycle
    pub async fn handle_socket(self: Arc<Self>, socket: WebSocket, client_ip: IpAddr) {
        info!("New WebSocket connection established from {}", client_ip);

        let write_timeout = self.rate_limiter.config().write_timeout;

        let guard = match RateLimitGuard::new(self.rate_limiter.clone(), client_ip) {
            Ok(g) => Arc::new(g),
            Err(e) => {
                warn!(
                    "WebSocket connection rejected for IP {} (rate limit): {}",
                    client_ip, e
                );
                let (mut sender, _) = socket.split();
                let _ = super::send_with_write_timeout(
                    &mut sender,
                    Message::Close(Some(axum::extract::ws::CloseFrame {
                        code: 1008, // Policy Violation
                        reason: e.to_string().into(),
                    })),
                    write_timeout,
                )
                .await;
                return;
            }
        };

        let connection_id = uuid::Uuid::new_v4().to_string();
        self.active_connections
            .write()
            .await
            .push(connection_id.clone());

        let frame_rx = self.controller.subscribe_frames();

        // Create channel for sending outgoing messages to this connection
        let (outgoing_tx, mut outgoing_rx) =
            byte_bounded_channel::<String>(OUTGOING_QUEUE_CAPACITY, MAX_QUEUED_OUTGOING_BYTES);
        self.outgoing_channels
            .write()
            .await
            .insert(connection_id.clone(), outgoing_tx);

        let (mut sender, mut receiver) = socket.split();

        // Spawn single task to handle both sending and receiving
        let transport_clone = self.clone();
        let connection_id_clone = Arc::new(connection_id.clone());
        let guard_for_task = guard.clone();
        let websocket_task = {
            let mut frame_rx = frame_rx;
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        // Handle frames from stream controller. Match on the full
                        // Result so Lagged is logged-and-skipped while Closed
                        // ends the loop instead of busy-spinning.
                        recv_result = frame_rx.recv() => {
                            match recv_result {
                                Ok((_session_id, message)) => {
                                    match serde_json::to_string(&message) {
                                        Ok(json_str) => {
                                            if let Err(e) = super::send_with_write_timeout(&mut sender, Message::Text(json_str.into()), write_timeout).await {
                                                error!("Failed to send message to client: {}", e);
                                                break;
                                            }
                                        }
                                        Err(e) => {
                                            error!("Failed to serialize message: {}", e);
                                        }
                                    }
                                }
                                Err(RecvError::Lagged(skipped)) => {
                                    warn!("Frame broadcast lagged; skipped {} frames", skipped);
                                }
                                Err(RecvError::Closed) => {
                                    debug!("Frame broadcast channel closed");
                                    break;
                                }
                            }
                        }
                        // Handle outgoing messages from application. Already
                        // serialized at `send_frame` time — see its doc for why.
                        // `split` (rather than `into_inner`) keeps the byte
                        // budget charged until the write actually completes,
                        // not just until the item leaves the channel.
                        Some(envelope) = outgoing_rx.recv() => {
                            let (json_str, _budget_permit) = envelope.split();
                            if let Err(e) = super::send_with_write_timeout(&mut sender, Message::Text(json_str.into()), write_timeout).await {
                                error!("Failed to send outgoing message to client: {}", e);
                                break;
                            }
                        }
                        // Handle incoming messages from client
                        Some(msg) = receiver.next() => {
                            match msg {
                                Ok(Message::Text(text)) => {
                                    if let Err(e) = guard_for_task.check_message(text.len()) {
                                        warn!(
                                            "Inbound text frame rejected for IP {} (rate limit): {}",
                                            client_ip, e
                                        );
                                        let _ = super::send_with_write_timeout(
                                            &mut sender,
                                            Message::Close(Some(axum::extract::ws::CloseFrame {
                                                code: 1008,
                                                reason: e.to_string().into(),
                                            })),
                                            write_timeout,
                                        ).await;
                                        break;
                                    }
                                    match serde_json::from_str::<WsMessage>(&text) {
                                        Ok(ws_message) => {
                                            if let Err(e) = transport_clone.handle_websocket_message(Arc::clone(&connection_id_clone), ws_message).await {
                                                error!("Failed to handle message: {}", e);
                                            }
                                        }
                                        Err(e) => {
                                            warn!("Failed to parse WebSocket message: {}", e);
                                        }
                                    }
                                }
                                Ok(Message::Binary(data)) => {
                                    if let Err(e) = guard_for_task.check_message(data.len()) {
                                        warn!(
                                            "Inbound binary frame rejected for IP {} (rate limit): {}",
                                            client_ip, e
                                        );
                                        let _ = super::send_with_write_timeout(
                                            &mut sender,
                                            Message::Close(Some(axum::extract::ws::CloseFrame {
                                                code: 1008,
                                                reason: e.to_string().into(),
                                            })),
                                            write_timeout,
                                        ).await;
                                        break;
                                    }
                                    debug!("Received binary data: {} bytes", data.len());
                                }
                                Ok(Message::Ping(data)) => {
                                    if let Err(e) = super::send_with_write_timeout(&mut sender, Message::Pong(data), write_timeout).await {
                                        error!("Failed to send pong: {}", e);
                                        break;
                                    }
                                }
                                Ok(Message::Pong(_)) => {
                                    debug!("Received pong from client");
                                }
                                Ok(Message::Close(_)) => {
                                    info!("Client closed WebSocket connection");
                                    break;
                                }
                                Err(e) => {
                                    error!("WebSocket error: {}", e);
                                    break;
                                }
                            }
                        }
                        else => {
                            break;
                        }
                    }
                }
                drop(guard_for_task);
            })
        };

        // Wait for the task to complete
        if let Err(e) = websocket_task.await {
            error!("WebSocket task failed: {}", e);
        }

        // Clean up outgoing channel and connection record. The rate-limit
        // guard's connection counter is decremented when the last Arc<Guard>
        // is dropped (here and when the spawned task ends).
        self.outgoing_channels.write().await.remove(&connection_id);
        let mut connections = self.active_connections.write().await;
        connections.retain(|conn_id| *conn_id != connection_id);
        drop(connections);
        drop(guard);

        // Abort every streaming task this connection started — otherwise a
        // session's frame-streaming task keeps running (and its abort
        // handle stays unreachable) after the client that requested it has
        // disconnected.
        if let Some(session_ids) = self
            .connection_sessions
            .write()
            .await
            .remove(&connection_id)
        {
            for session_id in session_ids {
                self.controller.remove_session(&session_id).await;
            }
        }

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
        self.active_connections.read().await.len()
    }

    /// Handle WebSocket message for a specific connection.
    ///
    /// Thin wrapper around [`WebSocketTransport::handle_message`] that
    /// forwards the axum socket loop's already-shared `connection_id`; all
    /// message handling, including `connection_sessions` tracking for
    /// `StreamInit`, lives in `handle_message` itself. `connection_sessions`
    /// entries are drained only by [`Self::handle_socket`]'s teardown,
    /// keyed by the same connection id — nothing else in this type removes
    /// them, so a caller that drives [`WebSocketTransport::handle_message`]
    /// directly, bypassing `handle_socket`, leaves its sessions in that map
    /// until the connection id happens to be reused or the process exits.
    async fn handle_websocket_message(
        &self,
        connection_id: Arc<String>,
        message: WsMessage,
    ) -> PjsResult<()> {
        debug!(
            "Handling WebSocket message for connection {}: {:?}",
            connection_id, message
        );
        self.handle_message(connection_id, message).await
    }
}

impl Default for AxumWebSocketTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl WebSocketTransport for AxumWebSocketTransport {
    type Connection = String; // Use connection ID instead of WebSocket

    type StartStreamFuture<'a>
        = impl Future<Output = PjsResult<String>> + Send + 'a
    where
        Self: 'a;

    type SendFrameFuture<'a>
        = impl Future<Output = PjsResult<()>> + Send + 'a
    where
        Self: 'a;

    type HandleMessageFuture<'a>
        = impl Future<Output = PjsResult<()>> + Send + 'a
    where
        Self: 'a;

    type CloseStreamFuture<'a>
        = impl Future<Output = PjsResult<()>> + Send + 'a
    where
        Self: 'a;

    fn start_stream(
        &self,
        _connection: Arc<Self::Connection>,
        data: Value,
        options: StreamOptions,
    ) -> Self::StartStreamFuture<'_> {
        async move {
            let session_id = self.controller.create_session(data, options).await?;
            self.controller.start_streaming(&session_id).await?;
            Ok(session_id)
        }
    }

    /// The channel this queues onto is drained by the same `tokio::select!`
    /// loop in [`Self::handle_socket`] that also awaits
    /// `handle_websocket_message` inline. Calling `send_frame` from
    /// within that inline handling path (directly or transitively) would
    /// deadlock the connection: the loop can't reach `outgoing_rx.recv()`
    /// again until the in-flight branch finishes, so a blocking send would
    /// wait forever on a receiver that can't run. Using `try_send` here
    /// keeps that latent hazard from becoming a real deadlock — see
    /// `WebSocketTransport::send_frame`'s doc for the general contract.
    ///
    /// Always returns `Ok(())` even when the frame is dropped (channel
    /// full, or larger than `MAX_QUEUED_OUTGOING_BYTES`) — this mirrors
    /// the underlying channel's own fire-and-forget delivery guarantee
    /// (an `Ok` `try_send` on a normal `mpsc` channel doesn't promise the
    /// receiver will ever read the item either) and matches how the
    /// broadcast-based `frame_rx` delivery path also has no per-frame
    /// delivery acknowledgment. Both drop reasons are logged via `warn!`.
    fn send_frame(
        &self,
        connection: Arc<Self::Connection>,
        message: WsMessage,
    ) -> Self::SendFrameFuture<'_> {
        async move {
            // Clone the sender and release the read lock before sending:
            // a stalled consumer must not hold up other connections
            // waiting on `outgoing_channels` (e.g. cleanup taking the write lock).
            let tx = self
                .outgoing_channels
                .read()
                .await
                .get(connection.as_ref())
                .cloned();
            if let Some(tx) = tx {
                // Serialized once here, rather than in the consuming loop:
                // this is also what the byte-budget check in `try_send`
                // measures, so the queued-bytes accounting matches the
                // actual bytes held in memory.
                match serde_json::to_string(&message) {
                    Ok(json_str) => {
                        let len = json_str.len();
                        match tx.try_send(json_str, len) {
                            Ok(()) => {}
                            Err(bounded_channel::TrySendError::BudgetExceeded(_)) => {
                                warn!(
                                    "send_frame: dropping frame for connection {} (byte budget exceeded, {} bytes)",
                                    connection.as_ref(),
                                    len
                                );
                            }
                            Err(bounded_channel::TrySendError::Channel(_)) => {
                                warn!(
                                    "send_frame: dropping frame for connection {} (channel full or closed)",
                                    connection.as_ref()
                                );
                            }
                        }
                    }
                    Err(e) => {
                        warn!(
                            "send_frame: failed to serialize frame for connection {}: {}",
                            connection.as_ref(),
                            e
                        );
                    }
                }
            } else {
                warn!(
                    "send_frame: no outgoing channel for connection {}",
                    connection.as_ref()
                );
            }
            Ok(())
        }
    }

    /// The `StreamInit` arm records the created session under `connection`
    /// in `connection_sessions` so [`Self::handle_socket`]'s teardown can
    /// abort it on disconnect. Nothing else drains that map: a caller that
    /// drives this method directly, bypassing `handle_socket` (e.g. a test,
    /// or a future non-axum trait caller), leaves its session's entry there
    /// indefinitely — [`WebSocketTransport::close_stream`] removes the
    /// session from the controller but does not touch `connection_sessions`.
    fn handle_message(
        &self,
        connection: Arc<Self::Connection>,
        message: WsMessage,
    ) -> Self::HandleMessageFuture<'_> {
        async move {
            match message {
                WsMessage::StreamInit { data, options, .. } => {
                    let session_id = self.controller.create_session(data, options).await?;
                    // Tracked before `start_streaming` so a session that was
                    // successfully created is still reachable for cleanup
                    // even if `start_streaming` itself returns an error.
                    self.connection_sessions
                        .write()
                        .await
                        .entry((*connection).clone())
                        .or_default()
                        .push(session_id.clone());
                    self.controller.start_streaming(&session_id).await?;
                    info!(
                        "Created new streaming session for connection {}",
                        connection.as_ref()
                    );
                }
                WsMessage::FrameAck {
                    session_id,
                    frame_id,
                    processing_time_ms,
                } => {
                    debug!(
                        "Received frame ack: session={}, frame={}, time={}ms",
                        session_id, frame_id, processing_time_ms
                    );
                    self.controller
                        .handle_frame_ack(&session_id, frame_id, processing_time_ms)
                        .await?;
                }
                WsMessage::Ping { timestamp } => {
                    debug!("Received ping with timestamp: {}", timestamp);
                    // Pong is handled automatically in handle_socket
                }
                WsMessage::Error {
                    session_id,
                    error,
                    code,
                } => {
                    // `session_id` and `error` are both arbitrary
                    // client-supplied strings with no length validation on
                    // this path — log only their lengths (plus the
                    // connection id for correlation), never the values
                    // themselves, at WARN (see #415 S1: unbounded WARN
                    // amplification from a single rate-limited connection).
                    warn!(
                        "Received error from client on connection {}: session_id_len={:?}, code={}, error_len={}",
                        connection.as_ref(),
                        session_id.as_deref().map(str::len),
                        code,
                        error.len()
                    );
                }
                _ => {
                    warn!(
                        "Unhandled message type from connection {}",
                        connection.as_ref()
                    );
                }
            }
            Ok(())
        }
    }

    fn close_stream(&self, session_id: &str) -> Self::CloseStreamFuture<'_> {
        let session_id = session_id.to_string();
        async move {
            info!("Closing stream session: {}", session_id);
            self.controller.remove_session(&session_id).await;
            Ok(())
        }
    }
}

/// Helper function to create WebSocket router for Axum
pub fn create_websocket_router() -> axum::Router<Arc<AxumWebSocketTransport>> {
    use axum::routing::get;

    axum::Router::new().route("/ws", get(AxumWebSocketTransport::upgrade_handler))
}

#[cfg(test)]
mod tests {
    use super::*;
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

        let session_id = transport
            .controller
            .create_session(data, StreamOptions::default())
            .await
            .unwrap();
        assert!(!session_id.is_empty());

        // Test starting stream
        transport
            .controller
            .start_streaming(&session_id)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_handle_message_stream_init_tracks_connection_session() {
        // Regression test for #415: `WebSocketTransport::handle_message`'s
        // `StreamInit` arm used to create a session without recording it in
        // `connection_sessions`, so a caller driving the transport through
        // the trait directly (bypassing `handle_socket`) got a session that
        // was never associated with its connection for cleanup. Assert the
        // association exists after calling `handle_message` directly.
        let transport = AxumWebSocketTransport::new();
        let connection = Arc::new("conn-x".to_string());

        transport
            .handle_message(
                connection.clone(),
                WsMessage::StreamInit {
                    session_id: "ignored-client-supplied-id".to_string(),
                    data: json!({"test": "value"}),
                    options: StreamOptions::default(),
                },
            )
            .await
            .unwrap();

        let sessions = transport.connection_sessions.read().await;
        let tracked = sessions
            .get(connection.as_ref())
            .expect("connection_sessions must have an entry for this connection");
        assert_eq!(tracked.len(), 1);
        assert!(!tracked[0].is_empty());
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
        let connection_id = "test-connection".to_string();
        let (tx, mut rx) =
            byte_bounded_channel::<String>(OUTGOING_QUEUE_CAPACITY, MAX_QUEUED_OUTGOING_BYTES);
        transport
            .outgoing_channels
            .write()
            .await
            .insert(connection_id.clone(), tx);

        let connection = Arc::new(connection_id);
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
        let connection_id = "test-connection".to_string();
        let (tx, mut rx) =
            byte_bounded_channel::<String>(OUTGOING_QUEUE_CAPACITY, MAX_QUEUED_OUTGOING_BYTES);
        transport
            .outgoing_channels
            .write()
            .await
            .insert(connection_id.clone(), tx);

        let connection = Arc::new(connection_id);
        for _ in 0..OUTGOING_QUEUE_CAPACITY {
            transport
                .send_frame(connection.clone(), WsMessage::Ping { timestamp: 0 })
                .await
                .expect("send_frame should accept sends up to channel capacity");
        }

        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            transport.send_frame(connection.clone(), WsMessage::Ping { timestamp: 0 }),
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
}
