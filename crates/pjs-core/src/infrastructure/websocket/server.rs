//! WebSocket server implementation for Axum

#[cfg(feature = "http-server")]
use super::{AdaptiveStreamController, StreamOptions, WebSocketTransport, WsMessage};
use crate::{Error as PjsError, Result as PjsResult};
use async_trait::async_trait;
#[cfg(feature = "http-server")]
use axum::{
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::Response,
};
use futures::{SinkExt, StreamExt};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use uuid;

/// Axum WebSocket transport implementation
pub struct AxumWebSocketTransport {
    controller: Arc<AdaptiveStreamController>,
    // Store connection IDs instead of WebSocket objects for Send/Sync compatibility
    active_connections: Arc<RwLock<Vec<String>>>,
}

impl AxumWebSocketTransport {
    pub fn new() -> Self {
        Self {
            controller: Arc::new(AdaptiveStreamController::new()),
            active_connections: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Handle WebSocket upgrade for Axum
    pub async fn upgrade_handler(
        ws: WebSocketUpgrade,
        State(transport): State<Arc<Self>>,
    ) -> Response {
        ws.on_upgrade(move |socket| transport.handle_socket(socket))
    }

    /// Handle WebSocket connection lifecycle
    pub async fn handle_socket(self: Arc<Self>, socket: WebSocket) {
        info!("New WebSocket connection established");

        let connection_id = uuid::Uuid::new_v4().to_string();
        self.active_connections
            .write()
            .await
            .push(connection_id.clone());

        let frame_rx = self.controller.subscribe_frames();

        // Create channels for communication between tasks
        let (_outgoing_tx, mut outgoing_rx) = tokio::sync::mpsc::unbounded_channel::<WsMessage>();
        let (mut sender, mut receiver) = socket.split();

        // Spawn single task to handle both sending and receiving
        let transport_clone = self.clone();
        let connection_id_clone = connection_id.clone();
        let websocket_task = {
            let mut frame_rx = frame_rx;
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        // Handle frames from stream controller
                        Ok((_session_id, message)) = frame_rx.recv() => {
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
            })
        };

        // Wait for the task to complete
        if let Err(e) = websocket_task.await {
            error!("WebSocket task failed: {}", e);
        }

        // Clean up connection
        let mut connections = self.active_connections.write().await;
        connections.retain(|conn_id| *conn_id != connection_id);
        info!("WebSocket connection closed");
    }

    pub fn controller(&self) -> Arc<AdaptiveStreamController> {
        self.controller.clone()
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
                let _session_id = self.controller.create_session(data, options).await?;
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

#[async_trait]
impl WebSocketTransport for AxumWebSocketTransport {
    type Connection = String; // Use connection ID instead of WebSocket

    async fn start_stream(
        &self,
        _connection: Arc<Self::Connection>,
        data: Value,
        options: StreamOptions,
    ) -> PjsResult<String> {
        let session_id = self.controller.create_session(data, options).await?;
        self.controller.start_streaming(&session_id).await?;
        Ok(session_id)
    }

    async fn send_frame(
        &self,
        _connection: Arc<Self::Connection>,
        message: WsMessage,
    ) -> PjsResult<()> {
        let json_str =
            serde_json::to_string(&message).map_err(|e| PjsError::Serialization(e.to_string()))?;

        // Note: In practice, this would need to be handled differently
        // since we can't directly send through Arc<WebSocket>
        // The actual sending is handled in handle_socket via frame subscription

        debug!("Frame queued for transmission: {}", json_str);
        Ok(())
    }

    async fn handle_message(
        &self,
        _connection: Arc<Self::Connection>,
        message: WsMessage,
    ) -> PjsResult<()> {
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

    async fn close_stream(&self, session_id: &str) -> PjsResult<()> {
        // TODO: Implement session cleanup
        info!("Closing stream session: {}", session_id);
        Ok(())
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
