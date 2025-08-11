//! WebSocket server implementation for Axum

#[cfg(feature = "http-server")]
use super::{AdaptiveStreamController, StreamOptions, WebSocketTransport, WsMessage};
use crate::{Error as PjsError, Result as PjsResult};
use async_trait::async_trait;
#[cfg(feature = "http-server")]
use axum::{
    extract::{
        ws::{Message, WebSocket},
        State, WebSocketUpgrade,
    },
    response::Response,
};
use futures::{SinkExt, StreamExt};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// Axum WebSocket transport implementation
pub struct AxumWebSocketTransport {
    controller: Arc<AdaptiveStreamController>,
    connections: Arc<RwLock<Vec<Arc<WebSocket>>>>,
}

impl AxumWebSocketTransport {
    pub fn new() -> Self {
        Self {
            controller: Arc::new(AdaptiveStreamController::new()),
            connections: Arc::new(RwLock::new(Vec::new())),
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
        
        let socket = Arc::new(socket);
        self.connections.write().await.push(socket.clone());
        
        let mut frame_rx = self.controller.subscribe_frames();
        
        let (mut sender, mut receiver) = {
            let socket_ref = Arc::try_unwrap(socket.clone())
                .unwrap_or_else(|arc| (*arc).clone())
                .split();
            socket_ref
        };

        // Spawn task to send frames to client
        let transport_clone = self.clone();
        let send_task = tokio::spawn(async move {
            while let Ok((session_id, message)) = frame_rx.recv().await {
                match serde_json::to_string(&message) {
                    Ok(json_str) => {
                        if let Err(e) = sender.send(Message::Text(json_str)).await {
                            error!("Failed to send message to client: {}", e);
                            break;
                        }
                    }
                    Err(e) => {
                        error!("Failed to serialize message: {}", e);
                    }
                }
            }
        });

        // Handle incoming messages from client
        let transport_clone = self.clone();
        let receive_task = tokio::spawn(async move {
            while let Some(msg) = receiver.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        match serde_json::from_str::<WsMessage>(&text) {
                            Ok(ws_message) => {
                                if let Err(e) = transport_clone.handle_message(socket.clone(), ws_message).await {
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
                        // TODO: Handle binary messages if needed
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
        });

        // Wait for either task to complete
        tokio::select! {
            _ = send_task => {
                debug!("Send task completed");
            }
            _ = receive_task => {
                debug!("Receive task completed");
            }
        }

        // Clean up connection
        let mut connections = self.connections.write().await;
        connections.retain(|conn| !Arc::ptr_eq(conn, &socket));
        info!("WebSocket connection closed");
    }

    pub fn controller(&self) -> Arc<AdaptiveStreamController> {
        self.controller.clone()
    }
}

impl Default for AxumWebSocketTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl WebSocketTransport for AxumWebSocketTransport {
    type Connection = WebSocket;

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
        connection: Arc<Self::Connection>,
        message: WsMessage,
    ) -> PjsResult<()> {
        let json_str = serde_json::to_string(&message)
            .map_err(|e| PjsError::Serialization(e.to_string()))?;
        
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
            WsMessage::FrameAck { session_id, frame_id, processing_time_ms } => {
                debug!("Received frame ack: session={}, frame={}, time={}ms", 
                       session_id, frame_id, processing_time_ms);
                self.controller.handle_frame_ack(&session_id, frame_id, processing_time_ms).await?;
            }
            WsMessage::Ping { timestamp } => {
                debug!("Received ping with timestamp: {}", timestamp);
                // Pong is handled automatically in handle_socket
            }
            WsMessage::Error { session_id, error, code } => {
                warn!("Received error from client: session={:?}, error={}, code={}", 
                      session_id, error, code);
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
    
    axum::Router::new()
        .route("/ws", get(AxumWebSocketTransport::upgrade_handler))
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
        
        let session_id = transport.controller.create_session(data, StreamOptions::default()).await.unwrap();
        assert!(!session_id.is_empty());
        
        // Test starting stream
        transport.controller.start_streaming(&session_id).await.unwrap();
    }
}