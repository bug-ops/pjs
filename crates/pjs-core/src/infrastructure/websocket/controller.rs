//! Transport-local session state and the adaptive streaming controller.
//!
//! Owns the sessions a WebSocket server transport streams to its connections
//! and paces their frames from the acknowledgments clients send back.

use super::{
    planner::{self, MAX_PLAN_FRAMES, MAX_QUEUED_OUTGOING_BYTES, PlanOptions},
    protocol::{FixedError, StreamOptions, WsMessage},
};
use crate::{
    Error as PjsError, Result as PjsResult,
    domain::SessionId,
    error::{PlanError, StreamOptionsError},
    infrastructure::{bounded_channel::ByteBoundedSender, http::ShutdownSignal},
    security::RateLimitGuard,
};
use parking_lot::RwLock;
use serde_json::Value;
use std::{
    collections::HashMap,
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Identity of one accepted WebSocket connection.
///
/// Generated server-side and never sent on the wire. The controller keys every
/// session by the connection that created it, so a connection only ever
/// receives, acknowledges and cancels its own sessions.
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
/// [`SessionId`] identifiers, held per connection by
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
    /// Pre-serialized messages in delivery order; taken when streaming starts.
    pub plan: Vec<String>,
    /// Minimum pause between frames requested through `client_fps`.
    pub frame_interval: Option<Duration>,
    /// Bytes of the plan not yet handed to the connection's channel; counted
    /// in the owner's `live_plan_bytes` and released on every removal path.
    pub unsent_plan_bytes: usize,
    /// Adaptive streaming metrics derived from client acks.
    pub client_metrics: ClientMetrics,
    /// Rate-limit guard scoped to this session, if installed.
    pub rate_limit_guard: Option<RateLimitGuard>,
    /// Distinguishes this session from an earlier one that used the same id
    /// on the same connection, so a finished task never removes its
    /// successor.
    generation: u64,
    /// Handle to abort the per-session frame-streaming task on teardown.
    ///
    /// Not `pub`: only ever set and read within this module, which keeps
    /// this session-lifecycle detail out of the public struct-literal
    /// surface.
    stream_task: Option<tokio::task::AbortHandle>,
}

