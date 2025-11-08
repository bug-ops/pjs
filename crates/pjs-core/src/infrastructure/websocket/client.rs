//! WebSocket client implementation for PJS streaming

#[cfg(feature = "websocket-client")]
use super::{StreamOptions, WsMessage};
use crate::{Error as PjsError, Result as PjsResult};
use futures::{SinkExt, StreamExt};
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{RwLock, mpsc};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, error, info, warn};
use url::Url;

/// WebSocket client for receiving PJS streams
pub struct PjsWebSocketClient {
    url: Url,
    sessions: Arc<RwLock<HashMap<String, StreamSession>>>,
    message_tx: mpsc::UnboundedSender<WsMessage>,
    message_rx: Arc<RwLock<Option<mpsc::UnboundedReceiver<WsMessage>>>>,
}

/// Client-side stream session
#[derive(Debug)]
struct StreamSession {
    id: String,
    created_at: Instant,
    received_frames: HashMap<u32, ReceivedFrame>,
    reconstructed_data: Value,
    is_complete: bool,
}

/// Frame received by client
#[derive(Debug, Clone)]
struct ReceivedFrame {
    frame_id: u32,
    priority: u8,
    payload: Value,
    received_at: Instant,
    processed_at: Option<Instant>,
}

impl PjsWebSocketClient {
    /// Create new WebSocket client
    pub fn new(url: impl AsRef<str>) -> PjsResult<Self> {
        let url = Url::parse(url.as_ref()).map_err(|e| PjsError::InvalidUrl(e.to_string()))?;

        let (message_tx, message_rx) = mpsc::unbounded_channel();

        Ok(Self {
            url,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            message_tx,
            message_rx: Arc::new(RwLock::new(Some(message_rx))),
        })
    }

