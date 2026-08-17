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
        }
    }

    /// Handle WebSocket upgrade for Axum.
    ///
    /// Extracts the peer address via [`ConnectInfo`] and rejects upgrade
    /// requests that exceed the per-IP request budget with HTTP 429 before any
    /// WebSocket frames are exchanged.
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
        State(transport): State<Arc<Self>>,
    ) -> Response {
        let client_ip = addr.ip();

        if let Err(e) = transport.rate_limiter.check_request(client_ip) {
            warn!("WebSocket upgrade denied for IP {}: {}", client_ip, e);
            return (StatusCode::TOO_MANY_REQUESTS, e.to_string()).into_response();
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
        let connection_id_clone = connection_id.clone();
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
                                            if let Err(e) = transport_clone.handle_websocket_message(connection_id_clone.clone(), ws_message).await {
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

    /// Handle WebSocket message for a specific connection
    async fn handle_websocket_message(
        &self,
        connection_id: String,
        message: WsMessage,
    ) -> PjsResult<()> {
        debug!(
            "Handling WebSocket message for connection {}: {:?}",
            connection_id, message
        );

        match message {
            WsMessage::FrameAck {
                session_id,
                frame_id,
                processing_time_ms,
            } => {
                self.controller
                    .handle_frame_ack(&session_id, frame_id, processing_time_ms)
                    .await?;
            }
            WsMessage::StreamInit {
                session_id: _,
                data,
                options,
            } => {
                let session_id = self.controller.create_session(data, options).await?;
                self.controller.start_streaming(&session_id).await?;
                self.connection_sessions
                    .write()
                    .await
                    .entry(connection_id.clone())
                    .or_default()
                    .push(session_id);
                info!(
                    "Created new streaming session for connection {}",
                    connection_id
                );
            }
            WsMessage::Ping { timestamp: _ } => {
                // Pong is handled automatically by the WebSocket implementation
                debug!("Received ping from connection {}", connection_id);
            }
            _ => {
                warn!("Unhandled message type from connection {}", connection_id);
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

    fn handle_message(
        &self,
        _connection: Arc<Self::Connection>,
        message: WsMessage,
    ) -> Self::HandleMessageFuture<'_> {
        async move {
            match message {
                WsMessage::StreamInit { data, options, .. } => {
                    info!("Initializing new stream");
                    let session_id = self.controller.create_session(data, options).await?;
                    self.controller.start_streaming(&session_id).await?;
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
                    warn!(
                        "Received error from client: session={:?}, error={}, code={}",
                        session_id, error, code
                    );
                }
                _ => {
                    warn!("Unhandled message type: {:?}", message);
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
