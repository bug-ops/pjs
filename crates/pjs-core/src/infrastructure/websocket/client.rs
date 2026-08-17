//! WebSocket client implementation for PJS streaming

#[cfg(feature = "websocket-client")]
use super::{StreamOptions, WsMessage};
use crate::{
    Error as PjsError, Result as PjsResult,
    infrastructure::bounded_channel::{ByteBoundedSender, Envelope, byte_bounded_channel},
};
use futures::StreamExt;
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

/// Capacity of the client's outgoing message channel.
///
/// Bounds how many outgoing messages (stream requests, acks, pongs) can
/// queue while `send_task` catches up. This is a message-count bound only;
/// [`MAX_QUEUED_MESSAGE_BYTES`] additionally bounds cumulative queued
/// bytes.
const MESSAGE_QUEUE_CAPACITY: usize = 1000;

/// Cumulative byte budget for the client's outgoing message channel, on
/// top of [`MESSAGE_QUEUE_CAPACITY`]'s message-count bound.
///
/// Keeps worst-case queued memory a small, predictable constant regardless
/// of individual message size (e.g. a large `StreamInit` payload).
const MAX_QUEUED_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

/// WebSocket client for receiving PJS streams
pub struct PjsWebSocketClient {
    url: Url,
    sessions: Arc<RwLock<HashMap<String, ClientStreamSession>>>,
    message_tx: ByteBoundedSender<String>,
    message_rx: Arc<RwLock<Option<mpsc::Receiver<Envelope<String>>>>>,
    write_timeout: Duration,
}

/// Client-side stream session
#[derive(Debug)]
struct ClientStreamSession {
    id: String,
    created_at: Instant,
    received_frames: HashMap<u32, ReceivedFrame>,
    reconstructed_data: Value,
    is_complete: bool,
}

/// Frame received by client
#[derive(Debug, Clone)]
struct ReceivedFrame {
    received_at: Instant,
    processed_at: Option<Instant>,
}

impl PjsWebSocketClient {
    /// Create new WebSocket client
    pub fn new(url: impl AsRef<str>) -> PjsResult<Self> {
        let url = Url::parse(url.as_ref()).map_err(|e| PjsError::InvalidUrl(e.to_string()))?;

        let (message_tx, message_rx) =
            byte_bounded_channel(MESSAGE_QUEUE_CAPACITY, MAX_QUEUED_MESSAGE_BYTES);

        Ok(Self {
            url,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            message_tx,
            message_rx: Arc::new(RwLock::new(Some(message_rx))),
            write_timeout: super::WRITE_TIMEOUT,
        })
    }

