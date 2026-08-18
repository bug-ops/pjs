//! StreamSession aggregate root managing multiple streams

use crate::domain::{
    DomainError, DomainResult,
    entities::{
        Frame, Stream,
        frame::FramePatch,
        stream::{StreamConfig, StreamState},
    },
    events::{DomainEvent, SessionState},
    ports::{SystemTimeProvider, TimeProvider},
    value_objects::{JsonData, Priority, SessionId, StreamId},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

/// Default [`TimeProvider`] used when a session is constructed without an
/// explicit one, and when a deserialized session needs one filled back in
/// (trait objects are not `Deserialize`, so the field is skipped on the wire).
fn default_time_provider() -> Arc<dyn TimeProvider> {
    Arc::new(SystemTimeProvider)
}

/// Custom serde for SessionId within aggregates
mod serde_session_id {
    use crate::domain::value_objects::SessionId;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(id: &SessionId, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        id.as_uuid().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<SessionId, D::Error>
    where
        D: Deserializer<'de>,
    {
        let uuid = uuid::Uuid::deserialize(deserializer)?;
        Ok(SessionId::from_uuid(uuid))
    }
}

/// Custom serde for HashMap<StreamId, Stream>
mod serde_stream_map {
    use crate::domain::{entities::Stream, value_objects::StreamId};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::HashMap;

    pub fn serialize<S>(map: &HashMap<StreamId, Stream>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let uuid_map: HashMap<String, &Stream> = map
            .iter()
            .map(|(k, v)| (k.as_uuid().to_string(), v))
            .collect();
        uuid_map.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<HashMap<StreamId, Stream>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let uuid_map: HashMap<String, Stream> = HashMap::deserialize(deserializer)?;
        uuid_map
            .into_iter()
            .map(|(k, v)| {
                uuid::Uuid::parse_str(&k)
                    .map(|uuid| (StreamId::from_uuid(uuid), v))
                    .map_err(serde::de::Error::custom)
            })
            .collect()
    }
}

/// Session configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    /// Maximum concurrent streams
    pub max_concurrent_streams: usize,
    /// Session timeout in seconds
    pub session_timeout_seconds: u64,
    /// Default stream configuration
    pub default_stream_config: StreamConfig,
    /// Enable session-level compression
    pub enable_compression: bool,
    /// Custom metadata
    pub metadata: HashMap<String, String>,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            max_concurrent_streams: 10,
            session_timeout_seconds: 3600, // 1 hour
            default_stream_config: StreamConfig::default(),
            enable_compression: true,
            metadata: HashMap::new(),
        }
    }
}

/// Session statistics and monitoring
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionStats {
    /// Total number of streams the session has hosted.
    pub total_streams: u64,
    /// Number of streams currently in an active state.
    pub active_streams: u64,
    /// Number of streams that completed successfully.
    pub completed_streams: u64,
    /// Number of streams that terminated with an error.
    pub failed_streams: u64,
    /// Total number of frames emitted by the session.
    pub total_frames: u64,
    /// Total estimated payload bytes emitted by the session, accumulated
    /// from [`Frame::estimated_size`] over every frame batch produced by
    /// [`StreamSession::create_stream_patch_frames`] and
    /// [`StreamSession::create_priority_frames`]. Counts bytes emitted over
    /// the wire, not distinct payload volume: patch generation is
    /// content-idempotent, so repeated polling against unchanged source data
    /// keeps adding the same bytes rather than counting them once.
    pub total_bytes: u64,
    /// Lifetime arithmetic mean of per-stream duration, in milliseconds,
    /// over streams that completed successfully (never updated by
    /// [`StreamSession::fail_stream`]). Each sample is a stream's
    /// `completed_at - created_at`, so it includes time spent queued/preparing
    /// before [`StreamSession::start_stream`], not just time spent actively
    /// streaming. Computed via Welford's running-mean formula, so a single
    /// slow or fast outlier from long ago carries the same weight as one from
    /// a moment ago — it never decays. For a recency-sensitive signal (e.g.
    /// health monitoring), use [`Self::recent_avg_duration_ms`] instead.
    pub average_stream_duration_ms: f64,
    /// Recency-weighted mean of per-stream duration, in milliseconds, over
    /// streams that completed successfully. Computed as a fixed-alpha
    /// (0.5) exponential moving average — `recent = 0.5 * duration + 0.5 *
    /// recent` — seeded with the first observed duration, so recent samples
    /// dominate and old ones decay away. This is what
    /// [`SessionHealthSnapshot`](crate::domain::ports::SessionHealthSnapshot)'s
    /// `recent_avg_duration_ms` metric reports; use
    /// [`Self::average_stream_duration_ms`] for the true lifetime average
    /// instead.
    pub recent_avg_duration_ms: f64,
}

/// StreamSession aggregate root - manages multiple prioritized streams
#[derive(Clone, Serialize, Deserialize)]
pub struct StreamSession {
    #[serde(with = "serde_session_id")]
    id: SessionId,
    state: SessionState,
    config: SessionConfig,
    stats: SessionStats,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,

    // Aggregate state
    #[serde(with = "serde_stream_map")]
    streams: HashMap<StreamId, Stream>,
    pending_events: VecDeque<DomainEvent>,

    // Session metadata
    client_info: Option<String>,
    user_agent: Option<String>,
    ip_address: Option<String>,

