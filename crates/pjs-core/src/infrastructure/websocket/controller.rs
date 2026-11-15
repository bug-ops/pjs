//! Transport-local session state and the adaptive streaming controller.
//!
//! Owns the sessions a WebSocket server transport streams to its connections
//! and paces their frames from the acknowledgments clients send back.

use super::protocol::{StreamOptions, WsMessage};
use crate::{
    Error as PjsError, Result as PjsResult, StreamFrame,
    domain::{Priority, SessionId},
    infrastructure::http::ShutdownSignal,
    security::RateLimitGuard,
};
use parking_lot::RwLock;
use serde_json::Value;
use std::{
    collections::{HashMap, hash_map::Entry},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{sync::broadcast, task::JoinHandle};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Identity of one accepted WebSocket connection.
///
/// Generated server-side and never sent on the wire. Sessions record the
/// connection that created them, and the frame broadcast is tagged with it, so
/// a connection only ever receives and acknowledges its own sessions.
///
/// # Examples
///
/// ```
/// use pjson_rs::infrastructure::websocket::ConnectionId;
///
/// let a = ConnectionId::new();
/// assert_eq!(a, a);
/// assert_ne!(a, ConnectionId::new());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConnectionId(Uuid);

impl ConnectionId {
    /// Generates a fresh random connection id.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ConnectionId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ConnectionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// WebSocket streaming session state.
///
/// This type is intentionally distinct from the domain-layer
/// [`crate::domain::aggregates::StreamSession`] aggregate. The WebSocket
/// transport maintains an ephemeral, transport-local session model with raw
/// [`SessionId`] identifiers and an in-memory `HashMap` keyed off
/// [`AdaptiveStreamController`]; it does **not** share state with the
/// `StreamRepositoryGat`-backed domain session created by
/// `POST /pjs/sessions`. Sessions created over WebSocket cannot be addressed
/// over HTTP and vice versa, and dictionary training, auth, and rate-limit
/// middleware applied to the HTTP router do not apply to this controller.
///
/// See issue #239 for the full rationale and the deliberate split between
/// the two session models.
#[derive(Debug)]
pub(crate) struct WebSocketStreamSession {
    /// Instant the session was created.
    pub created_at: Instant,
    /// Streaming options negotiated for this session.
    #[expect(
        dead_code,
        reason = "recorded for the session; the planner that reads it is #600's deferred scope"
    )]
    pub options: StreamOptions,
    /// Pre-computed delivery plan as an ordered list of frames.
    pub plan: Vec<StreamFrame>,
    /// Adaptive streaming metrics derived from client acks.
    pub client_metrics: ClientMetrics,
    /// Rate-limit guard scoped to this session, if installed.
    pub rate_limit_guard: Option<RateLimitGuard>,
    /// Connection that created the session; the only one allowed to receive
    /// its frames and acknowledge them.
    owner: ConnectionId,
    /// Handle to abort the per-session frame-streaming task on teardown.
    ///
    /// Not `pub`: only ever set and read within this module, which keeps
    /// this session-lifecycle detail out of the public struct-literal
    /// surface.
    stream_task: Option<tokio::task::AbortHandle>,
}

#[cfg(test)]
impl WebSocketStreamSession {
    /// Connection that created this session.
    pub fn owner(&self) -> ConnectionId {
        self.owner
    }
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

/// Why [`AdaptiveStreamController::create_session`] refused a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CreateSessionError {
    /// A session with this id already exists and was left untouched.
    #[error("session {0} already exists")]
    AlreadyExists(SessionId),
}

impl From<CreateSessionError> for PjsError {
    fn from(error: CreateSessionError) -> Self {
        PjsError::InvalidSession(error.to_string())
    }
}

/// One event on the frame broadcast: the connection that owns the session, the
/// session, and the message.
///
/// The message is shared behind an [`Arc`] so every subscribed connection gets
/// a cheap handle instead of a deep copy of the payload; receivers compare the
/// owner tag and drop events that belong to other connections.
pub type FrameEvent = (ConnectionId, SessionId, Arc<WsMessage>);

