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
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{Message, protocol::WebSocketConfig},
};
use tracing::{debug, error, info, warn};

use crate::security::rate_limit::DEFAULT_MAX_INBOUND_FRAME_SIZE;
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

/// Default cap on concurrently tracked client sessions.
const DEFAULT_MAX_SESSIONS: usize = 1024;

/// Default cap, in bytes, on the data reconstructed for a single session.
const DEFAULT_MAX_SESSION_BYTES: usize = 16 * 1024 * 1024;

/// Transport and state bounds applied to untrusted server input.
///
/// The message and frame caps default to the in-tree server's inbound cap
/// ([`RateLimitConfig::max_frame_size`](crate::security::RateLimitConfig::max_frame_size)),
/// because the server answers a `StreamInit` with the whole request data in a
/// single frame; a smaller client cap would sever the connection to it.
///
/// # Examples
///
/// ```
/// use pjson_rs::infrastructure::websocket::WebSocketClientConfig;
///
/// let config = WebSocketClientConfig {
///     max_message_size: 128 * 1024,
///     ..WebSocketClientConfig::default()
/// };
/// assert_eq!(config.max_frame_size, 1024 * 1024);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebSocketClientConfig {
    /// Maximum size in bytes of a complete incoming WebSocket message.
    pub max_message_size: usize,
    /// Maximum size in bytes of a single incoming WebSocket frame.
    pub max_frame_size: usize,
    /// Maximum number of sessions tracked at once.
    ///
    /// When the limit is reached, a new session first evicts completed
    /// sessions and, if none are completed, the oldest session. A request
    /// therefore never fails because of this limit, but an evicted session
    /// stops receiving frames.
    pub max_sessions: usize,
    /// Maximum size in bytes of the data reconstructed for one session.
    ///
    /// A frame that would exceed it is rejected and its session is dropped.
    pub max_session_bytes: usize,
}

impl Default for WebSocketClientConfig {
    fn default() -> Self {
        Self {
            max_message_size: DEFAULT_MAX_INBOUND_FRAME_SIZE,
            max_frame_size: DEFAULT_MAX_INBOUND_FRAME_SIZE,
            max_sessions: DEFAULT_MAX_SESSIONS,
            max_session_bytes: DEFAULT_MAX_SESSION_BYTES,
        }
    }
}

impl WebSocketClientConfig {
    fn websocket_config(&self) -> WebSocketConfig {
        WebSocketConfig::default()
            .max_message_size(Some(self.max_message_size))
            .max_frame_size(Some(self.max_frame_size))
    }
}

/// WebSocket client for receiving PJS streams
pub struct PjsWebSocketClient {
    url: Url,
    config: WebSocketClientConfig,
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
    frames_received: usize,
    total_processing_time: Duration,
    reconstructed_data: Value,
    reconstructed_bytes: usize,
    is_complete: bool,
}

impl ClientStreamSession {
    fn new(id: String) -> Self {
        Self {
            id,
            created_at: Instant::now(),
            frames_received: 0,
            total_processing_time: Duration::ZERO,
            reconstructed_data: serde_json::json!({}),
            reconstructed_bytes: json_size(&serde_json::json!({})),
            is_complete: false,
        }
    }

    fn record_frame(&mut self, processing_time: Duration) {
        self.frames_received = self.frames_received.saturating_add(1);
        self.total_processing_time = self.total_processing_time.saturating_add(processing_time);
    }

    /// Merges `payload` into the reconstructed data, refusing the frame
    /// (without mutating anything) if the result would exceed `max_bytes`.
    fn apply_frame(&mut self, payload: &Value, max_bytes: usize) -> PjsResult<()> {
        let projected = match (self.reconstructed_data.as_object(), payload.as_object()) {
            (Some(data_map), Some(payload_map)) => {
                let mut bytes = self.reconstructed_bytes;
                for (key, value) in payload_map {
                    bytes = bytes.saturating_add(entry_size(key, value));
                    if let Some(old) = data_map.get(key) {
                        bytes = bytes.saturating_sub(entry_size(key, old));
                    }
                }
                bytes
            }
            _ => json_size(payload),
        };
        if projected > max_bytes {
            return Err(PjsError::ClientError(format!(
                "Session {:?} exceeds the {max_bytes}-byte reconstructed data limit",
                self.id
            )));
        }

        match (self.reconstructed_data.as_object_mut(), payload.as_object()) {
            (Some(data_map), Some(payload_map)) => {
                for (key, value) in payload_map {
                    data_map.insert(key.clone(), value.clone());
                }
            }
            _ => self.reconstructed_data = payload.clone(),
        }
        self.reconstructed_bytes = projected;
        Ok(())
    }
}

