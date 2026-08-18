//! WebSocket transport layer for real-time PJS streaming
//!
//! Provides WebSocket-based streaming with progressive JSON delivery
//! and backpressure handling for optimal client performance.

use crate::{
    Error as PjsError, Result as PjsResult, StreamFrame, domain::Priority, security::RateLimitGuard,
};
use futures::{Sink, SinkExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    future::Future,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{RwLock, broadcast};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

#[cfg(feature = "websocket-client")]
pub mod client;
pub mod security;
#[cfg(feature = "http-server")]
pub mod server;

#[cfg(feature = "websocket-client")]
pub use client::{PjsWebSocketClient, StreamStats};
pub use security::SecureWebSocketHandler;
#[cfg(feature = "http-server")]
pub use server::{AxumWebSocketTransport, create_websocket_router};

/// Default deadline for a single outbound WebSocket sink write.
///
/// Writing to the sink blocks until the peer's TCP receive buffer drains;
/// a peer that stops reading (dead connection, slow-loris) would otherwise
/// wedge the connection's task — and, on the server, the
/// `Arc<RateLimitGuard>` it holds — until the OS eventually times out the
/// socket. No existing timeout in `config::security::NetworkLimits` fits
/// this: `connection_timeout_secs` bounds establishing a connection, not a
/// steady-state write. 10s is long enough to absorb ordinary network
/// jitter while still freeing a stuck connection promptly.
///
/// This bounds a single `feed`+`flush`, not a minimum throughput: a
/// legitimate large frame (see `MAX_QUEUED_OUTGOING_BYTES` in `server.rs`,
/// up to 16 MiB) sent to a genuinely slow-but-honest client needs roughly
/// 13 Mbps to flush inside 10s, or it gets disconnected same as a stalled
/// peer would. This is an accepted, documented tradeoff rather than a
/// per-byte deadline: distinguishing "slow but making progress" from
/// "stalled" would need tracking partial-write progress, which
/// `Sink::send`'s `feed`+`flush` doesn't expose, and misclassifying a
/// stalled peer as "still making progress" is the failure mode this
/// timeout exists to close. Operators serving large frames to
/// bandwidth-constrained clients should raise the value passed to
/// [`send_with_write_timeout`] accordingly (the server threads its value
/// through `RateLimitConfig::write_timeout`).
///
/// Shares its 10s default with `RateLimitConfig::write_timeout`, but the
/// two are independent constants gated behind different features
/// (`http-server` vs. none) and can't easily share a single definition —
/// an intentional change to one's default should be mirrored in the other
/// unless a divergence is deliberate.
pub(crate) const WRITE_TIMEOUT: Duration = Duration::from_secs(10);

/// Writes `message` to `sink`, aborting the write if it does not complete
/// within `timeout` (see [`WRITE_TIMEOUT`] for the production default and
/// the rationale/tradeoffs of a fixed per-write deadline).
///
/// Used at every outbound WebSocket write site (server and client). A
/// timeout is treated the same as a genuine send error — both mean the
/// caller should stop and close the connection. Taking `timeout` as a
/// parameter (rather than hardcoding [`WRITE_TIMEOUT`]) lets callers
/// configure it (see `RateLimitConfig::write_timeout` on the server side,
/// and `PjsWebSocketClient::with_write_timeout` on the client side) and
/// lets tests exercise a real stall deterministically with a short
/// deadline instead of waiting out the production value.
pub(crate) async fn send_with_write_timeout<S, M>(
    sink: &mut S,
    message: M,
    timeout: Duration,
) -> Result<(), String>
where
    S: Sink<M> + Unpin,
    S::Error: std::fmt::Display,
{
    match tokio::time::timeout(timeout, sink.send(message)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(format!("send failed: {e}")),
        Err(_elapsed) => Err(format!("write stalled for {timeout:?}")),
    }
}

#[cfg(test)]
mod write_timeout_tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn test_send_with_write_timeout_times_out_on_stalled_sink() {
        let handle = tokio::spawn(async {
            let mut sink = futures::sink::unfold((), |_, _item: &str| {
                futures::future::pending::<Result<(), std::io::Error>>()
            });
            send_with_write_timeout(&mut sink, "hello", Duration::from_millis(200)).await
        });

        tokio::time::advance(Duration::from_millis(201)).await;

        let result = handle.await.expect("task panicked");
        assert!(
            result.is_err(),
            "a write that never completes must time out, not hang forever"
        );
    }

    #[tokio::test]
    async fn test_send_with_write_timeout_succeeds_on_ready_sink() {
        let mut sink = futures::sink::drain();
        send_with_write_timeout(&mut sink, "hello", WRITE_TIMEOUT)
            .await
            .expect("a sink that accepts immediately must not be treated as stalled");
    }
}

