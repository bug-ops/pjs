//! WebSocket server implementation for Axum

#[cfg(feature = "http-server")]
use super::{AdaptiveStreamController, StreamOptions, WebSocketTransport, WsMessage};
use crate::{
    Error as PjsError, Result as PjsResult,
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
use futures::{SinkExt, StreamExt};
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, error, info, warn};
use uuid;

/// Axum WebSocket transport implementation
pub struct AxumWebSocketTransport {
    controller: Arc<AdaptiveStreamController>,
    /// Active connection IDs for tracking open sockets
    active_connections: Arc<RwLock<Vec<String>>>,
    /// Per-connection outgoing senders; keyed by connection ID
    outgoing_channels: Arc<RwLock<HashMap<String, UnboundedSender<WsMessage>>>>,
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
    pub fn with_rate_limit_config(config: RateLimitConfig) -> Self {
        Self {
            controller: Arc::new(AdaptiveStreamController::new()),
            active_connections: Arc::new(RwLock::new(Vec::new())),
            outgoing_channels: Arc::new(RwLock::new(HashMap::new())),
            rate_limiter: Arc::new(WebSocketRateLimiter::new(config)),
        }
    }

    /// Handle WebSocket upgrade for Axum.
    ///
    /// Extracts the peer address via [`ConnectInfo`] and rejects upgrade
    /// requests that exceed the per-IP request budget with HTTP 429 before any
    /// WebSocket frames are exchanged.
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

        ws.on_upgrade(move |socket| transport.handle_socket(socket, client_ip))
    }

    /// Handle WebSocket connection lifecycle
    pub async fn handle_socket(self: Arc<Self>, socket: WebSocket, client_ip: IpAddr) {
        info!("New WebSocket connection established from {}", client_ip);

        let guard = match RateLimitGuard::new(self.rate_limiter.clone(), client_ip) {
            Ok(g) => Arc::new(g),
            Err(e) => {
                warn!(
                    "WebSocket connection rejected for IP {} (rate limit): {}",
                    client_ip, e
                );
                let (mut sender, _) = socket.split();
                let _ = sender
                    .send(Message::Close(Some(axum::extract::ws::CloseFrame {
                        code: 1008, // Policy Violation
                        reason: e.to_string().into(),
                    })))
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
        let (outgoing_tx, mut outgoing_rx) = tokio::sync::mpsc::unbounded_channel::<WsMessage>();
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
                                            if let Err(e) = sender.send(Message::Text(json_str.into())).await {
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
                        // Handle outgoing messages from application
                        Some(message) = outgoing_rx.recv() => {
                            match serde_json::to_string(&message) {
                                Ok(json_str) => {
                                    if let Err(e) = sender.send(Message::Text(json_str.into())).await {
                                        error!("Failed to send message to client: {}", e);
                                        break;
                                    }
                                }
                                Err(e) => {
                                    error!("Failed to serialize outgoing message: {}", e);
                                }
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
                                        let _ = sender
                                            .send(Message::Close(Some(axum::extract::ws::CloseFrame {
                                                code: 1008,
                                                reason: e.to_string().into(),
                                            })))
                                            .await;
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
                                        let _ = sender
                                            .send(Message::Close(Some(axum::extract::ws::CloseFrame {
                                                code: 1008,
                                                reason: e.to_string().into(),
                                            })))
                                            .await;
                                        break;
                                    }
                                    debug!("Received binary data: {} bytes", data.len());
                                }
                                Ok(Message::Ping(data)) => {
                                    if let Err(e) = sender.send(Message::Pong(data)).await {
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
        drop(guard);
        info!("WebSocket connection closed for {}", client_ip);
    }

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

    fn send_frame(
        &self,
        connection: Arc<Self::Connection>,
        message: WsMessage,
    ) -> Self::SendFrameFuture<'_> {
        async move {
            let channels = self.outgoing_channels.read().await;
            if let Some(tx) = channels.get(connection.as_ref()) {
                tx.send(message)
                    .map_err(|e| PjsError::Other(format!("Failed to queue frame: {}", e)))?;
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
}