/// Adaptive streaming controller.
///
/// Owns the transport-local sessions and the frame broadcast. Every session
/// records the [`ConnectionId`] that created it; frames are tagged with that
/// owner and acknowledgments are accepted only from it.
pub struct AdaptiveStreamController {
    pub(super) sessions: Arc<RwLock<HashMap<SessionId, WebSocketStreamSession>>>,
    frame_tx: broadcast::Sender<FrameEvent>,
    shutdown: Option<ShutdownSignal>,
}

/// Spawns `fut` on `signal`'s tracker when one is set, so a shutdown drain
/// waits for it and can force-drop it; otherwise on the plain Tokio runtime.
pub(super) fn spawn_task<F>(signal: Option<&ShutdownSignal>, fut: F) -> JoinHandle<()>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    match signal {
        Some(signal) => signal.spawn_tracked(fut),
        None => tokio::spawn(fut),
    }
}

impl AdaptiveStreamController {
    /// Create an empty controller with no active sessions.
    pub fn new() -> Self {
        // TODO(#570): shared broadcast lag drops other connections' frames
        let (frame_tx, _) = broadcast::channel(1000);

        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            frame_tx,
            shutdown: None,
        }
    }

    /// Spawns every session task on `signal`'s tracker, so a shutdown drain
    /// waits for them and can force-drop them.
    ///
    /// # Examples
    ///
    /// ```
    /// use pjson_rs::infrastructure::http::ShutdownSignal;
    /// use pjson_rs::infrastructure::websocket::AdaptiveStreamController;
    ///
    /// let _controller = AdaptiveStreamController::new().with_shutdown(ShutdownSignal::new());
    /// ```
    #[must_use]
    pub fn with_shutdown(mut self, signal: ShutdownSignal) -> Self {
        self.shutdown = Some(signal);
        self
    }

    /// Create a streaming session under `session_id`, owned by `owner`.
    ///
    /// An existing session is never overwritten: its id may already be routed
    /// to another connection.
    ///
    /// # Errors
    ///
    /// Returns [`CreateSessionError::AlreadyExists`] if `session_id` is already in use.
    ///
    /// # Examples
    ///
    /// ```
    /// use pjson_rs::domain::SessionId;
    /// use pjson_rs::infrastructure::websocket::{
    ///     AdaptiveStreamController, ConnectionId, StreamOptions,
    /// };
    /// use serde_json::json;
    ///
    /// # #[tokio::main] async fn main() {
    /// let controller = AdaptiveStreamController::new();
    /// let id = SessionId::new();
    /// let owner = ConnectionId::new();
    /// controller
    ///     .create_session(id, owner, json!({}), StreamOptions::default())
    ///     .await
    ///     .unwrap();
    /// // The id is taken; a second connection cannot claim it.
    /// assert!(controller
    ///     .create_session(id, ConnectionId::new(), json!({}), StreamOptions::default())
    ///     .await
    ///     .is_err());
    /// # }
    /// ```
    pub async fn create_session(
        &self,
        session_id: SessionId,
        owner: ConnectionId,
        data: Value,
        options: StreamOptions,
    ) -> Result<(), CreateSessionError> {
        let plan = vec![StreamFrame {
            data,
            priority: Priority::HIGH,
            metadata: HashMap::new(),
        }]; // Simplified for now

        let session = WebSocketStreamSession {
            created_at: Instant::now(),
            options,
            plan,
            client_metrics: ClientMetrics::default(),
            rate_limit_guard: None, // Will be set when connection is established
            owner,
            stream_task: None, // Set when streaming starts
        };

        match self.sessions.write().entry(session_id) {
            Entry::Occupied(_) => return Err(CreateSessionError::AlreadyExists(session_id)),
            Entry::Vacant(slot) => {
                slot.insert(session);
            }
        }

        info!("Created streaming session: {}", session_id);
        Ok(())
    }

    /// Start streaming frames for session
    pub async fn start_streaming(&self, session_id: SessionId) -> PjsResult<()> {
        let mut sessions = self.sessions.write();
        let session = sessions
            .get_mut(&session_id)
            .ok_or_else(|| PjsError::InvalidSession(session_id.to_string()))?;

        // Start streaming task
        let owner = session.owner;
        let frame_tx = self.frame_tx.clone();
        let plan = session.plan.clone();

        let sessions_for_task = self.sessions.clone();
        let handle = spawn_task(self.shutdown.as_ref(), async move {
            if let Err(e) =
                Self::stream_frames(owner, session_id, plan, frame_tx, sessions_for_task).await
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
        spawn_task(self.shutdown.as_ref(), async move {
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
        owner: ConnectionId,
        session_id: SessionId,
        plan: Vec<StreamFrame>, // Simplified for now
        frame_tx: broadcast::Sender<FrameEvent>,
        sessions: Arc<RwLock<HashMap<SessionId, WebSocketStreamSession>>>,
    ) -> Result<(), PjsError> {
        for (frame_id, frame) in plan.iter().enumerate() {
            let ws_message = WsMessage::StreamFrame {
                session_id,
                frame_id: frame_id as u32,
                priority: frame.priority,
                payload: frame.data.clone(),
                is_complete: frame_id == (plan.len() - 1),
            };

            if let Err(e) = frame_tx.send((owner, session_id, Arc::new(ws_message))) {
                error!("Failed to send frame {}: {}", frame_id, e);
                break;
            }

            let delay = sessions
                .read()
                .get(&session_id)
                .map(|session| session.client_metrics.recommended_frame_delay())
                .unwrap_or(Duration::from_millis(10));
            tokio::time::sleep(delay).await;
        }

        let complete_message = WsMessage::StreamComplete { session_id };

        let _ = frame_tx.send((owner, session_id, Arc::new(complete_message)));
        Ok(())
    }

    /// Handle frame acknowledgment from `owner`.
    ///
    /// # Errors
    ///
    /// Returns [`PjsError::InvalidSession`] if the session does not exist or
    /// belongs to another connection; the metrics are left untouched.
    pub async fn handle_frame_ack(
        &self,
        owner: ConnectionId,
        session_id: SessionId,
        frame_id: u32,
        processing_time_ms: u64,
    ) -> PjsResult<()> {
        let mut sessions = self.sessions.write();
        let session = sessions
            .get_mut(&session_id)
            .filter(|session| session.owner == owner)
            .ok_or_else(|| PjsError::InvalidSession(session_id.to_string()))?;

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

    /// Get subscriber for frame events.
    ///
    /// Receives events of every connection; compare the [`ConnectionId`] tag
    /// to select the ones that belong to you.
    pub fn subscribe_frames(&self) -> broadcast::Receiver<FrameEvent> {
        self.frame_tx.subscribe()
    }

    /// Set rate limit guard for a session
    pub async fn set_rate_limit_guard(
        &self,
        session_id: SessionId,
        guard: RateLimitGuard,
    ) -> PjsResult<()> {
        let mut sessions = self.sessions.write();
        let session = sessions
            .get_mut(&session_id)
            .ok_or_else(|| PjsError::InvalidSession(session_id.to_string()))?;

        session.rate_limit_guard = Some(guard);
        Ok(())
    }

    /// Validate message against rate limits
    pub async fn validate_message(
        &self,
        session_id: SessionId,
        frame_size: usize,
    ) -> PjsResult<()> {
        let sessions = self.sessions.read();
        let session = sessions
            .get(&session_id)
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
    /// ```
    /// use pjson_rs::domain::SessionId;
    /// use pjson_rs::infrastructure::websocket::{
    ///     AdaptiveStreamController, ConnectionId, StreamOptions,
    /// };
    /// use serde_json::json;
    ///
    /// # #[tokio::main] async fn main() {
    /// let controller = AdaptiveStreamController::new();
    /// let id = SessionId::new();
    /// controller
    ///     .create_session(id, ConnectionId::new(), json!({}), StreamOptions::default())
    ///     .await
    ///     .unwrap();
    /// assert!(controller.remove_session(id).await);
    /// // Idempotent — second call is a no-op:
    /// assert!(!controller.remove_session(id).await);
    /// # }
    /// ```
    pub async fn remove_session(&self, session_id: SessionId) -> bool {
        let mut sessions = self.sessions.write();
        let removed = sessions.remove(&session_id);
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

    /// Remove every session created by `owner`, aborting their streaming tasks.
    ///
    /// Returns the number of sessions removed. Used when a connection closes.
    /// Synchronous so a connection's drop guard can call it.
    ///
    /// # Examples
    ///
    /// ```
    /// use pjson_rs::domain::SessionId;
    /// use pjson_rs::infrastructure::websocket::{
    ///     AdaptiveStreamController, ConnectionId, StreamOptions,
    /// };
    /// use serde_json::json;
    ///
    /// # #[tokio::main] async fn main() {
    /// let controller = AdaptiveStreamController::new();
    /// let (a, b) = (ConnectionId::new(), ConnectionId::new());
    /// for owner in [a, a, b] {
    ///     controller
    ///         .create_session(SessionId::new(), owner, json!({}), StreamOptions::default())
    ///         .await
    ///         .unwrap();
    /// }
    /// assert_eq!(controller.remove_sessions_owned_by(a), 2);
    /// assert_eq!(controller.remove_sessions_owned_by(a), 0);
    /// # }
    /// ```
    pub fn remove_sessions_owned_by(&self, owner: ConnectionId) -> usize {
        let mut sessions = self.sessions.write();
        let before = sessions.len();
        sessions.retain(|id, session| {
            if session.owner != owner {
                return true;
            }
            if let Some(abort_handle) = &session.stream_task {
                abort_handle.abort();
            }
            debug!("Removed session {} of closed connection {}", id, owner);
            false
        });
        before - sessions.len()
    }

    /// Clean up expired sessions
    pub async fn cleanup_expired_sessions(&self, max_age: Duration) {
        let mut sessions = self.sessions.write();
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    async fn new_session(
        controller: &AdaptiveStreamController,
        owner: ConnectionId,
        data: Value,
    ) -> SessionId {
        let session_id = SessionId::new();
        controller
            .create_session(session_id, owner, data, StreamOptions::default())
            .await
            .unwrap();
        session_id
    }

    #[tokio::test]
    async fn test_create_session() {
        let controller = AdaptiveStreamController::new();
        let owner = ConnectionId::new();
        let data = json!({
            "critical": {"id": 1, "status": "active"},
            "details": {"name": "test", "description": "test data"}
        });

        let session_id = new_session(&controller, owner, data).await;

        let sessions = controller.sessions.read();
        assert_eq!(sessions[&session_id].owner(), owner);
    }

    #[tokio::test]
    async fn test_create_session_rejects_duplicate_id_and_keeps_first() {
        let controller = AdaptiveStreamController::new();
        let first_owner = ConnectionId::new();
        let session_id = new_session(&controller, first_owner, json!({"v": 1})).await;

        let result = controller
            .create_session(
                session_id,
                ConnectionId::new(),
                json!({"v": 2}),
                StreamOptions::default(),
            )
            .await;

        assert_eq!(result, Err(CreateSessionError::AlreadyExists(session_id)));
        let sessions = controller.sessions.read();
        assert_eq!(sessions[&session_id].owner(), first_owner);
        assert_eq!(sessions[&session_id].plan[0].data, json!({"v": 1}));
    }

    #[tokio::test]
    async fn test_frame_acknowledgment() {
        let controller = AdaptiveStreamController::new();
        let owner = ConnectionId::new();
        let session_id = new_session(&controller, owner, json!({"test": "data"})).await;

        controller
            .handle_frame_ack(owner, session_id, 0, 50)
            .await
            .unwrap();

        let sessions = controller.sessions.read();
        let session = sessions.get(&session_id).unwrap();
        assert_eq!(session.client_metrics.frames_acknowledged, 1);
        assert_eq!(session.client_metrics.average_processing_time_ms, 50.0);
    }

    #[tokio::test]
    async fn test_frame_ack_from_non_owner_is_rejected_and_metrics_unchanged() {
        let controller = AdaptiveStreamController::new();
        let owner = ConnectionId::new();
        let session_id = new_session(&controller, owner, json!({})).await;

        let result = controller
            .handle_frame_ack(ConnectionId::new(), session_id, 0, 50)
            .await;

        assert!(matches!(result, Err(PjsError::InvalidSession(_))));
        let sessions = controller.sessions.read();
        let session = &sessions[&session_id];
        assert_eq!(session.client_metrics.frames_acknowledged, 0);
    }

    #[tokio::test]
    async fn test_remove_sessions_owned_by_removes_only_that_owners_sessions() {
        let controller = AdaptiveStreamController::new();
        let (a, b) = (ConnectionId::new(), ConnectionId::new());
        let a1 = new_session(&controller, a, json!({})).await;
        let a2 = new_session(&controller, a, json!({})).await;
        let b1 = new_session(&controller, b, json!({})).await;

        assert_eq!(controller.remove_sessions_owned_by(a), 2);

        let sessions = controller.sessions.read();
        assert!(!sessions.contains_key(&a1) && !sessions.contains_key(&a2));
        assert!(sessions.contains_key(&b1));
    }

    #[tokio::test]
    async fn test_broadcast_tags_frames_with_owner() {
        let controller = AdaptiveStreamController::new();
        let owner = ConnectionId::new();
        let session_id = new_session(&controller, owner, json!({"k": "v"})).await;
        let mut frames_rx = controller.subscribe_frames();

        controller.start_streaming(session_id).await.unwrap();

        let (tagged_owner, tagged_session, message) =
            tokio::time::timeout(Duration::from_secs(2), frames_rx.recv())
                .await
                .expect("a frame must be broadcast")
                .unwrap();
        assert_eq!(tagged_owner, owner);
        assert_eq!(tagged_session, session_id);
        assert!(matches!(*message, WsMessage::StreamFrame { .. }));
    }

    /// Proves `remove_session` actually stops the streaming task rather
    /// than merely dropping the session's bookkeeping entry: a long plan
    /// is aborted before it can send every frame or the completion
    /// message, instead of running to completion in the background.
    #[tokio::test]
    async fn test_remove_session_aborts_streaming_task_before_completion() {
        let controller = AdaptiveStreamController::new();
        let session_id =
            new_session(&controller, ConnectionId::new(), json!({"test": "data"})).await;

        // Long enough (10ms/frame) that the task is still far from done
        // when we abort it immediately below.
        {
            let mut sessions = controller.sessions.write();
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

        controller.start_streaming(session_id).await.unwrap();
        assert!(controller.remove_session(session_id).await);

        // Drain whatever the task managed to emit before the abort took
        // effect, for a window far shorter than the full 200-frame plan
        // (~2s) would take to complete on its own.
        let mut saw_complete = false;
        let mut frame_count = 0;
        let drain_deadline = tokio::time::Instant::now() + Duration::from_millis(300);
        while tokio::time::Instant::now() < drain_deadline {
            match tokio::time::timeout(Duration::from_millis(20), frames_rx.recv()).await {
                Ok(Ok((_, _, message))) if matches!(*message, WsMessage::StreamComplete { .. }) => {
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
        let session_id =
            new_session(&controller, ConnectionId::new(), json!({"test": "data"})).await;

        {
            let mut sessions = controller.sessions.write();
            sessions
                .get_mut(&session_id)
                .unwrap()
                .client_metrics
                .update_processing_time(100_000_000_000);
        }

        let mut frames_rx = controller.subscribe_frames();
        controller.start_streaming(session_id).await.unwrap();

        let result = tokio::time::timeout(MAX_ADAPTIVE_FRAME_DELAY * 2, async {
            loop {
                let (_, sid, message) = frames_rx
                    .recv()
                    .await
                    .expect("channel must not close early");
                if sid == session_id && matches!(*message, WsMessage::StreamComplete { .. }) {
                    break;
                }
            }
        })
        .await;

        assert!(
            result.is_ok(),
            "stream must complete within 2x MAX_ADAPTIVE_FRAME_DELAY, not stall on malicious client metrics"
        );
    }
}