/// WebSocket message types for PJS streaming
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum WsMessage {
    /// Stream initialization request
    StreamInit {
        /// Identifier of the WebSocket session.
        session_id: String,
        /// Source JSON payload to be streamed.
        data: Value,
        /// Per-stream options controlling framing and compression.
        options: StreamOptions,
    },
    /// Stream frame with priority data
    StreamFrame {
        /// Identifier of the WebSocket session.
        session_id: String,
        /// Monotonic frame index within the session.
        frame_id: u32,
        /// Priority assigned to this frame.
        priority: u8,
        /// Payload carried by the frame.
        payload: Value,
        /// Whether this frame completes the stream.
        is_complete: bool,
    },
    /// Client acknowledgment of frame
    FrameAck {
        /// Identifier of the WebSocket session.
        session_id: String,
        /// Index of the frame being acknowledged.
        frame_id: u32,
        /// Time the client took to process the frame, in milliseconds.
        processing_time_ms: u64,
    },
    /// Stream completion signal
    StreamComplete {
        /// Identifier of the WebSocket session.
        session_id: String,
        /// SHA-256 checksum of the concatenated frame payloads.
        checksum: String,
    },
    /// Error message
    Error {
        /// Identifier of the WebSocket session, if known.
        session_id: Option<String>,
        /// Human-readable error description.
        error: String,
        /// Numeric error code.
        code: u16,
    },
    /// Heartbeat/ping message
    Ping {
        /// Wall-clock timestamp at which the ping was sent.
        timestamp: u64,
    },
    /// Heartbeat/pong response
    Pong {
        /// Wall-clock timestamp at which the pong was sent.
        timestamp: u64,
    },
}

/// Stream configuration options
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamOptions {
    /// Maximum frame size in bytes
    pub max_frame_size: usize,
    /// Client processing capability (frames per second)
    pub client_fps: Option<u32>,
    /// Enable compression
    pub compression: bool,
    /// Custom priority mapping
    pub priority_mapping: Option<HashMap<String, u8>>,
}

impl Default for StreamOptions {
    fn default() -> Self {
        Self {
            max_frame_size: 64 * 1024, // 64KB
            client_fps: None,          // Auto-detect
            compression: true,
            priority_mapping: None,
        }
    }
}

/// WebSocket streaming session state.
///
/// This type is intentionally distinct from the domain-layer
/// [`crate::domain::aggregates::StreamSession`] aggregate. The WebSocket
/// transport maintains an ephemeral, transport-local session model with raw
/// `String` identifiers and an in-memory `HashMap` keyed off
/// [`AdaptiveStreamController`]; it does **not** share state with the
/// `StreamRepositoryGat`-backed domain session created by
/// `POST /pjs/sessions`. Sessions created over WebSocket cannot be addressed
/// over HTTP and vice versa, and dictionary training, auth, and rate-limit
/// middleware applied to the HTTP router do not apply to this controller.
///
/// See issue #239 for the full rationale and the deliberate split between
/// the two session models.
#[derive(Debug)]
pub struct WebSocketStreamSession {
    /// Transport-local session identifier.
    pub id: String,
    /// Instant the session was created.
    pub created_at: Instant,
    /// Streaming options negotiated for this session.
    pub options: StreamOptions,
    /// Pre-computed delivery plan as an ordered list of frames.
    pub plan: Vec<StreamFrame>,
    /// Index of the next frame to send.
    pub current_frame: u32,
    /// Frame indices acknowledged by the client so far.
    pub acknowledged_frames: Vec<u32>,
    /// Adaptive streaming metrics derived from client acks.
    pub client_metrics: ClientMetrics,
    /// Rate-limit guard scoped to this session, if installed.
    pub rate_limit_guard: Option<RateLimitGuard>,
    /// Handle to abort the per-session frame-streaming task on teardown.
    ///
    /// Not `pub`: only ever set and read within this module, which keeps
    /// this session-lifecycle detail out of the public struct-literal
    /// surface.
    stream_task: Option<tokio::task::AbortHandle>,
}