    /// Source of "now" for expiry, timeout, and event timestamps.
    ///
    /// Not part of the wire format: a `dyn TimeProvider` cannot be
    /// (de)serialized, so it is skipped and restored to [`SystemTimeProvider`]
    /// on deserialize — matching the pre-existing behavior where a
    /// deserialized session always used real system time.
    #[serde(skip, default = "default_time_provider")]
    time_provider: Arc<dyn TimeProvider>,
}

impl std::fmt::Debug for StreamSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamSession")
            .field("id", &self.id)
            .field("state", &self.state)
            .field("config", &self.config)
            .field("stats", &self.stats)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .field("expires_at", &self.expires_at)
            .field("completed_at", &self.completed_at)
            .field("streams", &self.streams)
            .field("pending_events", &self.pending_events)
            .field("client_info", &self.client_info)
            .field("user_agent", &self.user_agent)
            .field("ip_address", &self.ip_address)
            .finish_non_exhaustive()
    }
}

impl StreamSession {
    /// Create new session, using real system time for expiry and event timestamps.
    pub fn new(config: SessionConfig) -> Self {
        Self::with_time_provider(config, default_time_provider())
    }

    /// Create new session with an explicit [`TimeProvider`].
    ///
    /// Lets callers inject a fake clock so that *session-level* expiry,
    /// timeout, and timestamp-ordering logic (`created_at`, `updated_at`,
    /// `expires_at`, and event timestamps on `StreamSession` itself) can be
    /// tested deterministically instead of relying on real wall-clock
    /// delays. Child [`Stream`] entities are not clock-injected — they still
    /// timestamp themselves via the real system clock, so a session with an
    /// injected fake clock runs two independent time domains at once (e.g.
    /// `Stream::duration()` reflects real elapsed time even when the owning
    /// session's `updated_at` does not advance).
    ///
    /// # Examples
    ///
    /// ```
    /// use pjson_rs::domain::aggregates::stream_session::{SessionConfig, StreamSession};
    /// use pjson_rs::domain::ports::SystemTimeProvider;
    /// use std::sync::Arc;
    ///
    /// let session = StreamSession::with_time_provider(
    ///     SessionConfig::default(),
    ///     Arc::new(SystemTimeProvider),
    /// );
    /// assert!(!session.is_expired());
    /// ```
    pub fn with_time_provider(config: SessionConfig, time_provider: Arc<dyn TimeProvider>) -> Self {
        let now = time_provider.now();
        let expires_at = now + chrono::Duration::seconds(config.session_timeout_seconds as i64);

        Self {
            id: SessionId::new(),
            state: SessionState::Initializing,
            config,
            stats: SessionStats::default(),
            created_at: now,
            updated_at: now,
            expires_at,
            completed_at: None,
            streams: HashMap::new(),
            pending_events: VecDeque::new(),
            client_info: None,
            user_agent: None,
            ip_address: None,
            time_provider,
        }
    }

    /// Get session ID
    pub fn id(&self) -> SessionId {
        self.id
    }

    /// Get current state
    pub fn state(&self) -> &SessionState {
        &self.state
    }

    /// Get configuration
    pub fn config(&self) -> &SessionConfig {
        &self.config
    }

    /// Get statistics
    pub fn stats(&self) -> &SessionStats {
        &self.stats
    }