/// Serialized size of `value`; saturates so a failure counts against the cap.
fn json_size(value: &Value) -> usize {
    serde_json::to_vec(value).map_or(usize::MAX, |bytes| bytes.len())
}

/// Upper bound on the serialized size of one object entry: quoted key, colon,
/// value and a separating comma.
fn entry_size(key: &str, value: &Value) -> usize {
    let quoted_key = serde_json::to_vec(key).map_or(usize::MAX, |bytes| bytes.len());
    quoted_key
        .saturating_add(2)
        .saturating_add(json_size(value))
}

/// Removes a session from the map when dropped unless disarmed, so a
/// cancelled `request_stream` cannot leak a slot.
struct SessionSlotGuard {
    sessions: Arc<RwLock<HashMap<String, ClientStreamSession>>>,
    id: String,
    armed: bool,
}

impl SessionSlotGuard {
    fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for SessionSlotGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        if let Ok(mut sessions) = self.sessions.try_write() {
            sessions.remove(&self.id);
        } else if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let sessions = Arc::clone(&self.sessions);
            let id = std::mem::take(&mut self.id);
            runtime.spawn(async move {
                sessions.write().await.remove(&id);
            });
        }
    }
}

/// Makes room for a new session: evicts completed sessions when full and,
/// if none are completed, the oldest session.
fn reserve_session_slot(sessions: &mut HashMap<String, ClientStreamSession>, max_sessions: usize) {
    if sessions.len() < max_sessions {
        return;
    }
    sessions.retain(|_, session| !session.is_complete);
    while sessions.len() >= max_sessions {
        let Some(oldest) = sessions
            .iter()
            .min_by_key(|(_, session)| session.created_at)
            .map(|(id, _)| id.clone())
        else {
            break;
        };
        sessions.remove(&oldest);
    }
}

impl PjsWebSocketClient {
    /// Create new WebSocket client
    pub fn new(url: impl AsRef<str>) -> PjsResult<Self> {
        let url = Url::parse(url.as_ref()).map_err(|e| PjsError::InvalidUrl(e.to_string()))?;

        let (message_tx, message_rx) =
            byte_bounded_channel(MESSAGE_QUEUE_CAPACITY, MAX_QUEUED_MESSAGE_BYTES);

        Ok(Self {
            url,
            config: WebSocketClientConfig::default(),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            message_tx,
            message_rx: Arc::new(RwLock::new(Some(message_rx))),
            write_timeout: super::WRITE_TIMEOUT,
        })
    }