/// Client performance metrics for adaptive streaming
#[derive(Debug, Default)]
pub struct ClientMetrics {
    /// Exponential moving average of client frame-processing time, in milliseconds.
    pub average_processing_time_ms: f64,
    /// Number of frames the client has acknowledged.
    pub frames_acknowledged: u32,
    /// Instant of the most recent acknowledgement, if any.
    pub last_ack_time: Option<Instant>,
    /// Estimated downlink bandwidth, in kilobits per second, if measured.
    pub estimated_bandwidth_kbps: Option<f64>,
    /// Round-trip time of the WebSocket connection, in milliseconds, if measured.
    pub connection_rtt_ms: Option<u64>,
}

impl ClientMetrics {
    /// Fold a new processing-time observation into the moving average.
    pub fn update_processing_time(&mut self, processing_time_ms: u64) {
        let new_time = processing_time_ms as f64;
        if self.frames_acknowledged == 0 {
            self.average_processing_time_ms = new_time;
        } else {
            // Exponential moving average
            let alpha = 0.3;
            self.average_processing_time_ms =
                alpha * new_time + (1.0 - alpha) * self.average_processing_time_ms;
        }
        self.frames_acknowledged += 1;
        self.last_ack_time = Some(Instant::now());
    }

    /// Returns `true` when average client processing time exceeds the slow-client threshold.
    pub fn is_client_slow(&self) -> bool {
        self.average_processing_time_ms > 100.0 // > 100ms per frame
    }

    /// Recommended delay between frames given the current processing-time average.
    ///
    /// Clamped to `MAX_ADAPTIVE_FRAME_DELAY`: `average_processing_time_ms` is derived
    /// from client-supplied `processing_time_ms` in [`Self::update_processing_time`],
    /// which is unvalidated wire input (see `handle_frame_ack`). Without a ceiling, a
    /// single malicious `FrameAck` could drive the `tokio::time::sleep` in
    /// `AdaptiveStreamController::stream_frames` to an arbitrarily long duration and
    /// stall that session's stream.
    pub fn recommended_frame_delay(&self) -> Duration {
        if self.is_client_slow() {
            Duration::from_millis((self.average_processing_time_ms * 0.5) as u64)
                .min(MAX_ADAPTIVE_FRAME_DELAY)
        } else {
            Duration::from_millis(10) // Fast clients get minimal delay
        }
    }
}

/// Upper bound on the per-frame delay returned by [`ClientMetrics::recommended_frame_delay`].
const MAX_ADAPTIVE_FRAME_DELAY: Duration = Duration::from_secs(1);

/// WebSocket transport trait for different implementations (GAT-based)
pub trait WebSocketTransport: Send + Sync {
    /// Concrete connection type the implementor uses for I/O.
    type Connection: Send + Sync;

    /// Future type for starting stream
    type StartStreamFuture<'a>: Future<Output = PjsResult<String>> + Send + 'a
    where
        Self: 'a;

    /// Future type for sending frame
    type SendFrameFuture<'a>: Future<Output = PjsResult<()>> + Send + 'a
    where
        Self: 'a;

    /// Future type for handling message
    type HandleMessageFuture<'a>: Future<Output = PjsResult<()>> + Send + 'a
    where
        Self: 'a;

    /// Future type for closing stream
    type CloseStreamFuture<'a>: Future<Output = PjsResult<()>> + Send + 'a
    where
        Self: 'a;

    /// Start streaming session
    fn start_stream(
        &self,
        connection: Arc<Self::Connection>,
        data: Value,
        options: StreamOptions,
    ) -> Self::StartStreamFuture<'_>;