    /// Overrides the deadline for a single outbound WebSocket sink write,
    /// used by the `send_task` spawned in [`Self::connect`].
    ///
    /// Defaults to `infrastructure::websocket::WRITE_TIMEOUT` (10s) — see
    /// its doc for the rationale and the tradeoff it implies for large
    /// frames sent to slow clients. Pair a shorter value with a
    /// resource-constrained deployment where freeing a wedged send task
    /// quickly matters more than absorbing network jitter (mirroring
    /// `RateLimitConfig::low_resource`'s tightened `write_timeout` on the
    /// server side); pair a longer value with a deployment that expects
    /// large payloads over slow or high-latency uplinks and would
    /// otherwise see legitimate writes misclassified as stalled.
    ///
    /// `write_timeout` is not validated: [`Duration::ZERO`] leaves at most
    /// one poll of the underlying write before it is treated as a timeout,
    /// and an arbitrarily large value (including [`Duration::MAX`]) is
    /// accepted as-is and does not panic, since `tokio::time::timeout`
    /// clamps internally.
    ///
    /// # Examples
    ///
    /// ```
    /// use pjson_rs::infrastructure::websocket::PjsWebSocketClient;
    /// use std::time::Duration;
    ///
    /// let client = PjsWebSocketClient::new("ws://localhost:3001/ws")
    ///     .unwrap()
    ///     .with_write_timeout(Duration::from_secs(3));
    /// ```
    #[must_use]
    pub fn with_write_timeout(mut self, write_timeout: Duration) -> Self {
        self.write_timeout = write_timeout;
        self
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

        // Spawn task to send outgoing messages. Messages are already
        // serialized at the point they were queued (see `request_stream`
        // and `handle_incoming_message`), so the byte-budget accounting
        // there matches the bytes actually held in memory here. `split`
        // (rather than `into_inner`) keeps the byte budget charged until
        // the write actually completes, not just until the item leaves
        // the channel.
        let write_timeout = self.write_timeout;
        let send_task = tokio::spawn(async move {
            while let Some(envelope) = message_rx.recv().await {
                let (json_str, _budget_permit) = envelope.split();
                if let Err(e) = super::send_with_write_timeout(
                    &mut write,
                    Message::Text(json_str.into()),
                    write_timeout,
                )
                .await
                {
                    error!("Failed to send message: {}", e);
                    break;
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
        let json_str = serde_json::to_string(&message).map_err(|e| {
            PjsError::ClientError(format!("Failed to serialize stream request: {e}"))
        })?;
        let len = json_str.len();

        self.message_tx.send(json_str, len).await.map_err(|_| {
            PjsError::ClientError(
                "Failed to send stream request: outgoing channel closed".to_string(),
            )
        })?;

        // Initialize session tracking
        let session = ClientStreamSession {
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

    /// Best-effort control-message send: serializes `message` and
    /// `try_send`s it, logging (rather than propagating) both serialization
    /// and send failures. Used for acks/pongs sent from
    /// `handle_incoming_message`, which runs inline inside the read loop —
    /// awaiting a full channel there would stall draining the socket, so
    /// dropping is preferred over blocking (see call sites for the fuller
    /// rationale).
    fn try_send_control_message(
        message_tx: &ByteBoundedSender<String>,
        message: &WsMessage,
        kind: &str,
    ) {
        match serde_json::to_string(message) {
            Ok(json_str) => {
                let len = json_str.len();
                if let Err(e) = message_tx.try_send(json_str, len) {
                    warn!(
                        "Dropping {} (channel full, byte budget exceeded, or closed): {:?}",
                        kind, e
                    );
                }
            }
            Err(e) => warn!("Failed to serialize {}: {}", kind, e),
        }
    }

    async fn handle_incoming_message(
        sessions: Arc<RwLock<HashMap<String, ClientStreamSession>>>,
        message_tx: ByteBoundedSender<String>,
        message: WsMessage,
    ) -> PjsResult<()> {
        match message {
            WsMessage::StreamFrame {
                session_id,
                frame_id,
                priority: _priority,
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

                // Best-effort: dropping an ack is recoverable (the server
                // can re-send or time out the frame); stalling the whole
                // read loop is not. See `try_send_control_message`'s doc.
                Self::try_send_control_message(&message_tx, &ack_message, "frame acknowledgment");
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
                // Same rationale as the ack above: best-effort, must not
                // stall the read loop by awaiting a full channel.
                Self::try_send_control_message(&message_tx, &pong, "pong");
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
    /// Identifier of the streaming session.
    pub session_id: String,
    /// Total number of frames received from the server.
    pub total_frames: usize,
    /// Number of frames the client has finished processing.
    pub processed_frames: usize,
    /// Whether the stream has been marked complete.
    pub is_complete: bool,
    /// Wall-clock duration since the session was created.
    pub duration: Duration,
    /// Average per-frame processing duration, if any frames have been processed.
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
        assert_eq!(client.write_timeout, super::super::WRITE_TIMEOUT);
    }

    #[tokio::test]
    async fn test_with_write_timeout_overrides_default() {
        let client = PjsWebSocketClient::new("ws://localhost:3001/ws")
            .unwrap()
            .with_write_timeout(Duration::from_secs(3));
        assert_eq!(client.write_timeout, Duration::from_secs(3));
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

    #[tokio::test]
    async fn test_message_channel_is_bounded() {
        // Regression test for #314: the client's outgoing message channel
        // used to be unbounded, so a stalled `send_task` let it grow
        // without limit. It must now reject sends once
        // `MESSAGE_QUEUE_CAPACITY` is reached.
        let client = PjsWebSocketClient::new("ws://localhost:3001/ws").unwrap();
        let tx = client.message_tx.clone();
        let pong = "pong".to_string();

        for _ in 0..MESSAGE_QUEUE_CAPACITY {
            tx.try_send(pong.clone(), pong.len())
                .expect("channel should accept sends up to its capacity");
        }

        let result = tx.try_send(pong.clone(), pong.len());
        assert!(
            matches!(
                result,
                Err(
                    crate::infrastructure::bounded_channel::TrySendError::Channel(
                        mpsc::error::TrySendError::Full(_)
                    )
                )
            ),
            "channel must reject sends past capacity instead of growing unbounded"
        );
    }

    #[tokio::test]
    async fn test_message_channel_rejects_when_byte_budget_exceeded() {
        // Regression test for #349: a message-count bound alone doesn't
        // bound queued bytes. A single message larger than
        // `MAX_QUEUED_MESSAGE_BYTES` must be rejected even though the
        // channel is nowhere near its message-count capacity.
        let client = PjsWebSocketClient::new("ws://localhost:3001/ws").unwrap();
        let tx = client.message_tx.clone();
        let oversized = "x".repeat(MAX_QUEUED_MESSAGE_BYTES + 1);
        let len = oversized.len();

        assert!(
            matches!(
                tx.try_send(oversized, len),
                Err(crate::infrastructure::bounded_channel::TrySendError::BudgetExceeded(_))
            ),
            "a single over-budget message must be rejected"
        );
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