    /// Connect to WebSocket server and start message handling
    pub async fn connect(&self) -> PjsResult<()> {
        info!("Connecting to WebSocket server: {}", self.url);

        let (ws_stream, _) = connect_async(self.url.as_str())
            .await
            .map_err(|e| PjsError::ConnectionFailed(e.to_string()))?;

        info!("WebSocket connection established");

        let (mut write, mut read) = ws_stream.split();

        // Take the receiver (can only be done once)
        let mut message_rx = self
            .message_rx
            .write()
            .await
            .take()
            .ok_or_else(|| PjsError::ClientError("Client already connected".to_string()))?;

        // Spawn task to send outgoing messages
        let send_task = tokio::spawn(async move {
            while let Some(message) = message_rx.recv().await {
                match serde_json::to_string(&message) {
                    Ok(json_str) => {
                        if let Err(e) = write.send(Message::Text(json_str.into())).await {
                            error!("Failed to send message: {}", e);
                            break;
                        }
                    }
                    Err(e) => {
                        error!("Failed to serialize message: {}", e);
                    }
                }
            }
        });

        // Handle incoming messages
        let sessions = self.sessions.clone();
        let message_tx = self.message_tx.clone();
        let receive_task = tokio::spawn(async move {
            while let Some(msg) = read.next().await {
                match msg {
                    Ok(Message::Text(text)) => match serde_json::from_str::<WsMessage>(&text) {
                        Ok(ws_message) => {
                            if let Err(e) = Self::handle_incoming_message(
                                sessions.clone(),
                                message_tx.clone(),
                                ws_message,
                            )
                            .await
                            {
                                error!("Failed to handle incoming message: {}", e);
                            }
                        }
                        Err(e) => {
                            warn!("Failed to parse incoming message: {}", e);
                        }
                    },
                    Ok(Message::Binary(data)) => {
                        debug!("Received binary data: {} bytes", data.len());
                    }
                    Ok(Message::Ping(_data)) => {
                        debug!("Received ping, sending pong");
                        // Pong is handled automatically by tungstenite
                    }
                    Ok(Message::Pong(_)) => {
                        debug!("Received pong");
                    }
                    Ok(Message::Close(_)) => {
                        info!("Server closed connection");
                        break;
                    }
                    Ok(Message::Frame(_)) => {
                        // Raw frame - usually handled internally by tungstenite
                        debug!("Received raw frame");
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

        info!("WebSocket connection closed");
        Ok(())
    }

    /// Request stream initialization
    pub async fn request_stream(
        &self,
        data: Value,
        options: Option<StreamOptions>,
    ) -> PjsResult<String> {
        let session_id = uuid::Uuid::new_v4().to_string();
        let options = options.unwrap_or_default();

        let message = WsMessage::StreamInit {
            session_id: session_id.clone(),
            data,
            options,
        };

        self.message_tx
            .send(message)
            .map_err(|e| PjsError::ClientError(format!("Failed to send stream request: {e}")))?;

        // Initialize session tracking
        let session = StreamSession {
            id: session_id.clone(),
            created_at: Instant::now(),
            received_frames: HashMap::new(),
            reconstructed_data: serde_json::json!({}),
            is_complete: false,
        };

        self.sessions
            .write()
            .await
            .insert(session_id.clone(), session);

        info!("Requested stream initialization: {}", session_id);
        Ok(session_id)
    }

    /// Get current reconstructed data for session
    pub async fn get_current_data(&self, session_id: &str) -> PjsResult<Option<Value>> {
        let sessions = self.sessions.read().await;
        Ok(sessions
            .get(session_id)
            .map(|session| session.reconstructed_data.clone()))
    }

    /// Check if stream is complete
    pub async fn is_stream_complete(&self, session_id: &str) -> bool {
        let sessions = self.sessions.read().await;
        sessions
            .get(session_id)
            .map(|session| session.is_complete)
            .unwrap_or(false)
    }

    /// Get stream statistics
    pub async fn get_stream_stats(&self, session_id: &str) -> Option<StreamStats> {
        let sessions = self.sessions.read().await;
        sessions.get(session_id).map(|session| {
            let total_frames = session.received_frames.len();
            let processed_frames = session
                .received_frames
                .values()
                .filter(|frame| frame.processed_at.is_some())
                .count();

            let avg_processing_time = if processed_frames > 0 {
                let total_time: Duration = session
                    .received_frames
                    .values()
                    .filter_map(|frame| {
                        frame
                            .processed_at
                            .map(|processed| processed.duration_since(frame.received_at))
                    })
                    .sum();
                Some(total_time / processed_frames as u32)
            } else {
                None
            };

            StreamStats {
                session_id: session.id.clone(),
                total_frames,
                processed_frames,
                is_complete: session.is_complete,
                duration: session.created_at.elapsed(),
                average_processing_time: avg_processing_time,
            }
        })
    }

    async fn handle_incoming_message(
        sessions: Arc<RwLock<HashMap<String, StreamSession>>>,
        message_tx: mpsc::UnboundedSender<WsMessage>,
        message: WsMessage,
    ) -> PjsResult<()> {
        match message {
            WsMessage::StreamFrame {
                session_id,
                frame_id,
                priority,
                payload,
                is_complete,
            } => {
                debug!("Received frame {} for session {}", frame_id, session_id);

                let processing_start = Instant::now();

                {
                    let mut sessions = sessions.write().await;
                    if let Some(session) = sessions.get_mut(&session_id) {
                        // Store received frame
                        let frame = ReceivedFrame {
                            frame_id,
                            priority,
                            payload: payload.clone(),
                            received_at: processing_start,
                            processed_at: None,
                        };
                        session.received_frames.insert(frame_id, frame);

                        // Apply frame to reconstructed data
                        Self::apply_frame_to_data(&mut session.reconstructed_data, &payload)?;

                        if is_complete {
                            session.is_complete = true;
                            info!("Stream completed for session {}", session_id);
                        }

                        // Mark as processed
                        if let Some(frame) = session.received_frames.get_mut(&frame_id) {
                            frame.processed_at = Some(Instant::now());
                        }
                    }
                }

                let processing_time = processing_start.elapsed();

                // Send acknowledgment
                let ack_message = WsMessage::FrameAck {
                    session_id,
                    frame_id,
                    processing_time_ms: processing_time.as_millis() as u64,
                };

                if let Err(e) = message_tx.send(ack_message) {
                    warn!("Failed to send frame acknowledgment: {}", e);
                }
            }
            WsMessage::StreamComplete {
                session_id,
                checksum,
            } => {
                info!("Stream completed: {} (checksum: {})", session_id, checksum);

                let mut sessions = sessions.write().await;
                if let Some(session) = sessions.get_mut(&session_id) {
                    session.is_complete = true;
                }
            }
            WsMessage::Error {
                session_id,
                error,
                code,
            } => {
                error!(
                    "Received error from server: session={:?}, error={}, code={}",
                    session_id, error, code
                );
            }
            WsMessage::Ping { timestamp } => {
                debug!("Received ping with timestamp: {}", timestamp);
                let pong = WsMessage::Pong { timestamp };
                if let Err(e) = message_tx.send(pong) {
                    warn!("Failed to send pong: {}", e);
                }
            }
            WsMessage::Pong { timestamp } => {
                debug!("Received pong with timestamp: {}", timestamp);
            }
            _ => {
                warn!("Unhandled message type: {:?}", message);
            }
        }
        Ok(())
    }

    fn apply_frame_to_data(data: &mut Value, payload: &Value) -> PjsResult<()> {
        // Simple merge strategy - in production, this would be more sophisticated
        match (data.as_object_mut(), payload.as_object()) {
            (Some(data_map), Some(payload_map)) => {
                for (key, value) in payload_map {
                    data_map.insert(key.clone(), value.clone());
                }
            }
            _ => {
                *data = payload.clone();
            }
        }
        Ok(())
    }
}

/// Stream statistics
#[derive(Debug, Clone)]
pub struct StreamStats {
    pub session_id: String,
    pub total_frames: usize,
    pub processed_frames: usize,
    pub is_complete: bool,
    pub duration: Duration,
    pub average_processing_time: Option<Duration>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_client_creation() {
        let client = PjsWebSocketClient::new("ws://localhost:3001/ws").unwrap();
        assert_eq!(client.url.as_str(), "ws://localhost:3001/ws");
    }

    #[tokio::test]
    async fn test_stream_session() {
        let client = PjsWebSocketClient::new("ws://localhost:3001/ws").unwrap();
        let data = json!({"test": "data"});

        let session_id = client.request_stream(data, None).await.unwrap();
        assert!(!session_id.is_empty());

        let sessions = client.sessions.read().await;
        assert!(sessions.contains_key(&session_id));
    }

    #[test]
    fn test_apply_frame_to_data() {
        let mut data = json!({"existing": "value"});
        let payload = json!({"new": "data", "existing": "updated"});

        PjsWebSocketClient::apply_frame_to_data(&mut data, &payload).unwrap();

        assert_eq!(data["existing"], "updated");
        assert_eq!(data["new"], "data");
    }
}