    /// Send frame to client
    ///
    /// Implementors typically queue `message` onto a per-connection channel
    /// consumed by that same connection's I/O loop. Do not call this method
    /// from within that connection's own message-handling path (e.g. from
    /// [`WebSocketTransport::handle_message`]): if the implementor applies
    /// backpressure by awaiting channel capacity rather than dropping,
    /// calling from the same loop that drains the channel can deadlock the
    /// connection.
    fn send_frame(
        &self,
        connection: Arc<Self::Connection>,
        message: WsMessage,
    ) -> Self::SendFrameFuture<'_>;

    /// Handle incoming message
    fn handle_message(
        &self,
        connection: Arc<Self::Connection>,
        message: WsMessage,
    ) -> Self::HandleMessageFuture<'_>;

    /// Close streaming session
    fn close_stream(&self, session_id: &str) -> Self::CloseStreamFuture<'_>;
}

/// Adaptive streaming controller
pub struct AdaptiveStreamController {
    sessions: Arc<RwLock<HashMap<String, WebSocketStreamSession>>>,
    frame_tx: broadcast::Sender<(String, WsMessage)>,
}

impl AdaptiveStreamController {
    /// Create an empty controller with no active sessions.
    pub fn new() -> Self {
        let (frame_tx, _) = broadcast::channel(1000);

        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            frame_tx,
        }
    }

    /// Create new streaming session
    pub async fn create_session(&self, data: Value, options: StreamOptions) -> PjsResult<String> {
        let session_id = Uuid::new_v4().to_string();
        let plan = vec![StreamFrame {
            data: data.clone(),
            priority: Priority::HIGH,
            metadata: std::collections::HashMap::new(),
        }]; // Simplified for now

        let session = WebSocketStreamSession {
            id: session_id.clone(),
            created_at: Instant::now(),
            options,
            plan,
            current_frame: 0,
            acknowledged_frames: Vec::new(),
            client_metrics: ClientMetrics::default(),
            rate_limit_guard: None, // Will be set when connection is established
            stream_task: None,      // Set when streaming starts
        };

        self.sessions
            .write()
            .await
            .insert(session_id.clone(), session);

        info!("Created streaming session: {}", session_id);
        Ok(session_id)
    }

    /// Start streaming frames for session
    pub async fn start_streaming(&self, session_id: &str) -> PjsResult<()> {
        let mut sessions = self.sessions.write().await;
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| PjsError::InvalidSession(session_id.to_string()))?;

        // Start streaming task
        let session_id = session_id.to_string();
        let frame_tx = self.frame_tx.clone();
        let plan = session.plan.clone();

        let task_session_id = session_id.clone();
        let sessions_for_task = self.sessions.clone();
        let handle = tokio::spawn(async move {
            if let Err(e) =
                Self::stream_frames(task_session_id, plan, frame_tx, sessions_for_task).await
            {
                error!("Error streaming frames: {}", e);
            }
        });

        // Keep an abort handle so the task can be cancelled on session teardown,
        // and supervise the join handle to surface panics that would otherwise
        // be silently swallowed by the runtime. Abort any previous task first —
        // a repeated start_streaming call for the same session would otherwise
        // overwrite the handle and leak the earlier task.
        if let Some(previous) = session.stream_task.replace(handle.abort_handle()) {
            previous.abort();
        }
        tokio::spawn(async move {
            match handle.await {
                Ok(()) => {}
                Err(join_err) if join_err.is_panic() => {
                    error!(
                        "Streaming task panicked for session {}: {}",
                        session_id, join_err
                    );
                }
                Err(_) => {} // task was aborted — expected on session teardown
            }
        });

        Ok(())
    }

    async fn stream_frames(
        session_id: String,
        plan: Vec<StreamFrame>, // Simplified for now
        frame_tx: broadcast::Sender<(String, WsMessage)>,
        sessions: Arc<RwLock<HashMap<String, WebSocketStreamSession>>>,
    ) -> Result<(), PjsError> {
        let mut frames_data = Vec::new();

        for (frame_id, frame) in plan.iter().enumerate() {
            // Collect frame payload for checksum calculation
            let payload_bytes =
                serde_json::to_vec(&frame.data).map_err(|e| PjsError::Other(e.to_string()))?;
            frames_data.push(payload_bytes);

            let ws_message = WsMessage::StreamFrame {
                session_id: session_id.clone(),
                frame_id: frame_id as u32,
                priority: frame.priority.value(),
                payload: frame.data.clone(),
                is_complete: frame_id == (plan.len() - 1),
            };

            if let Err(e) = frame_tx.send((session_id.clone(), ws_message)) {
                error!("Failed to send frame {}: {}", frame_id, e);
                break;
            }

            let delay = sessions
                .read()
                .await
                .get(&session_id)
                .map(|session| session.client_metrics.recommended_frame_delay())
                .unwrap_or(Duration::from_millis(10));
            tokio::time::sleep(delay).await;
        }

        // Send completion message with calculated checksum
        let complete_message = WsMessage::StreamComplete {
            session_id: session_id.clone(),
            checksum: calculate_stream_checksum(&frames_data),
        };

        let _ = frame_tx.send((session_id, complete_message));
        Ok(())
    }

    /// Handle frame acknowledgment
    pub async fn handle_frame_ack(
        &self,
        session_id: &str,
        frame_id: u32,
        processing_time_ms: u64,
    ) -> PjsResult<()> {
        let mut sessions = self.sessions.write().await;
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| PjsError::InvalidSession(session_id.to_string()))?;

        session.acknowledged_frames.push(frame_id);
        session
            .client_metrics
            .update_processing_time(processing_time_ms);

        debug!(
            "Frame {} acknowledged for session {} (processing: {}ms, avg: {:.1}ms)",
            frame_id,
            session_id,
            processing_time_ms,
            session.client_metrics.average_processing_time_ms
        );

        if session.client_metrics.is_client_slow() {
            warn!(
                "Client {} is processing slowly (avg: {:.1}ms)",
                session_id, session.client_metrics.average_processing_time_ms
            );
        }

        Ok(())
    }

    /// Get subscriber for frame events
    pub fn subscribe_frames(&self) -> broadcast::Receiver<(String, WsMessage)> {
        self.frame_tx.subscribe()
    }

    /// Set rate limit guard for a session
    pub async fn set_rate_limit_guard(
        &self,
        session_id: &str,
        guard: RateLimitGuard,
    ) -> PjsResult<()> {
        let mut sessions = self.sessions.write().await;
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| PjsError::InvalidSession(session_id.to_string()))?;

        session.rate_limit_guard = Some(guard);
        Ok(())
    }

    /// Validate message against rate limits
    pub async fn validate_message(&self, session_id: &str, frame_size: usize) -> PjsResult<()> {
        let sessions = self.sessions.read().await;
        let session = sessions
            .get(session_id)
            .ok_or_else(|| PjsError::InvalidSession(session_id.to_string()))?;

        if let Some(guard) = &session.rate_limit_guard {
            guard
                .check_message(frame_size)
                .map_err(|e| PjsError::SecurityError(format!("Rate limit violation: {}", e)))?;
        }

        Ok(())
    }

    /// Remove a single session by id.
    ///
    /// Returns `true` if the session existed and was removed, `false` if the id
    /// was not present. Callers may safely invoke this multiple times — the
    /// second call is a no-op.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use pjson_rs::infrastructure::websocket::{AdaptiveStreamController, StreamOptions};
    /// # use serde_json::json;
    /// # #[tokio::main] async fn main() {
    /// let controller = AdaptiveStreamController::new();
    /// let id = controller.create_session(json!({}), StreamOptions::default()).await.unwrap();
    /// assert!(controller.remove_session(&id).await);
    /// // Idempotent — second call is a no-op:
    /// assert!(!controller.remove_session(&id).await);
    /// # }
    /// ```
    pub async fn remove_session(&self, session_id: &str) -> bool {
        let mut sessions = self.sessions.write().await;
        let removed = sessions.remove(session_id);
        match &removed {
            Some(session) => {
                if let Some(abort_handle) = &session.stream_task {
                    abort_handle.abort();
                }
                info!("Removed streaming session: {}", session_id);
            }
            None => debug!("remove_session called on unknown id: {}", session_id),
        }
        removed.is_some()
    }

    /// Clean up expired sessions
    pub async fn cleanup_expired_sessions(&self, max_age: Duration) {
        let mut sessions = self.sessions.write().await;
        let now = Instant::now();

        sessions.retain(|id, session| {
            let expired = now.duration_since(session.created_at) > max_age;
            if expired {
                if let Some(abort_handle) = &session.stream_task {
                    abort_handle.abort();
                }
                info!("Cleaning up expired session: {}", id);
            }
            !expired
        });
    }
}