    /// Get creation timestamp
    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    /// Get last update timestamp
    pub fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }

    /// Get expiration timestamp
    pub fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }

    /// Get completion timestamp
    pub fn completed_at(&self) -> Option<DateTime<Utc>> {
        self.completed_at
    }

    /// Get client info metadata
    pub fn client_info(&self) -> Option<&str> {
        self.client_info.as_deref()
    }

    /// Get session duration if completed
    pub fn duration(&self) -> Option<chrono::Duration> {
        self.completed_at.map(|end| end - self.created_at)
    }

    /// Check if session is expired
    pub fn is_expired(&self) -> bool {
        self.time_provider.now() > self.expires_at
    }

    /// Check if session is active
    pub fn is_active(&self) -> bool {
        matches!(self.state, SessionState::Active) && !self.is_expired()
    }

    /// Get all streams
    pub fn streams(&self) -> &HashMap<StreamId, Stream> {
        &self.streams
    }

    /// Get stream by ID
    pub fn stream(&self, stream_id: StreamId) -> Option<&Stream> {
        self.streams.get(&stream_id)
    }

    /// Update configuration of a child stream through the aggregate root.
    ///
    /// Child mutations must flow through the aggregate so the session-level
    /// timestamp is bumped and a [`DomainEvent::StreamConfigUpdated`] event is
    /// raised. Application code MUST NOT reach into `Stream` directly to
    /// mutate config — the previous `get_stream_mut` accessor that allowed
    /// this was a DDD violation (see issue #259).
    pub fn update_stream_config(
        &mut self,
        stream_id: StreamId,
        config: StreamConfig,
    ) -> DomainResult<()> {
        let stream = self
            .streams
            .get_mut(&stream_id)
            .ok_or_else(|| DomainError::StreamNotFound(stream_id.to_string()))?;

        stream.update_config(config)?;
        self.update_timestamp();

        self.add_event(DomainEvent::StreamConfigUpdated {
            session_id: self.id,
            stream_id,
            timestamp: self.time_provider.now(),
        });

        Ok(())
    }

    /// Generate patch frames for a child stream through the aggregate root.
    ///
    /// Wraps [`Stream::create_patch_frames`] so that session-level statistics
    /// (`stats.total_frames`, `stats.total_bytes`) and the `updated_at`
    /// timestamp stay consistent with the child mutation, and a
    /// [`DomainEvent::FramesBatched`] event is raised when frames are
    /// produced.
    ///
    /// `stats.total_bytes` is taken as a before/after delta of the child
    /// [`Stream`]'s own `total_bytes` counter — which `create_patch_frames`
    /// already computes internally via [`Frame::estimated_size`] for every
    /// frame it returns — rather than re-summing `estimated_size()` here,
    /// avoiding a second full JSON serialization pass per frame.
    pub fn create_stream_patch_frames(
        &mut self,
        stream_id: StreamId,
        priority_threshold: Priority,
        max_frames: usize,
    ) -> DomainResult<Vec<Frame>> {
        let patches = self.extract_prioritized_patches_for_stream(stream_id, priority_threshold)?;
        self.commit_patch_frames_for_stream(stream_id, patches, max_frames)
    }

    /// Compute prioritized patches for `stream_id` without mutating any
    /// state — the expensive half of [`Self::create_stream_patch_frames`].
    /// See [`Stream::extract_prioritized_patches`] for why this is safe to
    /// call without holding any lock a caller might otherwise need around
    /// the mutating half, [`Self::commit_patch_frames_for_stream`].
    pub fn extract_prioritized_patches_for_stream(
        &self,
        stream_id: StreamId,
        priority_threshold: Priority,
    ) -> DomainResult<Vec<(FramePatch, Priority)>> {
        let stream = self
            .streams
            .get(&stream_id)
            .ok_or_else(|| DomainError::StreamNotFound(stream_id.to_string()))?;

        stream.extract_prioritized_patches(priority_threshold)
    }

    /// Commit already-extracted prioritized patches (from
    /// [`Self::extract_prioritized_patches_for_stream`]) into frames for
    /// `stream_id` — the cheap, `O(max_frames)` half of
    /// [`Self::create_stream_patch_frames`], updating session-level
    /// statistics and raising [`DomainEvent::FramesBatched`] exactly as that
    /// method does.
    ///
    /// `stats.total_bytes` is taken as a before/after delta of the child
    /// [`Stream`]'s own `total_bytes` counter — which
    /// [`Stream::commit_patch_frames`] already computes internally via
    /// [`Frame::estimated_size`] for every frame it returns — rather than
    /// re-summing `estimated_size()` here, avoiding a second full JSON
    /// serialization pass per frame.
    pub fn commit_patch_frames_for_stream(
        &mut self,
        stream_id: StreamId,
        patches: Vec<(FramePatch, Priority)>,
        max_frames: usize,
    ) -> DomainResult<Vec<Frame>> {
        let stream = self
            .streams
            .get_mut(&stream_id)
            .ok_or_else(|| DomainError::StreamNotFound(stream_id.to_string()))?;

        let bytes_before = stream.stats().total_bytes;
        let frames = stream.commit_patch_frames(patches, max_frames)?;
        let bytes_after = stream.stats().total_bytes;

        self.stats.total_frames += frames.len() as u64;
        self.stats.total_bytes += bytes_after - bytes_before;
        self.update_timestamp();

        if !frames.is_empty() {
            self.add_event(DomainEvent::FramesBatched {
                session_id: self.id,
                frame_count: frames.len(),
                timestamp: self.time_provider.now(),
            });
        }

        Ok(frames)
    }

    /// Activate session
    pub fn activate(&mut self) -> DomainResult<()> {
        match self.state {
            SessionState::Initializing => {
                self.state = SessionState::Active;
                self.update_timestamp();

                self.add_event(DomainEvent::SessionActivated {
                    session_id: self.id,
                    timestamp: self.time_provider.now(),
                });

                Ok(())
            }
            _ => Err(DomainError::InvalidStateTransition(format!(
                "Cannot activate session from state: {:?}",
                self.state
            ))),
        }
    }

    /// Create new stream in this session
    pub fn create_stream(&mut self, source_data: JsonData) -> DomainResult<StreamId> {
        if !self.is_active() {
            return Err(DomainError::InvalidSessionState(
                "Session is not active".to_string(),
            ));
        }

        if self.streams.len() >= self.config.max_concurrent_streams {
            return Err(DomainError::TooManyStreams(format!(
                "Maximum {} concurrent streams exceeded",
                self.config.max_concurrent_streams
            )));
        }

        // source_data is now already JsonData (domain type)
        let domain_data = source_data;

        let stream = Stream::new(
            self.id,
            domain_data,
            self.config.default_stream_config.clone(),
        );
        let stream_id = stream.id();

        self.streams.insert(stream_id, stream);
        self.stats.total_streams += 1;
        self.stats.active_streams += 1;
        self.update_timestamp();

        self.add_event(DomainEvent::StreamCreated {
            session_id: self.id,
            stream_id,
            timestamp: self.time_provider.now(),
        });

        Ok(stream_id)
    }

    /// Create a stream and, if provided, apply a custom [`StreamConfig`] to
    /// it — as a single atomic domain operation.
    ///
    /// Equivalent to calling [`Self::create_stream`] followed by
    /// [`Self::update_stream_config`], but combined so a repository-level
    /// atomic update (which mutates the stored session in place and has no
    /// transactional rollback) never has to reason about two independently
    /// fallible calls. In practice the second call cannot fail here: a
    /// freshly created stream is always in
    /// [`StreamState::Preparing`],
    /// which [`Stream::is_active`](crate::domain::entities::Stream::is_active)
    /// treats as active — the only condition `update_stream_config` checks —
    /// and nothing else can run between the two calls within one `&mut self`
    /// invocation.
    pub fn create_stream_with_config(
        &mut self,
        source_data: JsonData,
        config: Option<StreamConfig>,
    ) -> DomainResult<StreamId> {
        let stream_id = self.create_stream(source_data)?;
        if let Some(config) = config {
            let result = self.update_stream_config(stream_id, config);
            debug_assert!(
                result.is_ok(),
                "update_stream_config failed immediately after create_stream: a freshly \
                 created stream is always Preparing, which is_active() treats as active — \
                 the only condition update_stream_config checks. If this fires, that \
                 invariant broke, and callers relying on this method's atomicity (e.g. a \
                 repository committing the mutation in place with no rollback) may now be \
                 left with a created-but-unconfigured stream on error."
            );
            result?;
        }
        Ok(stream_id)
    }

    /// Start streaming for a specific stream
    pub fn start_stream(&mut self, stream_id: StreamId) -> DomainResult<()> {
        let stream = self
            .streams
            .get_mut(&stream_id)
            .ok_or_else(|| DomainError::StreamNotFound(stream_id.to_string()))?;

        stream.start_streaming()?;
        self.update_timestamp();

        self.add_event(DomainEvent::StreamStarted {
            session_id: self.id,
            stream_id,
            timestamp: self.time_provider.now(),
        });

        Ok(())
    }

    /// Complete a specific stream
    pub fn complete_stream(&mut self, stream_id: StreamId) -> DomainResult<()> {
        let stream = self
            .streams
            .get_mut(&stream_id)
            .ok_or_else(|| DomainError::StreamNotFound(stream_id.to_string()))?;

        stream.complete()?;

        // Update session stats
        self.stats.active_streams = self.stats.active_streams.saturating_sub(1);
        self.stats.completed_streams += 1;

        // Update running arithmetic mean via Welford's incremental formula.
        // `completed_streams` was already incremented above, so it is the
        // sample count including this stream.
        if let Some(duration) = stream.duration() {
            let duration_ms = duration.num_milliseconds() as f64;
            self.stats.average_stream_duration_ms += (duration_ms
                - self.stats.average_stream_duration_ms)
                / self.stats.completed_streams as f64;

            // Fixed-alpha (0.5) EMA for the recency-sensitive companion metric
            // (see #458). Seeded with the first observed duration rather than
            // blended against a zeroed default, matching the lifetime mean's
            // own first-sample behavior above.
            const RECENT_AVG_ALPHA: f64 = 0.5;
            self.stats.recent_avg_duration_ms = if self.stats.completed_streams == 1 {
                duration_ms
            } else {
                RECENT_AVG_ALPHA * duration_ms
                    + (1.0 - RECENT_AVG_ALPHA) * self.stats.recent_avg_duration_ms
            };
        }

        self.update_timestamp();

        self.add_event(DomainEvent::StreamCompleted {
            session_id: self.id,
            stream_id,
            timestamp: self.time_provider.now(),
        });

        Ok(())
    }

    /// Fail a specific stream
    pub fn fail_stream(&mut self, stream_id: StreamId, error: String) -> DomainResult<()> {
        let stream = self
            .streams
            .get_mut(&stream_id)
            .ok_or_else(|| DomainError::StreamNotFound(stream_id.to_string()))?;

        stream.fail(error.clone())?;

        // Update session stats
        self.stats.active_streams = self.stats.active_streams.saturating_sub(1);
        self.stats.failed_streams += 1;

        self.update_timestamp();

        self.add_event(DomainEvent::StreamFailed {
            session_id: self.id,
            stream_id,
            error,
            timestamp: self.time_provider.now(),
        });

        Ok(())
    }

    /// Create frames for all active streams based on priority.
    ///
    /// Convenience single-call wrapper combining
    /// [`Self::extract_prioritized_patches_for_active_streams`] and
    /// [`Self::commit_priority_frames`]. Callers managing their own
    /// concurrency control around the mutating half (e.g. a repository
    /// holding a per-session lock for the read-modify-write) should call
    /// those two methods directly instead, running the expensive extraction
    /// step lock-free — see [`Self::commit_priority_frames`]'s docs for why
    /// this method's single call is unsuitable for that.
    pub fn create_priority_frames(&mut self, batch_size: usize) -> DomainResult<Vec<Frame>> {
        let extracted = self.extract_prioritized_patches_for_active_streams(Priority::BACKGROUND);
        self.commit_priority_frames(extracted, batch_size)
    }

    /// Compute prioritized patches for every currently-`Streaming` stream,
    /// without mutating any state — the expensive half of
    /// [`Self::create_priority_frames`] (one full `source_data` traversal
    /// per stream). Streams not in the `Streaming` state (e.g. still
    /// `Preparing`, or already finished) are silently omitted rather than
    /// erroring, since [`Stream::extract_prioritized_patches`] only accepts
    /// `Streaming` streams and a batch spanning many streams should not fail
    /// as a whole over one that is not yet ready.
    ///
    /// Split out so a caller that needs its own concurrency control around
    /// the *mutating* half (e.g. a repository holding a per-session lock)
    /// can run this traversal lock-free and only take the lock for
    /// [`Self::commit_priority_frames`]'s commit step — mirroring
    /// [`Self::extract_prioritized_patches_for_stream`]'s single-stream
    /// counterpart. Note that commit step is itself proportional to the
    /// total number of patches extracted across every `Streaming` stream,
    /// not to `max_frames`/`batch_size` — see that method's docs.
    pub fn extract_prioritized_patches_for_active_streams(
        &self,
        priority_threshold: Priority,
    ) -> Vec<(StreamId, Vec<(FramePatch, Priority)>)> {
        self.streams
            .iter()
            .filter(|(_, stream)| matches!(stream.state(), StreamState::Streaming))
            .filter_map(|(stream_id, stream)| {
                stream
                    .extract_prioritized_patches(priority_threshold)
                    // Only current error source is the `Streaming`-state
                    // check, already guaranteed by the `filter` above, so
                    // discarding is a no-op today. `collect_patches`
                    // (`stream.rs`) has no depth bound and could grow one
                    // later (the codebase's established pattern for
                    // recursive walkers, e.g. #469/#475) — if it does, that
                    // future error must not be silently dropped here.
                    .ok()
                    .map(|patches| (*stream_id, patches))
            })
            .collect()
    }

    /// Turn already-extracted per-stream patches (from
    /// [`Self::extract_prioritized_patches_for_active_streams`]) into
    /// frames, choosing up to 5 per stream, globally sorting all of them by
    /// priority, and truncating to `batch_size` — the mutating half of
    /// [`Self::create_priority_frames`], updating session-level statistics
    /// and raising [`DomainEvent::FramesBatched`] exactly as that method
    /// does.
    ///
    /// Despite being the half a caller is expected to run under a lock, this
    /// is not cheap or `max_frames`-bounded: [`Stream::commit_patch_frames`]
    /// clones every patch in its input while chunking it into frames
    /// (`batch_patches_into_frames`, `stream.rs`), so its cost is
    /// proportional to the total number of patches extracted across every
    /// `Streaming` stream in the session, not to `max_frames`/`batch_size`
    /// or the number of frames actually produced. Inherited from
    /// `create_stream_patch_frames_atomic`'s pre-existing single-stream
    /// commit step (#472); not addressed here.
    ///
    /// A `stream_id` in `extracted` that no longer exists, or transitioned
    /// out of `Streaming` between extraction and this call (e.g. completed
    /// concurrently), is silently skipped rather than failing the whole
    /// batch — the same reasoning as
    /// [`Self::extract_prioritized_patches_for_active_streams`]'s omission,
    /// and critically: unlike that read-only method, propagating an error
    /// here mid-loop via `?` would leave streams processed by earlier loop
    /// iterations with their frame/sequence bookkeeping already committed
    /// while this method's own session-level stats update and
    /// `FramesBatched` event never run, permanently desynchronizing the two
    /// (see #477).
    ///
    /// Unlike [`Self::commit_patch_frames_for_stream`], `stats.total_bytes`
    /// here cannot be taken as a child [`Stream`]'s before/after delta:
    /// frames are committed per-stream but then globally sorted by priority
    /// and truncated to `batch_size`, so a per-stream delta would count
    /// bytes for frames this call discards. `estimated_size()` is summed
    /// directly over only the frames actually retained in the result.
    pub fn commit_priority_frames(
        &mut self,
        extracted: Vec<(StreamId, Vec<(FramePatch, Priority)>)>,
        batch_size: usize,
    ) -> DomainResult<Vec<Frame>> {
        if !self.is_active() {
            return Err(DomainError::InvalidSessionState(
                "Session is not active".to_string(),
            ));
        }

        let mut stream_frames: Vec<(Priority, StreamId, Frame)> = Vec::new();

        for (stream_id, patches) in extracted {
            let Some(stream) = self.streams.get_mut(&stream_id) else {
                continue;
            };
            if !matches!(stream.state(), StreamState::Streaming) {
                continue;
            }
            let Ok(frames) = stream.commit_patch_frames(patches, 5) else {
                continue;
            };

            for frame in frames {
                let priority = frame.priority();
                stream_frames.push((priority, stream_id, frame));
            }
        }

        // Sort by priority (descending)
        stream_frames.sort_by_key(|frame| std::cmp::Reverse(frame.0));

        // Take up to batch_size frames
        let all_frames: Vec<Frame> = stream_frames
            .into_iter()
            .take(batch_size)
            .map(|(_, _, frame)| frame)
            .collect();

        // Update session stats
        self.stats.total_frames += all_frames.len() as u64;
        self.stats.total_bytes += all_frames
            .iter()
            .map(|frame| frame.estimated_size() as u64)
            .sum::<u64>();
        self.update_timestamp();

        if !all_frames.is_empty() {
            self.add_event(DomainEvent::FramesBatched {
                session_id: self.id,
                frame_count: all_frames.len(),
                timestamp: self.time_provider.now(),
            });
        }

        Ok(all_frames)
    }

    /// Close session gracefully
    pub fn close(&mut self) -> DomainResult<()> {
        match self.state {
            SessionState::Active => {
                self.state = SessionState::Closing;

                // Close all active streams
                let active_stream_ids: Vec<_> = self
                    .streams
                    .iter()
                    .filter(|(_, stream)| stream.is_active())
                    .map(|(id, _)| *id)
                    .collect();

                for stream_id in active_stream_ids {
                    if let Some(stream) = self.streams.get_mut(&stream_id) {
                        let _ = stream.cancel(); // Best effort
                    }
                }

                self.state = SessionState::Completed;
                self.completed_at = Some(self.time_provider.now());
                self.update_timestamp();

                self.add_event(DomainEvent::SessionClosed {
                    session_id: self.id,
                    timestamp: self.time_provider.now(),
                });

                Ok(())
            }
            _ => Err(DomainError::InvalidStateTransition(format!(
                "Cannot close session from state: {:?}",
                self.state
            ))),
        }
    }

    /// Force close expired session with proper cleanup
    pub fn force_close_expired(&mut self) -> DomainResult<bool> {
        if !self.is_expired() {
            return Ok(false);
        }

        // Force close regardless of current state
        let old_state = self.state.clone();
        self.state = SessionState::Failed;
        self.completed_at = Some(self.time_provider.now());
        self.update_timestamp();

        // Force cancel all streams with timeout reason
        for stream in self.streams.values_mut() {
            let _ = stream.cancel(); // Best effort cleanup
        }

        // Clear stream collections for memory cleanup
        self.streams.clear();

        // Emit timeout event
        self.add_event(DomainEvent::SessionTimedOut {
            session_id: self.id,
            original_state: old_state,
            timeout_duration: self.config.session_timeout_seconds,
            timestamp: self.time_provider.now(),
        });

        Ok(true)
    }

    /// Extend session timeout (if allowed)
    pub fn extend_timeout(&mut self, additional_seconds: u64) -> DomainResult<()> {
        if self.is_expired() {
            return Err(DomainError::InvalidStateTransition(
                "Cannot extend timeout for expired session".to_string(),
            ));
        }

        self.expires_at += chrono::Duration::seconds(additional_seconds as i64);
        self.update_timestamp();

        self.add_event(DomainEvent::SessionTimeoutExtended {
            session_id: self.id,
            additional_seconds,
            new_expires_at: self.expires_at,
            timestamp: self.time_provider.now(),
        });

        Ok(())
    }

    /// Set client information
    pub fn set_client_info(
        &mut self,
        client_info: String,
        user_agent: Option<String>,
        ip_address: Option<String>,
    ) {
        self.client_info = Some(client_info);
        self.user_agent = user_agent;
        self.ip_address = ip_address;
        self.update_timestamp();
    }

    /// Get pending domain events
    pub fn pending_events(&self) -> &VecDeque<DomainEvent> {
        &self.pending_events
    }

    /// Take all pending events (clears the queue)
    pub fn take_events(&mut self) -> VecDeque<DomainEvent> {
        std::mem::take(&mut self.pending_events)
    }

    /// Check session health
    pub fn health_check(&self) -> SessionHealth {
        let active_count = self.streams.values().filter(|s| s.is_active()).count();
        let failed_count = self
            .streams
            .values()
            .filter(|s| {
                matches!(
                    s.state(),
                    crate::domain::entities::stream::StreamState::Failed
                )
            })
            .count();

        SessionHealth {
            is_healthy: self.is_active() && failed_count == 0,
            active_streams: active_count,
            failed_streams: failed_count,
            is_expired: self.is_expired(),
            uptime_seconds: (self.time_provider.now() - self.created_at).num_seconds(),
        }
    }

    /// Private helper: Add domain event
    fn add_event(&mut self, event: DomainEvent) {
        self.pending_events.push_back(event);
    }

    /// Private helper: Update timestamp
    fn update_timestamp(&mut self) {
        self.updated_at = self.time_provider.now();
    }
}