impl WebSocketStreamSession {
    fn abort_task(&self) {
        if let Some(abort_handle) = &self.stream_task {
            abort_handle.abort();
        }
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

/// Longest a single frame may wait for room in the connection's channel
/// before the session is dropped with a `408`.
///
/// Measured per enqueue, so many sessions fanning into one channel share the
/// wait: a link that drains slower than the live sessions' combined frames
/// need within this window can see false positives while still progressing.
/// The connection writer's write timeout is what detects a dead peer.
const STREAM_STALL_TIMEOUT: Duration = Duration::from_secs(30);

/// Fixed part of a stream's absolute deadline.
const STREAM_GRACE: Duration = Duration::from_secs(120);

/// Slowest average delivery rate (bytes per second) a stream may sustain
/// beyond [`STREAM_GRACE`]; larger plans get proportionally longer.
const MIN_STREAM_THROUGHPUT: usize = 64 * 1024;

/// Most time a plan's inter-frame pacing may add in total.
const PACING_BUDGET: Duration = Duration::from_secs(30);

fn stream_deadline(plan_bytes: usize) -> Duration {
    STREAM_GRACE + Duration::from_secs((plan_bytes / MIN_STREAM_THROUGHPUT) as u64)
}

/// Delay after a frame: the adaptive delay raised to the `client_fps` floor,
/// but never so long that the whole plan paces for more than
/// [`PACING_BUDGET`].
fn paced_delay(adaptive: Duration, floor: Option<Duration>, frames: usize) -> Duration {
    let frames = u32::try_from(frames.max(1)).unwrap_or(u32::MAX);
    adaptive
        .max(floor.unwrap_or_default())
        .min(PACING_BUDGET / frames)
}

/// Default cap on live streaming sessions per connection.
///
/// A session counts from creation until its stream completes, it is
/// cancelled, or the connection closes.
///
/// Defence in depth: the default inbound message rate limit (30 per second
/// per IP, burst 5) stops a burst short of this cap, but a paced client
/// holding sessions open for more than about 2 s reaches it, since shorter
/// sessions complete first.
pub(super) const DEFAULT_MAX_SESSIONS_PER_CONNECTION: NonZeroUsize = NonZeroUsize::new(64).unwrap();

/// Default for [`AdaptiveStreamController::with_connection_plan_budget`]: the
/// outgoing channel's own byte budget.
pub(super) const DEFAULT_CONNECTION_PLAN_BUDGET: NonZeroUsize =
    NonZeroUsize::new(MAX_QUEUED_OUTGOING_BYTES).unwrap();

/// The connection plan budget for a server whose inbound frames may be
/// `inbound_frame_size` bytes: large enough that the biggest plan one inbound
/// message can produce always fits a connection holding no other plan.
pub(super) fn connection_plan_budget_for(inbound_frame_size: usize) -> NonZeroUsize {
    let largest_plan = inbound_frame_size
        .saturating_mul(2)
        .saturating_add(MAX_PLAN_FRAMES * planner::ENVELOPE_RESERVE);
    NonZeroUsize::new(largest_plan.max(MAX_QUEUED_OUTGOING_BYTES))
        .unwrap_or(DEFAULT_CONNECTION_PLAN_BUDGET)
}

/// Why [`AdaptiveStreamController::create_session`] refused a session.
///
/// The enum is `#[non_exhaustive]`, so a match outside the crate needs a
/// wildcard arm:
///
/// ```compile_fail
/// use pjson_rs::infrastructure::websocket::CreateSessionError;
///
/// fn describe(error: CreateSessionError) -> &'static str {
///     match error {
///         CreateSessionError::AlreadyExists(_) => "exists",
///         CreateSessionError::TooManySessions { .. } => "cap",
///         CreateSessionError::InvalidOptions(_) => "options",
///         CreateSessionError::PlanRejected(_) => "plan",
///     }
/// }
/// ```
///
/// ```
/// use pjson_rs::infrastructure::websocket::CreateSessionError;
///
/// fn describe(error: CreateSessionError) -> &'static str {
///     match error {
///         CreateSessionError::AlreadyExists(_) => "exists",
///         CreateSessionError::TooManySessions { .. } => "cap",
///         CreateSessionError::InvalidOptions(_) => "options",
///         CreateSessionError::PlanRejected(_) => "plan",
///         _ => "other",
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CreateSessionError {
    /// The connection already has a live session with this id; it was left
    /// untouched. The same id on another connection is unrelated.
    #[error("session {0} already exists")]
    AlreadyExists(SessionId),
    /// The connection already has the maximum number of live sessions.
    #[error("connection already has {limit} live sessions")]
    TooManySessions {
        /// The per-connection cap that was reached.
        limit: NonZeroUsize,
    },
    /// The stream options failed validation.
    #[error("invalid stream options")]
    InvalidOptions(#[source] StreamOptionsError),
    /// The planned frames were rejected.
    #[error("stream plan rejected")]
    PlanRejected(#[source] PlanError),
}

impl From<CreateSessionError> for PjsError {
    fn from(error: CreateSessionError) -> Self {
        match error {
            CreateSessionError::AlreadyExists(id) => PjsError::SessionAlreadyExists(id),
            CreateSessionError::TooManySessions { limit } => PjsError::TooManySessions { limit },
            CreateSessionError::InvalidOptions(e) => PjsError::InvalidStreamOptions(e),
            CreateSessionError::PlanRejected(e) => PjsError::PlanRejected(e),
        }
    }
}

/// One connection's outgoing channel and its live sessions.
#[derive(Default)]
struct ConnectionEntry {
    outgoing: Option<ByteBoundedSender<String>>,
    sessions: HashMap<SessionId, WebSocketStreamSession>,
    /// Sum of the live sessions' `unsent_plan_bytes`.
    live_plan_bytes: usize,
}

impl ConnectionEntry {
    fn is_idle(&self) -> bool {
        self.outgoing.is_none() && self.sessions.is_empty()
    }

    /// Removes a session, releasing whatever part of its plan was never sent.
    fn remove_session(&mut self, session_id: SessionId) -> Option<WebSocketStreamSession> {
        let session = self.sessions.remove(&session_id)?;
        self.live_plan_bytes = self
            .live_plan_bytes
            .saturating_sub(session.unsent_plan_bytes);
        Some(session)
    }

    fn abort_all(&mut self) -> usize {
        let count = self.sessions.len();
        for (_, session) in self.sessions.drain() {
            session.abort_task();
        }
        self.live_plan_bytes = 0;
        count
    }
}

type Connections = Arc<RwLock<HashMap<ConnectionId, ConnectionEntry>>>;

/// Adaptive streaming controller.
///
/// Owns the transport-local sessions, grouped by the [`ConnectionId`] that
/// created them, and each connection's outgoing channel. A session task
/// awaits capacity on its own connection's channel, so a connection that
/// stops reading stalls only its own sessions. Acknowledgments and
/// cancellations are looked up under the caller's connection, which makes a
/// session id private to the connection that opened it.
pub struct AdaptiveStreamController {
    connections: Connections,
    next_generation: AtomicU64,
    max_sessions_per_connection: NonZeroUsize,
    connection_plan_budget: NonZeroUsize,
    shutdown: Option<ShutdownSignal>,
}

/// Why a session's stream ended before completing.
enum StreamAbort {
    Undeliverable,
    Stalled,
}

impl StreamAbort {
    fn reply(&self) -> FixedError {
        match self {
            Self::Undeliverable => FixedError::FRAME_EXCEEDS_BUDGET,
            Self::Stalled => FixedError::STREAM_STALLED,
        }
    }
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

/// Outcome of queueing one message on a connection's channel.
enum Enqueued {
    Sent,
    /// The receiver is gone or the message can never fit the channel's budget.
    Closed,
    /// No room appeared within [`STREAM_STALL_TIMEOUT`].
    Stalled,
}

/// Queues `text` on the owner's channel, waiting up to
/// [`STREAM_STALL_TIMEOUT`] for capacity.
async fn enqueue(outgoing: Option<&ByteBoundedSender<String>>, text: String) -> Enqueued {
    let Some(outgoing) = outgoing else {
        return Enqueued::Sent;
    };
    let len = text.len();
    match tokio::time::timeout(STREAM_STALL_TIMEOUT, outgoing.send(text, len)).await {
        Ok(Ok(())) => Enqueued::Sent,
        Ok(Err(_)) => Enqueued::Closed,
        Err(_elapsed) => Enqueued::Stalled,
    }
}

/// Drops the connection's entry when it has neither a channel nor sessions.
fn prune_if_idle(connections: &mut HashMap<ConnectionId, ConnectionEntry>, owner: ConnectionId) {
    if connections
        .get(&owner)
        .is_some_and(ConnectionEntry::is_idle)
    {
        connections.remove(&owner);
    }
}

impl AdaptiveStreamController {
    /// Create an empty controller with no active sessions.
    pub fn new() -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
            next_generation: AtomicU64::new(0),
            max_sessions_per_connection: DEFAULT_MAX_SESSIONS_PER_CONNECTION,
            connection_plan_budget: DEFAULT_CONNECTION_PLAN_BUDGET,
            shutdown: None,
        }
    }

    /// Caps the bytes of not-yet-sent plans one connection may hold across its
    /// live sessions (default [`DEFAULT_CONNECTION_PLAN_BUDGET`]).
    #[must_use]
    pub(super) fn with_connection_plan_budget(mut self, budget: NonZeroUsize) -> Self {
        self.connection_plan_budget = budget;
        self
    }

    /// Spawns every session task on `signal`'s tracker, so a shutdown drain
    /// waits for them and can force-drop them.
    #[must_use]
    pub(super) fn with_shutdown(mut self, signal: ShutdownSignal) -> Self {
        self.shutdown = Some(signal);
        self
    }

    /// Caps the live sessions a single connection may hold (default 64).
    #[must_use]
    pub(super) fn with_max_sessions_per_connection(mut self, limit: NonZeroUsize) -> Self {
        self.max_sessions_per_connection = limit;
        self
    }

    /// Registers `owner`'s outgoing channel; its sessions' frames are queued
    /// on it.
    pub fn attach_connection(&self, owner: ConnectionId, outgoing: ByteBoundedSender<String>) {
        self.connections.write().entry(owner).or_default().outgoing = Some(outgoing);
    }

    /// Forgets `owner`'s channel and aborts every session it owns. Returns
    /// the number of sessions removed. Synchronous so a connection's drop
    /// guard can call it.
    pub(super) fn detach_connection(&self, owner: ConnectionId) -> usize {
        let removed = self.connections.write().remove(&owner);
        removed.map_or(0, |mut entry| entry.abort_all())
    }

    /// Number of connections with a registered outgoing channel.
    pub(super) fn connection_count(&self) -> usize {
        self.connections
            .read()
            .values()
            .filter(|entry| entry.outgoing.is_some())
            .count()
    }

    pub(super) fn outgoing_for(&self, owner: ConnectionId) -> Option<ByteBoundedSender<String>> {
        self.connections
            .read()
            .get(&owner)
            .and_then(|entry| entry.outgoing.clone())
    }

    /// Create a streaming session under `session_id`, owned by `owner`.
    ///
    /// Ids are scoped to the connection: two connections may use the same id.
    /// An existing session of `owner` is never overwritten.
    ///
    /// The document is split into messages according to `options`
    /// (see [`StreamOptions`]); planning runs inline and costs O(payload).
    ///
    /// # Errors
    ///
    /// Returns [`CreateSessionError::InvalidOptions`] if `options` fail
    /// validation, [`CreateSessionError::PlanRejected`] if the document cannot
    /// be planned within the limits or `owner`'s plan budget,
    /// [`CreateSessionError::AlreadyExists`] if `owner` already has a
    /// live session with `session_id`, and
    /// [`CreateSessionError::TooManySessions`] if `owner` is at its cap.
    /// A rejected request leaves no session behind.
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
    /// // The id is taken on this connection ...
    /// assert!(controller
    ///     .create_session(id, owner, json!({}), StreamOptions::default())
    ///     .await
    ///     .is_err());
    /// // ... but another connection may use it.
    /// assert!(controller
    ///     .create_session(id, ConnectionId::new(), json!({}), StreamOptions::default())
    ///     .await
    ///     .is_ok());
    /// # }
    /// ```
    pub async fn create_session(
        &self,
        session_id: SessionId,
        owner: ConnectionId,
        data: Value,
        options: StreamOptions,
    ) -> Result<(), CreateSessionError> {
        let plan_options =
            PlanOptions::try_from(&options).map_err(CreateSessionError::InvalidOptions)?;
        // Cheap rejections first: planning is O(payload) and would be wasted on
        // a duplicate id or a full connection. The connection budget needs the
        // plan size, so it is checked after planning; everything is re-checked
        // under the write lock below.
        self.check_slot(owner, session_id)?;
        let plan = planner::plan_stream(session_id, data, &plan_options)
            .map_err(CreateSessionError::PlanRejected)?;

        let session = WebSocketStreamSession {
            created_at: Instant::now(),
            plan: plan.messages,
            frame_interval: plan_options.frame_interval,
            unsent_plan_bytes: plan.total_bytes,
            client_metrics: ClientMetrics::default(),
            rate_limit_guard: None, // Will be set when connection is established
            generation: self.next_generation.fetch_add(1, Ordering::Relaxed),
            stream_task: None, // Set when streaming starts
        };

        let mut connections = self.connections.write();
        let entry = connections.entry(owner).or_default();
        let outcome = if entry.sessions.contains_key(&session_id) {
            Err(CreateSessionError::AlreadyExists(session_id))
        } else if entry.sessions.len() >= self.max_sessions_per_connection.get() {
            Err(CreateSessionError::TooManySessions {
                limit: self.max_sessions_per_connection,
            })
        } else if entry.live_plan_bytes.saturating_add(plan.total_bytes)
            > self.connection_plan_budget.get()
        {
            Err(CreateSessionError::PlanRejected(
                PlanError::ConnectionBudgetExhausted {
                    live: entry.live_plan_bytes,
                    requested: plan.total_bytes,
                    max: self.connection_plan_budget.get(),
                },
            ))
        } else {
            entry.live_plan_bytes += plan.total_bytes;
            entry.sessions.insert(session_id, session);
            Ok(())
        };
        if outcome.is_err() {
            prune_if_idle(&mut connections, owner);
        } else {
            info!("Created streaming session: {}", session_id);
        }
        outcome
    }

    /// Rejects a duplicate id or a connection at its session cap without
    /// touching any state.
    fn check_slot(
        &self,
        owner: ConnectionId,
        session_id: SessionId,
    ) -> Result<(), CreateSessionError> {
        let connections = self.connections.read();
        let Some(entry) = connections.get(&owner) else {
            return Ok(());
        };
        if entry.sessions.contains_key(&session_id) {
            Err(CreateSessionError::AlreadyExists(session_id))
        } else if entry.sessions.len() >= self.max_sessions_per_connection.get() {
            Err(CreateSessionError::TooManySessions {
                limit: self.max_sessions_per_connection,
            })
        } else {
            Ok(())
        }
    }

    /// Start streaming frames for `owner`'s session.
    ///
    /// A session streams once: calling this again for a session that is
    /// already streaming leaves the running stream untouched and returns
    /// `Ok(())`.
    ///
    /// # Errors
    ///
    /// Returns [`PjsError::UnknownSession`] if `owner` has no such session.
    pub async fn start_streaming(
        &self,
        owner: ConnectionId,
        session_id: SessionId,
    ) -> PjsResult<()> {
        let mut connections = self.connections.write();
        let entry = connections
            .get_mut(&owner)
            .ok_or(PjsError::UnknownSession(session_id))?;
        let outgoing = entry.outgoing.clone();
        let session = entry
            .sessions
            .get_mut(&session_id)
            .ok_or(PjsError::UnknownSession(session_id))?;

        if session.stream_task.is_some() {
            debug!("Session {} is already streaming", session_id);
            return Ok(());
        }

        let generation = session.generation;
        let plan = std::mem::take(&mut session.plan);
        let frame_interval = session.frame_interval;
        let connections_for_task = self.connections.clone();
        let handle = spawn_task(self.shutdown.as_ref(), async move {
            Self::stream_frames(
                owner,
                session_id,
                generation,
                plan,
                frame_interval,
                outgoing,
                connections_for_task,
            )
            .await;
        });

        // Keep an abort handle so the task can be cancelled on session teardown,
        // and supervise the join handle to surface panics that would otherwise
        // be silently swallowed by the runtime.
        session.stream_task = Some(handle.abort_handle());
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

    /// Streams `plan` to `outgoing` under the stall and absolute deadlines,
    /// then removes the session and announces completion.
    async fn stream_frames(
        owner: ConnectionId,
        session_id: SessionId,
        generation: u64,
        plan: Vec<String>,
        frame_interval: Option<Duration>,
        outgoing: Option<ByteBoundedSender<String>>,
        connections: Connections,
    ) {
        let deadline = stream_deadline(plan.iter().map(String::len).sum());
        let sent = tokio::time::timeout(
            deadline,
            Self::send_plan(
                owner,
                session_id,
                generation,
                plan,
                frame_interval,
                outgoing.as_ref(),
                &connections,
            ),
        )
        .await;
        match sent {
            Ok(Ok(())) => {
                // The session stops being live before its completion is
                // visible, so a peer that has seen StreamComplete can
                // immediately reuse the id or open another session without a
                // spurious 409 or 429.
                Self::remove_if_generation(&connections, owner, session_id, generation);
                if let Ok(complete) =
                    serde_json::to_string(&WsMessage::StreamComplete { session_id })
                {
                    enqueue(outgoing.as_ref(), complete).await;
                }
            }
            Ok(Err(abort)) => {
                Self::remove_if_generation(&connections, owner, session_id, generation);
                Self::report_dropped(outgoing.as_ref(), session_id, abort.reply());
            }
            Err(_elapsed) => {
                Self::remove_if_generation(&connections, owner, session_id, generation);
                Self::report_dropped(
                    outgoing.as_ref(),
                    session_id,
                    FixedError::STREAM_DEADLINE_EXCEEDED,
                );
            }
        }
    }

    async fn send_plan(
        owner: ConnectionId,
        session_id: SessionId,
        generation: u64,
        plan: Vec<String>,
        frame_interval: Option<Duration>,
        outgoing: Option<&ByteBoundedSender<String>>,
        connections: &Connections,
    ) -> Result<(), StreamAbort> {
        let frame_count = plan.len();
        for text in plan {
            let len = text.len();
            match enqueue(outgoing, text).await {
                Enqueued::Sent => {}
                Enqueued::Closed => return Err(StreamAbort::Undeliverable),
                Enqueued::Stalled => return Err(StreamAbort::Stalled),
            }
            let adaptive = Self::release_sent(connections, owner, session_id, generation, len);
            tokio::time::sleep(paced_delay(adaptive, frame_interval, frame_count)).await;
        }
        Ok(())
    }

    /// Releases `len` sent plan bytes of the session and returns the adaptive
    /// delay recommended for it.
    ///
    /// Takes the global write lock once per frame. Benchmarked at about
    /// 10 M lock operations per second under 8 or more contending tasks
    /// (`websocket_controller` bench), enough for roughly 3700 concurrently
    /// streaming sessions at the fastest pacing, so the lock is kept.
    fn release_sent(
        connections: &Connections,
        owner: ConnectionId,
        session_id: SessionId,
        generation: u64,
        len: usize,
    ) -> Duration {
        let mut connections = connections.write();
        let Some(entry) = connections.get_mut(&owner) else {
            return Duration::from_millis(10);
        };
        let Some(session) = entry
            .sessions
            .get_mut(&session_id)
            .filter(|session| session.generation == generation)
        else {
            return Duration::from_millis(10);
        };
        let released = len.min(session.unsent_plan_bytes);
        session.unsent_plan_bytes -= released;
        let delay = session.client_metrics.recommended_frame_delay();
        entry.live_plan_bytes = entry.live_plan_bytes.saturating_sub(released);
        delay
    }

    /// Tells the peer its stream was dropped, if its channel has room.
    ///
    /// Best effort: on a full channel nothing is sent, which is the usual case
    /// for a stall or a deadline.
    fn report_dropped(
        outgoing: Option<&ByteBoundedSender<String>>,
        session_id: SessionId,
        dropped: FixedError,
    ) {
        let Some(outgoing) = outgoing else {
            return;
        };
        let reply = WsMessage::Error {
            session_id: Some(session_id),
            error: dropped.reason.to_owned(),
            code: dropped.code,
        };
        let Ok(json) = serde_json::to_string(&reply) else {
            return;
        };
        let len = json.len();
        if outgoing.try_send(json, len).is_err() {
            debug!("Outgoing channel full or closed; stopped session {session_id}");
        } else {
            warn!("Dropped session {session_id}: {}", dropped.reason);
        }
    }

    /// Removes the session only if it is still the one started by the caller.
    fn remove_if_generation(
        connections: &Connections,
        owner: ConnectionId,
        session_id: SessionId,
        generation: u64,
    ) {
        let mut connections = connections.write();
        if let Some(entry) = connections.get_mut(&owner)
            && entry
                .sessions
                .get(&session_id)
                .is_some_and(|session| session.generation == generation)
        {
            entry.remove_session(session_id);
        }
        prune_if_idle(&mut connections, owner);
    }

    fn with_session<R>(
        &self,
        owner: ConnectionId,
        session_id: SessionId,
        f: impl FnOnce(&mut WebSocketStreamSession) -> R,
    ) -> PjsResult<R> {
        self.connections
            .write()
            .get_mut(&owner)
            .and_then(|entry| entry.sessions.get_mut(&session_id))
            .map(f)
            .ok_or(PjsError::UnknownSession(session_id))
    }

    /// Handle frame acknowledgment from `owner`.
    ///
    /// # Errors
    ///
    /// Returns [`PjsError::UnknownSession`] if `owner` has no such live
    /// session; nothing is changed.
    pub async fn handle_frame_ack(
        &self,
        owner: ConnectionId,
        session_id: SessionId,
        frame_id: u32,
        processing_time_ms: u64,
    ) -> PjsResult<()> {
        self.with_session(owner, session_id, |session| {
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
        })
    }

    /// Set rate limit guard for a session
    pub async fn set_rate_limit_guard(
        &self,
        owner: ConnectionId,
        session_id: SessionId,
        guard: RateLimitGuard,
    ) -> PjsResult<()> {
        self.with_session(owner, session_id, |session| {
            session.rate_limit_guard = Some(guard);
        })
    }

    /// Validate message against rate limits
    pub async fn validate_message(
        &self,
        owner: ConnectionId,
        session_id: SessionId,
        frame_size: usize,
    ) -> PjsResult<()> {
        let connections = self.connections.read();
        let session = connections
            .get(&owner)
            .and_then(|entry| entry.sessions.get(&session_id))
            .ok_or(PjsError::UnknownSession(session_id))?;

        if let Some(guard) = &session.rate_limit_guard {
            guard.check_message(frame_size)?;
        }

        Ok(())
    }

    /// Remove `owner`'s session by id, aborting its streaming task.
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
    /// let owner = ConnectionId::new();
    /// controller
    ///     .create_session(id, owner, json!({}), StreamOptions::default())
    ///     .await
    ///     .unwrap();
    /// assert!(controller.remove_session(owner, id).await);
    /// // Idempotent — second call is a no-op:
    /// assert!(!controller.remove_session(owner, id).await);
    /// # }
    /// ```
    pub async fn remove_session(&self, owner: ConnectionId, session_id: SessionId) -> bool {
        let mut connections = self.connections.write();
        let removed = connections
            .get_mut(&owner)
            .and_then(|entry| entry.remove_session(session_id));
        prune_if_idle(&mut connections, owner);
        match &removed {
            Some(session) => {
                session.abort_task();
                info!("Removed streaming session: {}", session_id);
            }
            None => debug!("remove_session called on unknown id: {}", session_id),
        }
        removed.is_some()
    }

    /// Remove every session created by `owner`, aborting their streaming tasks.
    ///
    /// Returns the number of sessions removed; the connection's outgoing
    /// channel stays registered.
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
        let mut connections = self.connections.write();
        let removed = connections
            .get_mut(&owner)
            .map_or(0, ConnectionEntry::abort_all);
        prune_if_idle(&mut connections, owner);
        removed
    }

    /// Clean up expired sessions
    pub async fn cleanup_expired_sessions(&self, max_age: Duration) {
        let mut connections = self.connections.write();
        let now = Instant::now();

        for entry in connections.values_mut() {
            let expired: Vec<SessionId> = entry
                .sessions
                .iter()
                .filter(|(_, session)| now.duration_since(session.created_at) > max_age)
                .map(|(id, _)| *id)
                .collect();
            for id in expired {
                if let Some(session) = entry.remove_session(id) {
                    session.abort_task();
                    info!("Cleaning up expired session: {}", id);
                }
            }
        }
        connections.retain(|_, entry| !entry.is_idle());
    }

    #[cfg(test)]
    pub(super) fn session_count(&self, owner: ConnectionId) -> usize {
        self.connections
            .read()
            .get(&owner)
            .map_or(0, |entry| entry.sessions.len())
    }

    #[cfg(test)]
    pub(super) fn with_session_mut<R>(
        &self,
        owner: ConnectionId,
        session_id: SessionId,
        f: impl FnOnce(&mut WebSocketStreamSession) -> R,
    ) -> Option<R> {
        self.with_session(owner, session_id, f).ok()
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
    use crate::domain::Priority;
    use crate::infrastructure::bounded_channel::{Envelope, byte_bounded_channel};
    use serde_json::json;
    use tokio::sync::mpsc;

    type Outbox = mpsc::Receiver<Envelope<String>>;

    fn attach(controller: &AdaptiveStreamController, owner: ConnectionId) -> Outbox {
        attach_with_capacity(controller, owner, 1000)
    }

    fn attach_with_capacity(
        controller: &AdaptiveStreamController,
        owner: ConnectionId,
        capacity: usize,
    ) -> Outbox {
        let (tx, rx) = byte_bounded_channel::<String>(capacity, 16 * 1024 * 1024);
        controller.attach_connection(owner, tx);
        rx
    }

    async fn next_message(outbox: &mut Outbox) -> WsMessage {
        let envelope = tokio::time::timeout(Duration::from_secs(2), outbox.recv())
            .await
            .expect("a message must arrive")
            .expect("channel open");
        serde_json::from_str(&envelope.into_inner()).unwrap()
    }

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

    fn poison_metrics(controller: &AdaptiveStreamController, owner: ConnectionId, id: SessionId) {
        controller
            .with_session_mut(owner, id, |session| {
                session
                    .client_metrics
                    .update_processing_time(100_000_000_000);
            })
            .unwrap();
    }

    #[tokio::test]
    async fn test_create_session() {
        let controller = AdaptiveStreamController::new();
        let owner = ConnectionId::new();
        new_session(&controller, owner, json!({"k": 1})).await;
        assert_eq!(controller.session_count(owner), 1);
    }

    #[tokio::test]
    async fn test_create_session_rejects_duplicate_id_on_same_connection_and_keeps_first() {
        let controller = AdaptiveStreamController::new();
        let owner = ConnectionId::new();
        let session_id = new_session(&controller, owner, json!({"v": 1})).await;

        let result = controller
            .create_session(session_id, owner, json!({"v": 2}), StreamOptions::default())
            .await;

        assert_eq!(result, Err(CreateSessionError::AlreadyExists(session_id)));
        let first_frame = controller
            .with_session_mut(owner, session_id, |s| {
                serde_json::from_str::<Value>(&s.plan[0]).unwrap()
            })
            .unwrap();
        assert_eq!(first_frame["data"]["payload"], json!({"v": 1}));
    }

    #[tokio::test]
    async fn test_same_session_id_on_two_connections_is_independent() {
        let controller = AdaptiveStreamController::new();
        let (a, b) = (ConnectionId::new(), ConnectionId::new());
        let session_id = new_session(&controller, a, json!({})).await;

        controller
            .create_session(session_id, b, json!({}), StreamOptions::default())
            .await
            .unwrap();

        assert_eq!(controller.session_count(a), 1);
        assert_eq!(controller.session_count(b), 1);
    }

    #[tokio::test]
    async fn test_live_session_cap_rejects_the_session_over_the_limit() {
        let limit = NonZeroUsize::new(3).unwrap();
        let controller = AdaptiveStreamController::new().with_max_sessions_per_connection(limit);
        let owner = ConnectionId::new();
        for _ in 0..3 {
            new_session(&controller, owner, json!({})).await;
        }

        let rejected = SessionId::new();
        let result = controller
            .create_session(rejected, owner, json!({}), StreamOptions::default())
            .await;

        assert_eq!(result, Err(CreateSessionError::TooManySessions { limit }));
        assert_eq!(controller.session_count(owner), 3);
        let other = ConnectionId::new();
        new_session(&controller, other, json!({})).await;
    }

    #[tokio::test]
    async fn test_completed_sessions_do_not_count_toward_the_cap() {
        let controller = AdaptiveStreamController::new()
            .with_max_sessions_per_connection(NonZeroUsize::new(1).unwrap());
        let owner = ConnectionId::new();
        let mut outbox = attach(&controller, owner);

        for _ in 0..3 {
            let id = new_session(&controller, owner, json!({})).await;
            controller.start_streaming(owner, id).await.unwrap();
            assert!(matches!(
                next_message(&mut outbox).await,
                WsMessage::StreamFrame { .. }
            ));
            assert!(matches!(
                next_message(&mut outbox).await,
                WsMessage::StreamComplete { .. }
            ));
        }
    }

    #[tokio::test]
    async fn test_session_is_removed_before_stream_complete_is_visible() {
        let controller = AdaptiveStreamController::new();
        let owner = ConnectionId::new();
        let mut outbox = attach(&controller, owner);
        let id = new_session(&controller, owner, json!({})).await;
        controller.start_streaming(owner, id).await.unwrap();

        loop {
            if matches!(
                next_message(&mut outbox).await,
                WsMessage::StreamComplete { .. }
            ) {
                break;
            }
        }

        assert_eq!(controller.session_count(owner), 0);
        controller
            .create_session(id, owner, json!({}), StreamOptions::default())
            .await
            .expect("the id is reusable as soon as StreamComplete is seen");
    }

    #[tokio::test]
    async fn test_finished_task_never_removes_a_newer_session_with_the_same_id() {
        let controller = AdaptiveStreamController::new();
        let owner = ConnectionId::new();
        let id = new_session(&controller, owner, json!({})).await;
        let old_generation = controller
            .with_session_mut(owner, id, |s| s.generation)
            .unwrap();
        assert!(controller.remove_session(owner, id).await);
        new_session_with_id(&controller, owner, id).await;
        let new_generation = controller
            .with_session_mut(owner, id, |s| s.generation)
            .unwrap();
        assert_ne!(old_generation, new_generation);

        AdaptiveStreamController::remove_if_generation(
            &controller.connections,
            owner,
            id,
            old_generation,
        );
        assert_eq!(
            controller.session_count(owner),
            1,
            "stale removal must be a no-op"
        );

        AdaptiveStreamController::remove_if_generation(
            &controller.connections,
            owner,
            id,
            new_generation,
        );
        assert_eq!(controller.session_count(owner), 0);
    }

    async fn new_session_with_id(
        controller: &AdaptiveStreamController,
        owner: ConnectionId,
        id: SessionId,
    ) {
        controller
            .create_session(id, owner, json!({}), StreamOptions::default())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_cancel_then_reinit_while_old_task_sleeps_keeps_the_new_session() {
        let controller = AdaptiveStreamController::new();
        let owner = ConnectionId::new();
        let mut outbox = attach(&controller, owner);
        let id = new_session(&controller, owner, json!({})).await;
        poison_metrics(&controller, owner, id);
        controller.start_streaming(owner, id).await.unwrap();
        assert!(matches!(
            next_message(&mut outbox).await,
            WsMessage::StreamFrame { .. }
        ));

        assert!(controller.remove_session(owner, id).await);
        new_session_with_id(&controller, owner, id).await;
        poison_metrics(&controller, owner, id);
        controller.start_streaming(owner, id).await.unwrap();

        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(controller.session_count(owner), 1);
        assert!(matches!(
            next_message(&mut outbox).await,
            WsMessage::StreamFrame { .. }
        ));
    }

    #[tokio::test]
    async fn test_cancel_stops_a_delayed_session_before_stream_complete() {
        let controller = AdaptiveStreamController::new();
        let owner = ConnectionId::new();
        let mut outbox = attach(&controller, owner);
        let id = new_session(&controller, owner, json!({})).await;
        poison_metrics(&controller, owner, id);
        controller.start_streaming(owner, id).await.unwrap();
        assert!(matches!(
            next_message(&mut outbox).await,
            WsMessage::StreamFrame { .. }
        ));

        assert!(controller.remove_session(owner, id).await);

        let late = tokio::time::timeout(Duration::from_millis(1500), outbox.recv()).await;
        assert!(
            late.is_err(),
            "no StreamComplete may follow a cancel: {late:?}"
        );
    }

    #[tokio::test]
    async fn test_frames_reach_only_the_owning_connection() {
        let controller = AdaptiveStreamController::new();
        let (a, b) = (ConnectionId::new(), ConnectionId::new());
        let mut a_outbox = attach(&controller, a);
        let mut b_outbox = attach(&controller, b);
        let id = new_session(&controller, a, json!({"k": "v"})).await;

        controller.start_streaming(a, id).await.unwrap();

        assert!(matches!(
            next_message(&mut a_outbox).await,
            WsMessage::StreamFrame { .. }
        ));
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(b_outbox.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_a_connection_that_never_reads_does_not_starve_another() {
        let controller = AdaptiveStreamController::new();
        let (stalled, healthy) = (ConnectionId::new(), ConnectionId::new());
        let _stalled_outbox = attach_with_capacity(&controller, stalled, 1);
        let mut healthy_outbox = attach(&controller, healthy);
        for _ in 0..5 {
            let id = new_session(&controller, stalled, json!({})).await;
            controller.start_streaming(stalled, id).await.unwrap();
        }
        let id = new_session(&controller, healthy, json!({})).await;

        controller.start_streaming(healthy, id).await.unwrap();

        assert!(matches!(
            next_message(&mut healthy_outbox).await,
            WsMessage::StreamFrame { .. }
        ));
        assert!(matches!(
            next_message(&mut healthy_outbox).await,
            WsMessage::StreamComplete { .. }
        ));
    }

    #[tokio::test]
    async fn test_frame_over_the_byte_budget_ends_the_session_with_a_413() {
        let controller = AdaptiveStreamController::new();
        let owner = ConnectionId::new();
        let (tx, mut outbox) = byte_bounded_channel::<String>(8, 4096);
        controller.attach_connection(owner, tx);
        let id = new_session(&controller, owner, json!({"blob": "x".repeat(8192)})).await;

        controller.start_streaming(owner, id).await.unwrap();

        assert!(matches!(
            next_message(&mut outbox).await,
            WsMessage::Error { code: 413, session_id: Some(reported), .. } if reported == id
        ));
        assert_eq!(controller.session_count(owner), 0);
    }

    #[tokio::test]
    async fn test_start_streaming_twice_leaves_the_running_stream_untouched() {
        let controller = AdaptiveStreamController::new();
        let owner = ConnectionId::new();
        let mut outbox = attach(&controller, owner);
        let id = new_session(&controller, owner, json!({})).await;

        controller.start_streaming(owner, id).await.unwrap();
        controller.start_streaming(owner, id).await.unwrap();

        let (mut frames, mut completes) = (0, 0);
        while let Ok(Some(envelope)) =
            tokio::time::timeout(Duration::from_millis(300), outbox.recv()).await
        {
            match serde_json::from_str::<WsMessage>(&envelope.into_inner()) {
                Ok(WsMessage::StreamFrame { .. }) => frames += 1,
                Ok(WsMessage::StreamComplete { .. }) => completes += 1,
                other => panic!("unexpected message: {other:?}"),
            }
        }
        assert_eq!((frames, completes), (1, 1));
        assert_eq!(controller.session_count(owner), 0);
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

        let (acked, average) = controller
            .with_session_mut(owner, session_id, |s| {
                (
                    s.client_metrics.frames_acknowledged,
                    s.client_metrics.average_processing_time_ms,
                )
            })
            .unwrap();
        assert_eq!((acked, average), (1, 50.0));
    }

    #[tokio::test]
    async fn test_frame_ack_from_another_connection_is_rejected_and_metrics_unchanged() {
        let controller = AdaptiveStreamController::new();
        let owner = ConnectionId::new();
        let session_id = new_session(&controller, owner, json!({})).await;

        let result = controller
            .handle_frame_ack(ConnectionId::new(), session_id, 0, 50)
            .await;

        assert!(matches!(result, Err(PjsError::UnknownSession(_))));
        let acked = controller
            .with_session_mut(owner, session_id, |s| s.client_metrics.frames_acknowledged)
            .unwrap();
        assert_eq!(acked, 0);
    }

    #[tokio::test]
    async fn test_remove_sessions_owned_by_removes_only_that_owners_sessions() {
        let controller = AdaptiveStreamController::new();
        let (a, b) = (ConnectionId::new(), ConnectionId::new());
        new_session(&controller, a, json!({})).await;
        new_session(&controller, a, json!({})).await;
        new_session(&controller, b, json!({})).await;

        assert_eq!(controller.remove_sessions_owned_by(a), 2);

        assert_eq!(controller.session_count(a), 0);
        assert_eq!(controller.session_count(b), 1);
    }

    #[tokio::test]
    async fn test_detach_connection_removes_channel_and_sessions() {
        let controller = AdaptiveStreamController::new();
        let owner = ConnectionId::new();
        let _outbox = attach(&controller, owner);
        new_session(&controller, owner, json!({})).await;
        assert_eq!(controller.connection_count(), 1);

        assert_eq!(controller.detach_connection(owner), 1);

        assert_eq!(controller.connection_count(), 0);
        assert_eq!(controller.session_count(owner), 0);
    }

    /// Proves `remove_session` actually stops the streaming task rather
    /// than merely dropping the session's bookkeeping entry: a long plan
    /// is aborted before it can send every frame or the completion
    /// message, instead of running to completion in the background.
    #[tokio::test]
    async fn test_remove_session_aborts_streaming_task_before_completion() {
        let controller = AdaptiveStreamController::new();
        let owner = ConnectionId::new();
        let mut outbox = attach(&controller, owner);
        let session_id = new_session(&controller, owner, json!({"test": "data"})).await;

        // Long enough (10ms/frame) that the task is still far from done
        // when we abort it immediately below.
        controller
            .with_session_mut(owner, session_id, |session| {
                session.plan = (0..200)
                    .map(|frame_id| {
                        serde_json::to_string(&WsMessage::StreamFrame {
                            session_id,
                            frame_id,
                            priority: Priority::HIGH,
                            payload: json!({}),
                            is_complete: false,
                        })
                        .unwrap()
                    })
                    .collect();
            })
            .unwrap();

        controller.start_streaming(owner, session_id).await.unwrap();
        assert!(controller.remove_session(owner, session_id).await);

        // Drain whatever the task managed to emit before the abort took
        // effect, for a window far shorter than the full 200-frame plan
        // (~2s) would take to complete on its own.
        let mut saw_complete = false;
        let mut frame_count = 0;
        let drain_deadline = tokio::time::Instant::now() + Duration::from_millis(300);
        while tokio::time::Instant::now() < drain_deadline {
            match tokio::time::timeout(Duration::from_millis(20), outbox.recv()).await {
                Ok(Some(envelope)) => {
                    match serde_json::from_str::<WsMessage>(&envelope.into_inner()).unwrap() {
                        WsMessage::StreamComplete { .. } => {
                            saw_complete = true;
                            break;
                        }
                        _ => frame_count += 1,
                    }
                }
                Ok(None) => break,
                Err(_) => {}
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

    #[tokio::test(start_paused = true)]
    async fn test_stalled_stream_reports_408_when_the_channel_has_room_for_it() {
        let controller = AdaptiveStreamController::new();
        let owner = ConnectionId::new();
        let (tx, mut outbox) = byte_bounded_channel::<String>(1000, 2000);
        controller.attach_connection(owner, tx);
        let data = Value::Object(
            (0..3)
                .map(|i| (format!("k{i}"), json!("x".repeat(700))))
                .collect(),
        );
        let id = SessionId::new();
        let options = StreamOptions {
            max_frame_size: 1024,
            ..StreamOptions::default()
        };
        controller
            .create_session(id, owner, data, options)
            .await
            .unwrap();

        controller.start_streaming(owner, id).await.unwrap();
        tokio::time::sleep(STREAM_STALL_TIMEOUT + Duration::from_secs(1)).await;

        let mut kinds = Vec::new();
        while let Ok(envelope) = outbox.try_recv() {
            kinds.push(serde_json::from_str::<WsMessage>(&envelope.into_inner()).unwrap());
        }
        assert!(
            matches!(
                kinds.last(),
                Some(WsMessage::Error { code: 408, error, session_id: Some(reported) })
                    if error == FixedError::STREAM_STALLED.reason && *reported == id
            ),
            "{kinds:?}"
        );
        assert_eq!(controller.session_count(owner), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn test_stream_past_its_deadline_reports_408_when_the_channel_has_room_for_it() {
        let controller = AdaptiveStreamController::new();
        let owner = ConnectionId::new();
        let (tx, mut outbox) = byte_bounded_channel::<String>(1000, 2000);
        controller.attach_connection(owner, tx);
        let data = Value::Object(
            (0..8)
                .map(|i| (format!("k{i}"), json!("x".repeat(700))))
                .collect(),
        );
        let id = SessionId::new();
        let options = StreamOptions {
            max_frame_size: 1024,
            ..StreamOptions::default()
        };
        controller
            .create_session(id, owner, data, options)
            .await
            .unwrap();
        controller.start_streaming(owner, id).await.unwrap();

        // A reader that frees one frame every 25 s keeps every enqueue under
        // the 30 s stall timeout, so only the absolute deadline can end it.
        for _ in 0..4 {
            tokio::time::sleep(Duration::from_secs(25)).await;
            outbox.try_recv().expect("a frame is queued");
        }
        tokio::time::sleep(Duration::from_secs(21)).await;

        let mut messages = Vec::new();
        while let Ok(envelope) = outbox.try_recv() {
            messages.push(serde_json::from_str::<WsMessage>(&envelope.into_inner()).unwrap());
        }
        assert!(
            matches!(
                messages.last(),
                Some(WsMessage::Error { code: 408, error, session_id: Some(reported) })
                    if error == FixedError::STREAM_DEADLINE_EXCEEDED.reason && *reported == id
            ),
            "{messages:?}"
        );
        assert_eq!(controller.session_count(owner), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn test_duplicate_and_capped_requests_are_rejected_before_planning() {
        let controller =
            AdaptiveStreamController::new().with_max_sessions_per_connection(NonZeroUsize::MIN);
        let owner = ConnectionId::new();
        let id = new_session(&controller, owner, json!({"v": 1})).await;
        // A document that fails planning proves planning was skipped: the
        // slot errors win over `PathTooDeep`.
        let mut too_deep = json!(1);
        for _ in 0..200 {
            too_deep = json!([too_deep]);
        }

        let duplicate = controller
            .create_session(id, owner, too_deep.clone(), StreamOptions::default())
            .await;
        let capped = controller
            .create_session(SessionId::new(), owner, too_deep, StreamOptions::default())
            .await;

        assert_eq!(duplicate, Err(CreateSessionError::AlreadyExists(id)));
        assert!(matches!(
            capped,
            Err(CreateSessionError::TooManySessions { .. })
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn test_cancel_mid_stream_releases_unsent_plan_bytes() {
        use crate::infrastructure::websocket::Framing;

        let controller = AdaptiveStreamController::new()
            .with_connection_plan_budget(NonZeroUsize::new(40_000).unwrap());
        let owner = ConnectionId::new();
        let _outbox = attach_with_capacity(&controller, owner, 1);
        let data = json!({"blob": "x".repeat(30_000)});
        let options = StreamOptions {
            framing: Framing::Patch,
            max_frame_size: 2048,
            ..StreamOptions::default()
        };
        let first = SessionId::new();
        controller
            .create_session(first, owner, data.clone(), options.clone())
            .await
            .unwrap();
        assert!(matches!(
            controller
                .create_session(SessionId::new(), owner, data.clone(), options.clone())
                .await,
            Err(CreateSessionError::PlanRejected(
                PlanError::ConnectionBudgetExhausted { .. }
            ))
        ));

        controller.start_streaming(owner, first).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(controller.remove_session(owner, first).await);

        controller
            .create_session(SessionId::new(), owner, data, options)
            .await
            .expect("the cancelled session's unsent bytes are released");
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
        let owner = ConnectionId::new();
        let mut outbox = attach(&controller, owner);
        let session_id = new_session(&controller, owner, json!({"test": "data"})).await;
        poison_metrics(&controller, owner, session_id);

        controller.start_streaming(owner, session_id).await.unwrap();

        let result = tokio::time::timeout(MAX_ADAPTIVE_FRAME_DELAY * 2, async {
            loop {
                if matches!(
                    next_message(&mut outbox).await,
                    WsMessage::StreamComplete { .. }
                ) {
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