impl Default for AdaptiveStreamController {
    fn default() -> Self {
        Self::new()
    }
}

/// Calculate SHA-256 checksum for stream completion verification
fn calculate_stream_checksum(frames_data: &[Vec<u8>]) -> String {
    let mut hasher = Sha256::new();

    // Hash each frame's data
    for frame_data in frames_data {
        hasher.update(frame_data);
    }

    // Hash frame count to ensure integrity
    hasher.update((frames_data.len() as u64).to_le_bytes());

    let result = hasher.finalize();
    let hex: String = result.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("sha256:{hex}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_create_session() {
        let controller = AdaptiveStreamController::new();
        let data = json!({
            "critical": {"id": 1, "status": "active"},
            "details": {"name": "test", "description": "test data"}
        });

        let session_id = controller
            .create_session(data, StreamOptions::default())
            .await
            .unwrap();

        assert!(!session_id.is_empty());

        let sessions = controller.sessions.read().await;
        assert!(sessions.contains_key(&session_id));
    }

    #[tokio::test]
    async fn test_frame_acknowledgment() {
        let controller = AdaptiveStreamController::new();
        let data = json!({"test": "data"});

        let session_id = controller
            .create_session(data, StreamOptions::default())
            .await
            .unwrap();

        controller
            .handle_frame_ack(&session_id, 0, 50)
            .await
            .unwrap();

        let sessions = controller.sessions.read().await;
        let session = sessions.get(&session_id).unwrap();
        assert_eq!(session.acknowledged_frames, vec![0]);
        assert_eq!(session.client_metrics.average_processing_time_ms, 50.0);
    }

    /// Proves `remove_session` actually stops the streaming task rather
    /// than merely dropping the session's bookkeeping entry: a long plan
    /// is aborted before it can send every frame or the completion
    /// message, instead of running to completion in the background.
    #[tokio::test]
    async fn test_remove_session_aborts_streaming_task_before_completion() {
        let controller = AdaptiveStreamController::new();
        let session_id = controller
            .create_session(json!({"test": "data"}), StreamOptions::default())
            .await
            .unwrap();

        // Long enough (10ms/frame) that the task is still far from done
        // when we abort it immediately below.
        {
            let mut sessions = controller.sessions.write().await;
            let session = sessions.get_mut(&session_id).unwrap();
            session.plan = (0..200)
                .map(|_| StreamFrame {
                    data: json!({}),
                    priority: Priority::HIGH,
                    metadata: std::collections::HashMap::new(),
                })
                .collect();
        }

        let mut frames_rx = controller.subscribe_frames();

        controller.start_streaming(&session_id).await.unwrap();
        assert!(controller.remove_session(&session_id).await);

        // Drain whatever the task managed to emit before the abort took
        // effect, for a window far shorter than the full 200-frame plan
        // (~2s) would take to complete on its own.
        let mut saw_complete = false;
        let mut frame_count = 0;
        let drain_deadline = tokio::time::Instant::now() + Duration::from_millis(300);
        while tokio::time::Instant::now() < drain_deadline {
            match tokio::time::timeout(Duration::from_millis(20), frames_rx.recv()).await {
                Ok(Ok((_, WsMessage::StreamComplete { .. }))) => {
                    saw_complete = true;
                    break;
                }
                Ok(Ok(_)) => frame_count += 1,
                Ok(Err(_)) => break, // channel closed
                Err(_) => {}         // no message in this slice; keep polling
            }
        }

        assert!(
            !saw_complete,
            "streaming task must not run to completion after remove_session aborts it"
        );
        assert!(
            frame_count < 200,
            "streaming task must stop well short of the full plan once aborted, sent {frame_count} frames"
        );
    }

    #[test]
    fn test_client_metrics() {
        let mut metrics = ClientMetrics::default();

        metrics.update_processing_time(100);
        assert_eq!(metrics.average_processing_time_ms, 100.0);

        metrics.update_processing_time(200);
        // Should be exponential moving average: 0.3 * 200 + 0.7 * 100 = 130
        assert!((metrics.average_processing_time_ms - 130.0).abs() < 0.1);

        assert!(metrics.is_client_slow());
    }

    /// Regression test for a malicious `FrameAck` (see `handle_frame_ack`) reporting an
    /// astronomical `processing_time_ms`: without the clamp in `recommended_frame_delay`,
    /// this would drive its output — and thus the per-frame `sleep` in `stream_frames` —
    /// to roughly 46 days for a single ack.
    #[test]
    fn test_recommended_frame_delay_clamps_extreme_processing_time() {
        let mut metrics = ClientMetrics::default();
        metrics.update_processing_time(100_000_000_000);

        assert_eq!(
            metrics.recommended_frame_delay(),
            MAX_ADAPTIVE_FRAME_DELAY,
            "delay must be clamped to MAX_ADAPTIVE_FRAME_DELAY, not scale unbounded with client-supplied input"
        );
    }

    /// Exercises the actual per-frame delay read path in `stream_frames`, not just the
    /// pure `recommended_frame_delay` function: a session whose `client_metrics` were
    /// poisoned by a malicious ack must still complete its stream promptly instead of
    /// stalling on an unbounded `tokio::time::sleep`.
    #[tokio::test]
    async fn test_stream_frames_completes_promptly_under_malicious_client_metrics() {
        let controller = AdaptiveStreamController::new();
        let session_id = controller
            .create_session(json!({"test": "data"}), StreamOptions::default())
            .await
            .unwrap();

        {
            let mut sessions = controller.sessions.write().await;
            sessions
                .get_mut(&session_id)
                .unwrap()
                .client_metrics
                .update_processing_time(100_000_000_000);
        }

        let mut frames_rx = controller.subscribe_frames();
        controller.start_streaming(&session_id).await.unwrap();

        let result = tokio::time::timeout(MAX_ADAPTIVE_FRAME_DELAY * 2, async {
            loop {
                match frames_rx
                    .recv()
                    .await
                    .expect("channel must not close early")
                {
                    (sid, WsMessage::StreamComplete { .. }) if sid == session_id => break,
                    _ => continue,
                }
            }
        })
        .await;

        assert!(
            result.is_ok(),
            "stream must complete within 2x MAX_ADAPTIVE_FRAME_DELAY, not stall on malicious client metrics"
        );
    }

    #[test]
    fn test_checksum_calculation() {
        // Test empty frames
        let empty_frames: Vec<Vec<u8>> = vec![];
        let checksum = calculate_stream_checksum(&empty_frames);
        assert!(checksum.starts_with("sha256:"));

        // Test single frame
        let single_frame = vec![vec![1, 2, 3, 4]];
        let checksum1 = calculate_stream_checksum(&single_frame);
        assert!(checksum1.starts_with("sha256:"));

        // Test multiple frames
        let multi_frames = vec![vec![1, 2], vec![3, 4], vec![5, 6]];
        let checksum2 = calculate_stream_checksum(&multi_frames);
        assert!(checksum2.starts_with("sha256:"));

        // Same data should produce same checksum
        let same_frames = vec![vec![1, 2], vec![3, 4], vec![5, 6]];
        let checksum3 = calculate_stream_checksum(&same_frames);
        assert_eq!(checksum2, checksum3);

        // Different data should produce different checksum
        let diff_frames = vec![vec![1, 2], vec![3, 4], vec![5, 7]]; // Last byte different
        let checksum4 = calculate_stream_checksum(&diff_frames);
        assert_ne!(checksum2, checksum4);

        // Different order should produce different checksum
        let reordered_frames = vec![vec![3, 4], vec![1, 2], vec![5, 6]];
        let checksum5 = calculate_stream_checksum(&reordered_frames);
        assert_ne!(checksum2, checksum5);
    }
}