    /// Overrides the transport and state bounds applied to server input.
    ///
    /// Defaults to [`WebSocketClientConfig::default`].
    ///
    /// # Examples
    ///
    /// ```
    /// use pjson_rs::infrastructure::websocket::{PjsWebSocketClient, WebSocketClientConfig};
    ///
    /// let client = PjsWebSocketClient::new("ws://localhost:3001/ws")
    ///     .unwrap()
    ///     .with_config(WebSocketClientConfig {
    ///         max_sessions: 16,
    ///         ..WebSocketClientConfig::default()
    ///     });
    /// ```
    #[must_use]
    pub fn with_config(mut self, config: WebSocketClientConfig) -> Self {
        self.config = config;
        self
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

    /// Connect to WebSocket server and start message handling.
    ///
    /// # Errors
    ///
    /// Returns an error if the handshake fails or the connection is aborted
    /// by a transport error, including a message or frame exceeding the
    /// configured [`WebSocketClientConfig`] caps.
    pub async fn connect(&self) -> PjsResult<()> {
        info!("Connecting to WebSocket server: {}", self.url);

        let (ws_stream, _) = connect_async_with_config(
            self.url.as_str(),
            Some(self.config.websocket_config()),
            false,
        )
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
        let config = self.config;
        let receive_task = tokio::spawn(async move {
            while let Some(msg) = read.next().await {
                match msg {
                    Ok(Message::Text(text)) => match serde_json::from_str::<WsMessage>(&text) {
                        Ok(ws_message) => {
                            if let Err(e) = Self::handle_incoming_message(
                                sessions.clone(),
                                message_tx.clone(),
                                config,
                                ws_message,
                            )
                            .await
                            {
                                error!("Failed to handle incoming message: {}", e);
                            }
                        }
                        Err(e) => {
                            warn!("Failed to parse incoming message: {:?}", e);
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
                        return Err(PjsError::ConnectionFailed(e.to_string()));
                    }
                }
            }
            Ok(())
        });

        // Wait for either task to complete
        let outcome = tokio::select! {
            _ = send_task => {
                debug!("Send task completed");
                Ok(())
            }
            result = receive_task => {
                debug!("Receive task completed");
                result.unwrap_or_else(|e| {
                    Err(PjsError::ClientError(format!("Receive task failed: {e}")))
                })
            }
        };

        info!("WebSocket connection closed");
        outcome
    }

    /// Request stream initialization and return the new session id.
    ///
    /// When the session limit is reached, an existing session is evicted
    /// (see [`WebSocketClientConfig::max_sessions`]).
    ///
    /// # Errors
    ///
    /// Returns an error if the request cannot be serialized or the outgoing
    /// channel is closed.
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

        {
            let mut sessions = self.sessions.write().await;
            reserve_session_slot(&mut sessions, self.config.max_sessions);
            sessions.insert(
                session_id.clone(),
                ClientStreamSession::new(session_id.clone()),
            );
        }
        let slot = SessionSlotGuard {
            sessions: Arc::clone(&self.sessions),
            id: session_id.clone(),
            armed: true,
        };

        self.message_tx.send(json_str, len).await.map_err(|_| {
            PjsError::ClientError(
                "Failed to send stream request: outgoing channel closed".to_string(),
            )
        })?;
        slot.disarm();

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
            let total_frames = session.frames_received;
            let avg_processing_time = u32::try_from(total_frames)
                .ok()
                .filter(|&n| n > 0)
                .map(|n| session.total_processing_time / n);

            StreamStats {
                session_id: session.id.clone(),
                total_frames,
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
        config: WebSocketClientConfig,
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
                debug!("Received frame {} for session {:?}", frame_id, session_id);

                let processing_start = Instant::now();

                {
                    let mut sessions = sessions.write().await;
                    if let Some(session) = sessions.get_mut(&session_id) {
                        if let Err(e) = session.apply_frame(&payload, config.max_session_bytes) {
                            sessions.remove(&session_id);
                            return Err(e);
                        }

                        if is_complete {
                            session.is_complete = true;
                            info!("Stream completed for session {:?}", session_id);
                        }

                        session.record_frame(processing_start.elapsed());
                    }
                }

                let processing_time = processing_start.elapsed();

                // Send acknowledgment
                let ack_message = WsMessage::FrameAck {
                    session_id,
                    frame_id,
                    processing_time_ms: u64::try_from(processing_time.as_millis())
                        .unwrap_or(u64::MAX),
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
                info!(
                    "Stream completed: {:?} (checksum: {:?})",
                    session_id, checksum
                );

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
                    "Received error from server: session={:?}, error={:?}, code={}",
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
}

/// Stream statistics
#[derive(Debug, Clone)]
pub struct StreamStats {
    /// Identifier of the streaming session.
    pub session_id: String,
    /// Number of frames successfully applied to the session.
    ///
    /// A repeated `frame_id` counts again, and frames rejected (for example
    /// for exceeding the session data cap) are not counted.
    pub total_frames: usize,
    /// Whether the stream has been marked complete.
    pub is_complete: bool,
    /// Wall-clock duration since the session was created.
    pub duration: Duration,
    /// Average per-frame processing duration, if any frames have been applied.
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
    fn test_client_config_default_matches_server_inbound_cap() {
        let config = WebSocketClientConfig::default();
        let server_cap = crate::security::RateLimitConfig::default().max_frame_size;
        assert_eq!(config.max_message_size, server_cap);
        assert_eq!(config.max_frame_size, server_cap);
    }

    #[test]
    fn test_client_config_maps_to_transport_limits() {
        let config = WebSocketClientConfig {
            max_message_size: 100,
            max_frame_size: 50,
            ..WebSocketClientConfig::default()
        };
        let ws = config.websocket_config();
        assert_eq!(ws.max_message_size, Some(100));
        assert_eq!(ws.max_frame_size, Some(50));
    }

    #[tokio::test]
    async fn test_with_config_overrides_default() {
        let config = WebSocketClientConfig {
            max_sessions: 3,
            ..WebSocketClientConfig::default()
        };
        let client = PjsWebSocketClient::new("ws://localhost:3001/ws")
            .unwrap()
            .with_config(config);
        assert_eq!(client.config, config);
    }

    fn session_aged(id: &str, age_secs: u64, is_complete: bool) -> ClientStreamSession {
        let mut session = ClientStreamSession::new(id.to_string());
        session.created_at = Instant::now() - Duration::from_secs(age_secs);
        session.is_complete = is_complete;
        session
    }

    #[test]
    fn test_reserve_session_slot_evicts_completed_before_live_sessions() {
        let mut sessions = HashMap::new();
        sessions.insert("done".to_string(), session_aged("done", 1, true));
        sessions.insert("old".to_string(), session_aged("old", 100, false));

        reserve_session_slot(&mut sessions, 2);

        assert!(!sessions.contains_key("done"));
        assert!(sessions.contains_key("old"));
    }

    #[test]
    fn test_reserve_session_slot_evicts_oldest_when_none_completed() {
        let mut sessions = HashMap::new();
        sessions.insert("oldest".to_string(), session_aged("oldest", 100, false));
        sessions.insert("newer".to_string(), session_aged("newer", 10, false));

        reserve_session_slot(&mut sessions, 2);

        assert!(!sessions.contains_key("oldest"));
        assert!(sessions.contains_key("newer"));
    }

    #[test]
    fn test_reserve_session_slot_is_noop_below_limit() {
        let mut sessions = HashMap::new();
        sessions.insert("a".to_string(), session_aged("a", 1, false));

        reserve_session_slot(&mut sessions, 2);

        assert_eq!(sessions.len(), 1);
    }

    #[tokio::test]
    async fn test_request_stream_at_capacity_evicts_oldest_instead_of_failing() {
        let client = PjsWebSocketClient::new("ws://localhost:3001/ws")
            .unwrap()
            .with_config(WebSocketClientConfig {
                max_sessions: 2,
                ..WebSocketClientConfig::default()
            });

        let first = client.request_stream(json!({}), None).await.unwrap();
        let second = client.request_stream(json!({}), None).await.unwrap();
        let third = client.request_stream(json!({}), None).await.unwrap();

        let sessions = client.sessions.read().await;
        assert_eq!(sessions.len(), 2);
        assert!(!sessions.contains_key(&first));
        assert!(sessions.contains_key(&second));
        assert!(sessions.contains_key(&third));
    }

    #[tokio::test]
    async fn test_request_stream_failed_send_does_not_leak_session() {
        let client = PjsWebSocketClient::new("ws://localhost:3001/ws").unwrap();
        drop(client.message_rx.write().await.take());

        assert!(client.request_stream(json!({}), None).await.is_err());
        assert!(client.sessions.read().await.is_empty());
    }

    #[tokio::test]
    async fn test_request_stream_cancelled_while_send_pending_does_not_leak_session() {
        let client = PjsWebSocketClient::new("ws://localhost:3001/ws").unwrap();
        let filler = "x".to_string();
        for _ in 0..MESSAGE_QUEUE_CAPACITY {
            client
                .message_tx
                .try_send(filler.clone(), filler.len())
                .expect("channel should accept sends up to its capacity");
        }

        let pending = tokio::time::timeout(
            Duration::from_millis(50),
            client.request_stream(json!({}), None),
        )
        .await;

        assert!(pending.is_err(), "send must still be waiting for capacity");
        assert!(client.sessions.read().await.is_empty());
    }

    fn frame(session_id: &str, frame_id: u32, payload: Value) -> WsMessage {
        WsMessage::StreamFrame {
            session_id: session_id.to_string(),
            frame_id,
            priority: 1,
            payload,
            is_complete: false,
        }
    }

    async fn sessions_with(id: &str) -> Arc<RwLock<HashMap<String, ClientStreamSession>>> {
        let sessions = Arc::new(RwLock::new(HashMap::new()));
        sessions
            .write()
            .await
            .insert(id.to_string(), ClientStreamSession::new(id.to_string()));
        sessions
    }

    fn control_channel() -> (ByteBoundedSender<String>, mpsc::Receiver<Envelope<String>>) {
        byte_bounded_channel(MESSAGE_QUEUE_CAPACITY, MAX_QUEUED_MESSAGE_BYTES)
    }

    #[tokio::test]
    async fn test_frame_state_is_bounded_by_counters() {
        let sessions = sessions_with("s").await;
        let (tx, _rx) = control_channel();

        for frame_id in 0..100u32 {
            PjsWebSocketClient::handle_incoming_message(
                sessions.clone(),
                tx.clone(),
                WebSocketClientConfig::default(),
                frame("s", frame_id, json!({"k": frame_id})),
            )
            .await
            .unwrap();
        }

        let session = &sessions.read().await["s"];
        assert_eq!(session.frames_received, 100);
    }

    #[tokio::test]
    async fn test_session_exceeding_byte_cap_is_rejected_and_dropped() {
        let sessions = sessions_with("s").await;
        let (tx, _rx) = control_channel();
        let config = WebSocketClientConfig {
            max_session_bytes: 64,
            ..WebSocketClientConfig::default()
        };

        PjsWebSocketClient::handle_incoming_message(
            sessions.clone(),
            tx.clone(),
            config,
            frame("s", 0, json!({"a": "small"})),
        )
        .await
        .unwrap();
        assert!(sessions.read().await.contains_key("s"));

        let result = PjsWebSocketClient::handle_incoming_message(
            sessions.clone(),
            tx,
            config,
            frame("s", 1, json!({"b": "x".repeat(100)})),
        )
        .await;

        assert!(matches!(result, Err(PjsError::ClientError(_))));
        assert!(!sessions.read().await.contains_key("s"));
    }

    #[test]
    fn test_apply_frame_overwriting_a_key_does_not_grow_byte_count() {
        let mut session = ClientStreamSession::new("s".to_string());
        session.apply_frame(&json!({"k": "aaaa"}), 1024).unwrap();
        let after_first = session.reconstructed_bytes;

        session.apply_frame(&json!({"k": "bbbb"}), 1024).unwrap();

        assert_eq!(session.reconstructed_bytes, after_first);
        assert!(session.reconstructed_bytes >= json_size(&session.reconstructed_data));
    }

    #[test]
    fn test_apply_frame_merges_objects() {
        let mut session = ClientStreamSession::new("s".to_string());
        session
            .apply_frame(&json!({"existing": "value"}), 1024)
            .unwrap();
        session
            .apply_frame(&json!({"new": "data", "existing": "updated"}), 1024)
            .unwrap();

        assert_eq!(session.reconstructed_data["existing"], "updated");
        assert_eq!(session.reconstructed_data["new"], "data");
    }

    #[tokio::test]
    async fn test_completed_session_is_evicted_when_cap_is_reached() {
        let client = PjsWebSocketClient::new("ws://localhost:3001/ws")
            .unwrap()
            .with_config(WebSocketClientConfig {
                max_sessions: 2,
                ..WebSocketClientConfig::default()
            });
        let live = client.request_stream(json!({}), None).await.unwrap();
        let done = client.request_stream(json!({}), None).await.unwrap();
        let (tx, _rx) = control_channel();
        PjsWebSocketClient::handle_incoming_message(
            client.sessions.clone(),
            tx,
            client.config,
            WsMessage::StreamComplete {
                session_id: done.clone(),
                checksum: "c".to_string(),
            },
        )
        .await
        .unwrap();

        let fresh = client.request_stream(json!({}), None).await.unwrap();

        let sessions = client.sessions.read().await;
        assert!(sessions.contains_key(&live));
        assert!(!sessions.contains_key(&done));
        assert!(sessions.contains_key(&fresh));
    }

    #[derive(Clone, Default)]
    struct CapturedLogs(Arc<std::sync::Mutex<Vec<u8>>>);

    impl std::io::Write for CapturedLogs {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CapturedLogs {
        type Writer = CapturedLogs;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    #[tokio::test]
    async fn test_server_supplied_strings_cannot_forge_log_lines() {
        let logs = CapturedLogs::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(logs.clone())
            .with_ansi(false)
            .with_max_level(tracing::Level::DEBUG)
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);
        let hostile = "\n\r\x1b[31m".to_string();
        let sessions = sessions_with("s").await;
        let (tx, _rx) = control_channel();
        let config = WebSocketClientConfig::default();

        for message in [
            WsMessage::StreamComplete {
                session_id: hostile.clone(),
                checksum: hostile.clone(),
            },
            WsMessage::Error {
                session_id: Some(hostile.clone()),
                error: hostile.clone(),
                code: 500,
            },
            frame(&hostile, 0, json!({})),
        ] {
            PjsWebSocketClient::handle_incoming_message(
                sessions.clone(),
                tx.clone(),
                config,
                message,
            )
            .await
            .unwrap();
        }

        let output = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
        assert!(
            !output.contains('\r'),
            "raw CR leaked into logs: {output:?}"
        );
        assert!(
            !output.contains('\x1b'),
            "raw ESC leaked into logs: {output:?}"
        );
        assert_eq!(
            output.lines().count(),
            3,
            "a hostile string started its own log line: {output:?}"
        );
    }
}