/// Session health information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionHealth {
    /// Aggregate health flag derived from rates and recent activity.
    pub is_healthy: bool,
    /// Number of streams currently in an active state.
    pub active_streams: usize,
    /// Number of streams that have terminated with an error.
    pub failed_streams: usize,
    /// Whether the session has passed its expiry instant.
    pub is_expired: bool,
    /// Seconds elapsed since the session was created.
    pub uptime_seconds: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_creation_and_activation() {
        let mut session = StreamSession::new(SessionConfig::default());

        assert_eq!(session.state(), &SessionState::Initializing);
        assert!(!session.is_active());

        assert!(session.activate().is_ok());
        assert_eq!(session.state(), &SessionState::Active);
        assert!(session.is_active());
    }

    #[test]
    fn test_stream_management() {
        let mut session = StreamSession::new(SessionConfig::default());
        assert!(session.activate().is_ok());

        let mut map = HashMap::new();
        map.insert("test".to_string(), JsonData::String("data".to_string()));
        let source_data = JsonData::Object(map);

        // Create stream
        let stream_id = session.create_stream(source_data).unwrap();
        assert_eq!(session.streams().len(), 1);
        assert_eq!(session.stats().total_streams, 1);
        assert_eq!(session.stats().active_streams, 1);

        // Start stream
        assert!(session.start_stream(stream_id).is_ok());

        // Complete stream
        assert!(session.complete_stream(stream_id).is_ok());
        assert_eq!(session.stats().active_streams, 0);
        assert_eq!(session.stats().completed_streams, 1);
    }

    #[test]
    fn test_create_stream_with_config_none_behaves_like_create_stream() {
        let mut session = StreamSession::new(SessionConfig::default());
        assert!(session.activate().is_ok());

        let stream_id = session
            .create_stream_with_config(JsonData::String("test".to_string()), None)
            .unwrap();

        assert_eq!(session.streams().len(), 1);
        assert_eq!(session.stats().total_streams, 1);
        assert_eq!(session.stats().active_streams, 1);
        assert!(session.stream(stream_id).is_some());
    }

    #[test]
    fn test_create_stream_with_config_applies_custom_config() {
        let mut session = StreamSession::new(SessionConfig::default());
        assert!(session.activate().is_ok());

        let custom_config = StreamConfig {
            max_frame_size: 1234,
            ..StreamConfig::default()
        };

        let stream_id = session
            .create_stream_with_config(
                JsonData::String("test".to_string()),
                Some(custom_config.clone()),
            )
            .unwrap();

        assert_eq!(
            session.stream(stream_id).unwrap().config().max_frame_size,
            custom_config.max_frame_size
        );
    }

    #[test]
    fn test_average_stream_duration_single_stream_equals_its_duration() {
        let mut session = StreamSession::new(SessionConfig::default());
        assert!(session.activate().is_ok());

        let mut map = HashMap::new();
        map.insert("test".to_string(), JsonData::String("data".to_string()));
        let source_data = JsonData::Object(map);

        let stream_id = session.create_stream(source_data).unwrap();
        assert!(session.start_stream(stream_id).is_ok());
        std::thread::sleep(std::time::Duration::from_millis(15));
        assert!(session.complete_stream(stream_id).is_ok());

        let d1 = session.stream(stream_id).unwrap().duration().unwrap();
        let d1_ms = d1.num_milliseconds() as f64;

        // A single sample's running mean must equal the sample itself, not
        // the old EMA result of `duration_ms / 2.0`. Exact `f64` equality is
        // safe here only because `num_milliseconds()` yields small whole
        // integers with no rounding error, not because float comparison is
        // generally safe in this codebase.
        assert_eq!(session.stats().average_stream_duration_ms, d1_ms);
    }

    #[test]
    fn test_average_stream_duration_two_streams_is_true_arithmetic_mean() {
        let mut session = StreamSession::new(SessionConfig::default());
        assert!(session.activate().is_ok());

        let mut map = HashMap::new();
        map.insert("test".to_string(), JsonData::String("data".to_string()));
        let source_data = JsonData::Object(map);

        let stream_id_1 = session.create_stream(source_data.clone()).unwrap();
        assert!(session.start_stream(stream_id_1).is_ok());
        std::thread::sleep(std::time::Duration::from_millis(15));
        assert!(session.complete_stream(stream_id_1).is_ok());
        let d1_ms = session
            .stream(stream_id_1)
            .unwrap()
            .duration()
            .unwrap()
            .num_milliseconds() as f64;

        let stream_id_2 = session.create_stream(source_data).unwrap();
        assert!(session.start_stream(stream_id_2).is_ok());
        std::thread::sleep(std::time::Duration::from_millis(30));
        assert!(session.complete_stream(stream_id_2).is_ok());
        let d2_ms = session
            .stream(stream_id_2)
            .unwrap()
            .duration()
            .unwrap()
            .num_milliseconds() as f64;

        // The true arithmetic mean of two samples, not the old EMA result
        // (which would weigh the second sample by 0.5 regardless of history).
        // Exact `f64` equality is safe here only because both samples are
        // small whole millisecond integers, not because float comparison is
        // generally safe in this codebase.
        assert_eq!(
            session.stats().average_stream_duration_ms,
            (d1_ms + d2_ms) / 2.0
        );
    }

    #[test]
    fn test_recent_avg_duration_seeded_with_first_sample() {
        let mut session = StreamSession::new(SessionConfig::default());
        assert!(session.activate().is_ok());

        let stream_id = session
            .create_stream(JsonData::String("test".to_string()))
            .unwrap();
        assert!(session.start_stream(stream_id).is_ok());
        std::thread::sleep(std::time::Duration::from_millis(15));
        assert!(session.complete_stream(stream_id).is_ok());

        let d1_ms = session
            .stream(stream_id)
            .unwrap()
            .duration()
            .unwrap()
            .num_milliseconds() as f64;

        // The first sample seeds the EMA directly, mirroring the lifetime
        // mean's own first-sample behavior — the two fields agree until a
        // second sample arrives.
        assert_eq!(session.stats().recent_avg_duration_ms, d1_ms);
        assert_eq!(
            session.stats().recent_avg_duration_ms,
            session.stats().average_stream_duration_ms
        );
    }

    #[test]
    fn test_recent_avg_duration_applies_fixed_alpha_ema_and_diverges_from_lifetime_mean() {
        let mut session = StreamSession::new(SessionConfig::default());
        assert!(session.activate().is_ok());

        // Deliberately non-uniform sleeps so the fixed-alpha EMA (which
        // decays older samples) and the lifetime arithmetic mean (which
        // never decays) diverge.
        let mut durations_ms = Vec::new();
        for sleep_ms in [15, 60, 15] {
            let stream_id = session
                .create_stream(JsonData::String("test".to_string()))
                .unwrap();
            assert!(session.start_stream(stream_id).is_ok());
            std::thread::sleep(std::time::Duration::from_millis(sleep_ms));
            assert!(session.complete_stream(stream_id).is_ok());
            durations_ms.push(
                session
                    .stream(stream_id)
                    .unwrap()
                    .duration()
                    .unwrap()
                    .num_milliseconds() as f64,
            );
        }

        const ALPHA: f64 = 0.5;
        let mut expected_ema = durations_ms[0];
        for &d in &durations_ms[1..] {
            expected_ema = ALPHA * d + (1.0 - ALPHA) * expected_ema;
        }

        assert_eq!(session.stats().recent_avg_duration_ms, expected_ema);
        assert_ne!(
            session.stats().recent_avg_duration_ms,
            session.stats().average_stream_duration_ms
        );
    }

    #[test]
    fn test_create_stream_patch_frames_tracks_total_bytes() {
        let mut session = StreamSession::new(SessionConfig::default());
        assert!(session.activate().is_ok());

        let source_data: JsonData = serde_json::json!({
            "id": "abc-123",
            "name": "Alice",
            "bio": "a".repeat(500),
            "items": [1, 2, 3, 4, 5]
        })
        .into();

        let stream_id = session.create_stream(source_data).unwrap();
        assert!(session.start_stream(stream_id).is_ok());
        assert_eq!(session.stats().total_bytes, 0);

        let frames = session
            .create_stream_patch_frames(stream_id, Priority::BACKGROUND, 16)
            .expect("frame generation must succeed");
        assert!(!frames.is_empty());

        let expected_bytes: u64 = frames.iter().map(|f| f.estimated_size() as u64).sum();
        assert_eq!(session.stats().total_bytes, expected_bytes);
        assert!(session.stats().total_bytes > 0);

        // A smaller payload must accumulate proportionally fewer bytes.
        let mut small_session = StreamSession::new(SessionConfig::default());
        assert!(small_session.activate().is_ok());
        let small_data: JsonData = serde_json::json!({ "id": "x" }).into();
        let small_stream_id = small_session.create_stream(small_data).unwrap();
        assert!(small_session.start_stream(small_stream_id).is_ok());
        small_session
            .create_stream_patch_frames(small_stream_id, Priority::BACKGROUND, 16)
            .expect("frame generation must succeed");

        assert!(session.stats().total_bytes > small_session.stats().total_bytes);
    }

    #[test]
    fn test_extract_then_commit_patch_frames_matches_combined_call() {
        // The split API (extract_prioritized_patches_for_stream +
        // commit_patch_frames_for_stream) must produce identical results to
        // the combined create_stream_patch_frames it's built from, since a
        // repository holding a per-session lock relies on this equivalence
        // to narrow its critical section (#457 follow-up).
        let source_data: JsonData = serde_json::json!({
            "id": "abc-123",
            "name": "Alice",
            "items": [1, 2, 3]
        })
        .into();

        let mut combined = StreamSession::new(SessionConfig::default());
        combined.activate().unwrap();
        let combined_stream_id = combined.create_stream(source_data.clone()).unwrap();
        combined.start_stream(combined_stream_id).unwrap();
        let combined_frames = combined
            .create_stream_patch_frames(combined_stream_id, Priority::BACKGROUND, 16)
            .unwrap();

        let mut split = StreamSession::new(SessionConfig::default());
        split.activate().unwrap();
        let split_stream_id = split.create_stream(source_data).unwrap();
        split.start_stream(split_stream_id).unwrap();
        let patches = split
            .extract_prioritized_patches_for_stream(split_stream_id, Priority::BACKGROUND)
            .unwrap();
        let split_frames = split
            .commit_patch_frames_for_stream(split_stream_id, patches, 16)
            .unwrap();

        assert_eq!(combined_frames.len(), split_frames.len());
        assert_eq!(combined.stats().total_frames, split.stats().total_frames);
        assert_eq!(combined.stats().total_bytes, split.stats().total_bytes);
    }

    #[test]
    fn test_commit_patch_frames_for_stream_rejects_stream_completed_since_extraction() {
        // Guards the safety argument for narrowing create_stream_patch_frames_atomic's
        // critical section: if the target stream stops being Streaming between
        // an earlier extract_prioritized_patches_for_stream call (run outside
        // any lock) and the commit, the commit must fail cleanly instead of
        // silently mutating a stream that can no longer accept frames.
        let mut session = StreamSession::new(SessionConfig::default());
        session.activate().unwrap();
        let stream_id = session
            .create_stream(JsonData::String("test".to_string()))
            .unwrap();
        session.start_stream(stream_id).unwrap();

        let patches = session
            .extract_prioritized_patches_for_stream(stream_id, Priority::BACKGROUND)
            .unwrap();

        // Simulates a concurrent CompleteStreamCommand landing between the
        // extraction above and the commit below.
        session.complete_stream(stream_id).unwrap();

        let result = session.commit_patch_frames_for_stream(stream_id, patches, 16);
        assert!(matches!(result, Err(DomainError::InvalidStreamState(_))));
    }

    #[test]
    fn test_concurrent_stream_limit() {
        let config = SessionConfig {
            max_concurrent_streams: 2,
            ..Default::default()
        };
        let mut session = StreamSession::new(config);
        assert!(session.activate().is_ok());

        let source_data = JsonData::Object(HashMap::new());

        // Create max streams
        assert!(session.create_stream(source_data.clone()).is_ok());
        assert!(session.create_stream(source_data.clone()).is_ok());

        // Should fail to create third stream
        assert!(session.create_stream(source_data).is_err());
    }

    #[test]
    fn test_session_expiration() {
        let config = SessionConfig {
            session_timeout_seconds: 1,
            ..Default::default()
        };
        let session = StreamSession::new(config);

        // Session should not be expired immediately
        assert!(!session.is_expired());

        // Would need to sleep for 1+ seconds to test expiration in real scenario
        // For unit test, we verify the expiration logic exists
        assert!(session.expires_at > session.created_at);
    }

    #[test]
    fn test_domain_events() {
        let mut session = StreamSession::new(SessionConfig::default());

        // Events should be generated for state transitions
        assert!(session.activate().is_ok());
        assert!(!session.pending_events().is_empty());

        let events = session.take_events();
        assert_eq!(events.len(), 1);

        // Events queue should be empty after taking
        assert!(session.pending_events().is_empty());
    }

    #[test]
    fn test_session_health() {
        let mut session = StreamSession::new(SessionConfig::default());
        assert!(session.activate().is_ok());

        let health = session.health_check();
        assert!(health.is_healthy);
        assert_eq!(health.active_streams, 0);
        assert_eq!(health.failed_streams, 0);
        assert!(!health.is_expired);
        assert!(health.uptime_seconds >= 0);
    }
}
