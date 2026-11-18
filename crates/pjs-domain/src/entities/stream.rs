//! Stream entity representing a prioritized data stream

use crate::{
    DomainError, DomainResult,
    clock::TimeProvider,
    entities::{Frame, frame::FramePatch},
    services::skeleton,
    value_objects::{
        JsonData, JsonPath, MAX_DESERIALIZE_DEPTH, PathSegment, Priority, SessionId, StreamId,
    },
};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

/// Stream state in its lifecycle
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum StreamState {
    /// Stream is being prepared
    Preparing,
    /// Stream is actively sending data
    Streaming,
    /// Stream completed successfully
    Completed,
    /// Stream failed with error
    Failed,
    /// Stream was cancelled
    Cancelled,
}

/// Stream configuration and metadata
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StreamConfig {
    /// Custom priority rules
    #[serde(with = "crate::serde_support::priority_map")]
    pub priority_rules: HashMap<String, Priority>,
}

/// Stream statistics for monitoring
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StreamStats {
    /// Total number of frames generated
    pub total_frames: u64,
    /// Number of skeleton frames sent
    pub skeleton_frames: u64,
    /// Number of patch frames sent
    pub patch_frames: u64,
    /// Number of completion frames sent
    pub complete_frames: u64,
    /// Number of error frames sent
    pub error_frames: u64,
    /// Total bytes transmitted across all frames
    pub total_bytes: u64,
    /// Bytes transmitted in critical priority frames
    pub critical_bytes: u64,
    /// Bytes transmitted in high priority frames
    pub high_priority_bytes: u64,
    /// Average size of frames in bytes
    pub average_frame_size: f64,
}

/// Lifecycle position of a [`Stream`]; owns the data that only exists in a
/// given state, so `completed_at` and the failure reason cannot disagree with it.
#[derive(Debug, Clone)]
enum Lifecycle {
    Preparing,
    Streaming,
    Completed { at: Timestamp },
    Failed { at: Timestamp, reason: String },
    Cancelled { at: Timestamp },
}

/// Time source of a [`Stream`]; `System` avoids an allocation for the default case.
#[derive(Clone)]
enum Clock {
    System,
    Injected(Arc<dyn TimeProvider>),
}

impl Clock {
    fn now(&self) -> Timestamp {
        match self {
            Self::System => Timestamp::now(),
            Self::Injected(clock) => clock.now(),
        }
    }
}

impl fmt::Debug for Clock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::System => f.write_str("System"),
            Self::Injected(_) => f.write_str("Injected"),
        }
    }
}

/// Priority data stream entity
///
/// # Wire format
///
/// Serializes as the stream layout with `state`, `completed_at` and, only in
/// the `Failed` state, `failure_reason`. Decoding validates that these agree
/// with the state and that `next_sequence` is at least 1. The clock is not part
/// of the wire format; a decoded stream uses the system clock.
#[derive(Debug, Clone, Deserialize)]
#[serde(try_from = "StreamWire")]
pub struct Stream {
    id: StreamId,
    session_id: SessionId,
    lifecycle: Lifecycle,
    config: StreamConfig,
    stats: StreamStats,
    created_at: Timestamp,
    updated_at: Timestamp,
    next_sequence: u64,
    source_data: Option<JsonData>,
    metadata: HashMap<String, String>,
    clock: Clock,
}

/// Private decoding layout of [`Stream`]; validated by `TryFrom`.
#[derive(Deserialize)]
struct StreamWire {
    #[serde(with = "crate::serde_support::stream_id")]
    id: StreamId,
    #[serde(with = "crate::serde_support::session_id")]
    session_id: SessionId,
    state: StreamState,
    config: StreamConfig,
    stats: StreamStats,
    created_at: Timestamp,
    updated_at: Timestamp,
    completed_at: Option<Timestamp>,
    #[serde(default)]
    failure_reason: Option<String>,
    next_sequence: u64,
    source_data: Option<JsonData>,
    metadata: HashMap<String, String>,
}

/// Borrowed encoding view of [`Stream`], so serializing never clones it.
#[derive(Serialize)]
struct StreamRef<'a> {
    #[serde(with = "crate::serde_support::stream_id")]
    id: StreamId,
    #[serde(with = "crate::serde_support::session_id")]
    session_id: SessionId,
    state: &'a StreamState,
    config: &'a StreamConfig,
    stats: &'a StreamStats,
    created_at: Timestamp,
    updated_at: Timestamp,
    completed_at: Option<Timestamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure_reason: Option<&'a str>,
    next_sequence: u64,
    source_data: &'a Option<JsonData>,
    metadata: &'a HashMap<String, String>,
}

impl Serialize for Stream {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        StreamRef {
            id: self.id,
            session_id: self.session_id,
            state: self.state(),
            config: &self.config,
            stats: &self.stats,
            created_at: self.created_at,
            updated_at: self.updated_at,
            completed_at: self.completed_at(),
            failure_reason: self.failure_reason(),
            next_sequence: self.next_sequence,
            source_data: &self.source_data,
            metadata: &self.metadata,
        }
        .serialize(serializer)
    }
}

impl TryFrom<StreamWire> for Stream {
    type Error = DomainError;

    fn try_from(wire: StreamWire) -> DomainResult<Self> {
        let lifecycle = match (wire.state, wire.completed_at, wire.failure_reason) {
            (StreamState::Preparing, None, None) => Lifecycle::Preparing,
            (StreamState::Streaming, None, None) => Lifecycle::Streaming,
            (StreamState::Completed, Some(at), None) => Lifecycle::Completed { at },
            (StreamState::Cancelled, Some(at), None) => Lifecycle::Cancelled { at },
            (StreamState::Failed, Some(at), Some(reason)) => Lifecycle::Failed { at, reason },
            (state, completed_at, failure_reason) => {
                return Err(DomainError::InvalidStreamState(format!(
                    "state {state:?} disagrees with completed_at {completed_at:?} and \
                     failure_reason {failure_reason:?}"
                )));
            }
        };
        if wire.next_sequence == 0 {
            return Err(DomainError::InvalidStreamState(
                "next_sequence must be at least 1".to_string(),
            ));
        }
        Ok(Self {
            id: wire.id,
            session_id: wire.session_id,
            lifecycle,
            config: wire.config,
            stats: wire.stats,
            created_at: wire.created_at,
            updated_at: wire.updated_at,
            next_sequence: wire.next_sequence,
            source_data: wire.source_data,
            metadata: wire.metadata,
            clock: Clock::System,
        })
    }
}

impl Stream {
    /// Create new stream timestamped by the system clock.
    pub fn new(session_id: SessionId, source_data: JsonData, config: StreamConfig) -> Self {
        Self::with_clock_source(session_id, source_data, config, Clock::System)
    }

    /// Create a stream whose every timestamp (`created_at`, `updated_at`,
    /// `completed_at` and the timestamps of the frames it builds) comes from
    /// `clock`, so an owner with an injected clock runs a single time domain.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::Arc;
    /// use jiff::Timestamp;
    /// use pjson_rs_domain::Stream;
    /// use pjson_rs_domain::clock::TimeProvider;
    /// use pjson_rs_domain::value_objects::{JsonData, SessionId};
    ///
    /// struct Epoch;
    /// impl TimeProvider for Epoch {
    ///     fn now(&self) -> Timestamp {
    ///         Timestamp::UNIX_EPOCH
    ///     }
    /// }
    ///
    /// let stream = Stream::with_clock(
    ///     SessionId::new(),
    ///     JsonData::Null,
    ///     Default::default(),
    ///     Arc::new(Epoch),
    /// );
    /// assert_eq!(stream.created_at(), Timestamp::UNIX_EPOCH);
    /// ```
    pub fn with_clock(
        session_id: SessionId,
        source_data: JsonData,
        config: StreamConfig,
        clock: Arc<dyn TimeProvider>,
    ) -> Self {
        Self::with_clock_source(session_id, source_data, config, Clock::Injected(clock))
    }

    fn with_clock_source(
        session_id: SessionId,
        source_data: JsonData,
        config: StreamConfig,
        clock: Clock,
    ) -> Self {
        let now = clock.now();

        Self {
            id: StreamId::new(),
            session_id,
            lifecycle: Lifecycle::Preparing,
            config,
            stats: StreamStats::default(),
            created_at: now,
            updated_at: now,
            next_sequence: 1,
            source_data: Some(source_data),
            metadata: HashMap::new(),
            clock,
        }
    }

    /// Get stream ID
    pub fn id(&self) -> StreamId {
        self.id
    }

    /// Get session ID
    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Get current state
    pub fn state(&self) -> &StreamState {
        match self.lifecycle {
            Lifecycle::Preparing => &StreamState::Preparing,
            Lifecycle::Streaming => &StreamState::Streaming,
            Lifecycle::Completed { .. } => &StreamState::Completed,
            Lifecycle::Failed { .. } => &StreamState::Failed,
            Lifecycle::Cancelled { .. } => &StreamState::Cancelled,
        }
    }

    /// Get configuration
    pub fn config(&self) -> &StreamConfig {
        &self.config
    }

    /// Get statistics
    pub fn stats(&self) -> &StreamStats {
        &self.stats
    }

    /// Get creation timestamp
    pub fn created_at(&self) -> Timestamp {
        self.created_at
    }

    /// Get last update timestamp
    pub fn updated_at(&self) -> Timestamp {
        self.updated_at
    }

    /// Get completion timestamp
    pub fn completed_at(&self) -> Option<Timestamp> {
        match self.lifecycle {
            Lifecycle::Preparing | Lifecycle::Streaming => None,
            Lifecycle::Completed { at }
            | Lifecycle::Failed { at, .. }
            | Lifecycle::Cancelled { at } => Some(at),
        }
    }

    /// Reason the stream failed, `Some` only in the `Failed` state.
    ///
    /// Independent of [`Self::metadata`]: writing an `error` metadata entry
    /// neither sets nor changes it.
    pub fn failure_reason(&self) -> Option<&str> {
        match &self.lifecycle {
            Lifecycle::Failed { reason, .. } => Some(reason),
            _ => None,
        }
    }

    /// Get source data
    pub fn source_data(&self) -> Option<&JsonData> {
        self.source_data.as_ref()
    }

    /// Get metadata
    pub fn metadata(&self) -> &HashMap<String, String> {
        &self.metadata
    }

    /// Add metadata
    pub fn add_metadata(&mut self, key: String, value: String) {
        self.metadata.insert(key, value);
        self.update_timestamp();
    }

    /// Start streaming (transition to Streaming state)
    pub fn start_streaming(&mut self) -> DomainResult<()> {
        match self.lifecycle {
            Lifecycle::Preparing => {
                self.lifecycle = Lifecycle::Streaming;
                self.update_timestamp();
                Ok(())
            }
            _ => Err(self.invalid_transition("start streaming")),
        }
    }

    /// Complete stream successfully
    pub fn complete(&mut self) -> DomainResult<()> {
        match self.lifecycle {
            Lifecycle::Streaming => {
                let at = self.update_timestamp();
                self.lifecycle = Lifecycle::Completed { at };
                Ok(())
            }
            _ => Err(self.invalid_transition("complete stream")),
        }
    }

    /// Fail stream with error; the reason is readable through
    /// [`Self::failure_reason`].
    pub fn fail(&mut self, error: String) -> DomainResult<()> {
        match self.lifecycle {
            Lifecycle::Preparing | Lifecycle::Streaming => {
                let at = self.update_timestamp();
                self.lifecycle = Lifecycle::Failed { at, reason: error };
                Ok(())
            }
            _ => Err(self.invalid_transition("fail stream")),
        }
    }

    /// Cancel stream
    pub fn cancel(&mut self) -> DomainResult<()> {
        match self.lifecycle {
            Lifecycle::Preparing | Lifecycle::Streaming => {
                let at = self.update_timestamp();
                self.lifecycle = Lifecycle::Cancelled { at };
                Ok(())
            }
            _ => Err(self.invalid_transition("cancel stream")),
        }
    }

    fn invalid_transition(&self, action: &str) -> DomainError {
        DomainError::InvalidStateTransition(format!(
            "Cannot {action} from state: {:?}",
            self.state()
        ))
    }

    fn ensure_streaming(&self) -> DomainResult<()> {
        if matches!(self.lifecycle, Lifecycle::Streaming) {
            Ok(())
        } else {
            Err(DomainError::InvalidStreamState(
                "Stream must be in streaming state to create frames".to_string(),
            ))
        }
    }

    /// Generate skeleton frame for the stream
    pub fn create_skeleton_frame(&mut self) -> DomainResult<Frame> {
        self.ensure_streaming()?;

        let skeleton_data = self.source_data.as_ref().ok_or_else(|| {
            DomainError::InvalidStreamState("No source data available for skeleton".to_string())
        })?;

        let frame = Frame::skeleton(
            self.id,
            self.next_sequence,
            skeleton(skeleton_data, MAX_DESERIALIZE_DEPTH),
        )
        .with_timestamp(self.clock.now());

        self.record_frame_created(&frame);

        Ok(frame)
    }

    /// Create batch of patch frames based on priority
    pub fn create_patch_frames(
        &mut self,
        priority_threshold: Priority,
        max_frames: usize,
    ) -> DomainResult<Vec<Frame>> {
        let patches = self.extract_prioritized_patches(priority_threshold)?;
        self.commit_patch_frames(patches, max_frames)
    }

    /// Compute prioritized patches for this stream's current `source_data`,
    /// without mutating any state — the expensive half of
    /// [`Self::create_patch_frames`] (a full traversal of `source_data`,
    /// computing a priority for every leaf value).
    ///
    /// Split out from [`Self::commit_patch_frames`] so a caller that needs
    /// its own concurrency control around the *mutating* half (e.g. a
    /// repository holding a per-session lock for the read-modify-write) can
    /// run this traversal lock-free and only take the lock for the commit
    /// step — which is itself `O(patches.len())`, not cheap or
    /// `max_frames`-bounded; see [`Self::commit_patch_frames`]'s docs. Safe
    /// to call without holding any lock: `source_data` is set once at
    /// stream creation and never mutated afterward.
    pub fn extract_prioritized_patches(
        &self,
        priority_threshold: Priority,
    ) -> DomainResult<Vec<(FramePatch, Priority)>> {
        self.ensure_streaming()?;

        let source_data = self.source_data.as_ref().ok_or_else(|| {
            DomainError::InvalidStreamState("No source data available for patches".to_string())
        })?;

        Ok(self.extract_patches(source_data, priority_threshold))
    }

    /// Turn already-extracted prioritized patches (from
    /// [`Self::extract_prioritized_patches`]) into frames and commit their
    /// bookkeeping (`next_sequence`, `stats`) — the other half of
    /// [`Self::create_patch_frames`].
    ///
    /// `max_frames` only bounds the number of frames returned, not the work
    /// done to produce them: every patch in `patches` is still moved into
    /// some frame, so this call's cost is `O(patches.len())`, proportional to
    /// the total number of patches extracted, not to `max_frames`.
    ///
    /// Re-checks the streaming-state precondition itself: if the stream
    /// transitioned out of `Streaming` between the caller's earlier
    /// [`Self::extract_prioritized_patches`] call and this one, this fails
    /// cleanly instead of committing frames for a stream that can no longer
    /// accept them.
    ///
    /// Not strictly all-or-nothing: chunks are finalized one at a time via
    /// [`Self::finalize_patch_frame`], so a construction failure partway
    /// through would leave earlier chunks already committed. Currently
    /// unobservable — [`Frame::patch`] only errors on an empty patch vector,
    /// and chunking never produces an empty chunk here.
    pub fn commit_patch_frames(
        &mut self,
        patches: Vec<(FramePatch, Priority)>,
        max_frames: usize,
    ) -> DomainResult<Vec<Frame>> {
        self.ensure_streaming()?;

        Self::chunk_patches_for_commit(patches, max_frames)
            .into_iter()
            .map(|(priority, frame_patches)| self.finalize_patch_frame(priority, frame_patches))
            .collect()
    }

    /// Build and commit one patch frame from an already-chunked group of
    /// patches (see [`Self::chunk_patches_for_commit`]).
    ///
    /// This is the only place patch-frame `next_sequence`/`stats` bookkeeping
    /// happens: assigns the next sequence number, constructs the [`Frame`],
    /// and records it. Callers that decide
    /// *which* chunked candidates survive before committing (e.g. a
    /// cross-stream priority truncation across multiple streams) should call
    /// this only for the surviving subset, so discarded candidates never
    /// consume a sequence number or inflate stats.
    ///
    /// # Examples
    ///
    /// ```
    /// use pjson_rs_domain::entities::Stream;
    /// use pjson_rs_domain::entities::frame::FramePatch;
    /// use pjson_rs_domain::value_objects::{JsonData, JsonPath, Priority, SessionId};
    ///
    /// let mut stream = Stream::new(
    ///     SessionId::new(),
    ///     JsonData::Object(Default::default()),
    ///     Default::default(),
    /// );
    /// stream.start_streaming().unwrap();
    ///
    /// let patch = FramePatch::set(JsonPath::root(), JsonData::Bool(true));
    /// let frame = stream
    ///     .finalize_patch_frame(Priority::HIGH, vec![patch])
    ///     .unwrap();
    ///
    /// assert_eq!(frame.sequence(), 1);
    /// assert_eq!(stream.stats().total_frames, 1);
    /// ```
    pub fn finalize_patch_frame(
        &mut self,
        priority: Priority,
        frame_patches: Vec<FramePatch>,
    ) -> DomainResult<Frame> {
        self.ensure_streaming()?;

        let frame = Frame::patch(self.id, self.next_sequence, priority, frame_patches)?
            .with_timestamp(self.clock.now());
        self.record_frame_created(&frame);
        Ok(frame)
    }

    /// Create completion frame
    pub fn create_completion_frame(&mut self, checksum: Option<String>) -> DomainResult<Frame> {
        self.ensure_streaming()?;

        let frame =
            Frame::complete(self.id, self.next_sequence, checksum).with_timestamp(self.clock.now());
        self.record_frame_created(&frame);

        Ok(frame)
    }

    /// Check if stream is active
    pub fn is_active(&self) -> bool {
        matches!(self.lifecycle, Lifecycle::Preparing | Lifecycle::Streaming)
    }

    /// Check if stream is finished
    pub fn is_finished(&self) -> bool {
        !self.is_active()
    }

    /// Get stream duration
    pub fn duration(&self) -> Option<jiff::SignedDuration> {
        self.completed_at().map(|end| end - self.created_at)
    }

    /// Calculate stream progress (0.0 to 1.0)
    pub fn progress(&self) -> f64 {
        match self.state() {
            StreamState::Preparing => 0.0,
            StreamState::Streaming => {
                // Estimate based on frames sent vs expected
                if self.stats.total_frames == 0 {
                    0.1 // Just started
                } else {
                    // Simple heuristic: more frames = more progress
                    (self.stats.total_frames as f64 / 100.0).min(0.9)
                }
            }
            StreamState::Completed => 1.0,
            StreamState::Failed | StreamState::Cancelled => {
                // Partial progress before failure/cancellation
                (self.stats.total_frames as f64 / 100.0).min(0.99)
            }
        }
    }

    /// Update configuration
    pub fn update_config(&mut self, config: StreamConfig) -> DomainResult<()> {
        if !self.is_active() {
            return Err(DomainError::InvalidStreamState(
                "Cannot update config of inactive stream".to_string(),
            ));
        }

        self.config = config;
        self.update_timestamp();
        Ok(())
    }

    /// Private helper: stamp `updated_at` from the clock and return it.
    fn update_timestamp(&mut self) -> Timestamp {
        let now = self.clock.now();
        self.updated_at = now;
        now
    }

    /// Private helper: Record frame creation for stats
    fn record_frame_created(&mut self, frame: &Frame) {
        self.next_sequence += 1;
        self.stats.total_frames += 1;

        let frame_size = frame.estimated_size() as u64;
        self.stats.total_bytes += frame_size;

        match frame.frame_type() {
            crate::entities::frame::FrameType::Skeleton => {
                self.stats.skeleton_frames += 1;
                self.stats.critical_bytes += frame_size;
            }
            crate::entities::frame::FrameType::Patch => {
                self.stats.patch_frames += 1;
                if frame.is_critical() {
                    self.stats.critical_bytes += frame_size;
                } else if frame.is_high_priority() {
                    self.stats.high_priority_bytes += frame_size;
                }
            }
            crate::entities::frame::FrameType::Complete => {
                self.stats.complete_frames += 1;
                self.stats.critical_bytes += frame_size;
            }
            crate::entities::frame::FrameType::Error => {
                self.stats.error_frames += 1;
                self.stats.critical_bytes += frame_size;
            }
        }

        // Update average frame size
        self.stats.average_frame_size =
            self.stats.total_bytes as f64 / self.stats.total_frames as f64;

        self.updated_at = frame.timestamp();
    }

    /// Private helper: Extract patches with priority filtering.
    ///
    /// Walks `data` recursively, emitting one `Set` patch per leaf-level
    /// value (primitives and arrays — objects are traversed without emitting
    /// a patch, since their structure is already conveyed by the skeleton
    /// frame). Each patch is paired with a computed priority so that
    /// [`Self::chunk_patches_for_commit`] can group chunks by maximum
    /// priority. Patches whose priority falls below `threshold` are dropped.
    fn extract_patches(&self, data: &JsonData, threshold: Priority) -> Vec<(FramePatch, Priority)> {
        let heuristic = self.heuristic();
        let mut patches = Vec::new();
        Self::collect_patches(
            data,
            &mut JsonPath::root(),
            threshold,
            &heuristic,
            &mut patches,
        );
        // Sort by priority descending so high-priority patches land in earlier
        // frames within the chunk-based batch layout.
        patches.sort_by_key(|p| core::cmp::Reverse(p.1));
        patches
    }

    /// Recursive walker that emits prioritized patches into `out`, descending
    /// at most [`MAX_DESERIALIZE_DEPTH`] object levels.
    ///
    /// Walks one mutable `path` buffer; a leaf is priced on the borrowed
    /// buffer, and the path and value are cloned only when a patch is emitted.
    fn collect_patches(
        data: &JsonData,
        path: &mut JsonPath,
        threshold: Priority,
        heuristic: &crate::services::PriorityHeuristicConfig,
        out: &mut Vec<(FramePatch, Priority)>,
    ) {
        // An object at the depth bound is emitted whole: the skeleton nulls it
        // (see `services::skeleton`), so this keeps the stream lossless.
        if let JsonData::Object(map) = data
            && path.depth() < MAX_DESERIALIZE_DEPTH
        {
            for (key, value) in map.iter() {
                path.push(PathSegment::Key(key.clone()));
                Self::collect_patches(value, path, threshold, heuristic, out);
                path.pop();
            }
            return;
        }

        let priority = crate::services::compute_priority(
            heuristic,
            path,
            crate::services::ValueShape::from(data),
        );
        if priority >= threshold {
            out.push((FramePatch::set(path.clone(), data.clone()), priority));
        }
    }

    /// Build the priority heuristic for one extraction run.
    ///
    /// The per-stream `priority_rules` map is mapped onto
    /// [`PriorityHeuristicConfig::overrides`] so that user-provided rules keep
    /// winning over the shared heuristic. Built once per
    /// [`Self::extract_patches`] call and fed to
    /// [`crate::services::compute_priority`], the same entry point the
    /// WebAssembly bindings use; see #242 for the divergence this resolves.
    fn heuristic(&self) -> crate::services::PriorityHeuristicConfig {
        let mut cfg = crate::services::PriorityHeuristicConfig::default();
        if !self.config.priority_rules.is_empty() {
            cfg.overrides = self.config.priority_rules.clone();
        }
        cfg
    }

    /// Group prioritized patches into per-frame chunks without constructing
    /// any [`Frame`] or mutating stream state.
    ///
    /// Each returned chunk's priority is the maximum priority of the patches
    /// it contains, so per-frame ordering downstream reflects the most
    /// important content the frame will carry. Pure and side-effect-free so a
    /// caller (e.g. a cross-stream priority batch) can chunk candidates from
    /// several streams, decide which survive truncation, and only then
    /// commit the survivors via [`Self::finalize_patch_frame`] — see that
    /// method's docs.
    ///
    /// # Examples
    ///
    /// ```
    /// use pjson_rs_domain::entities::Stream;
    /// use pjson_rs_domain::entities::frame::FramePatch;
    /// use pjson_rs_domain::value_objects::{JsonData, JsonPath, Priority, SessionId};
    ///
    /// let mut stream = Stream::new(
    ///     SessionId::new(),
    ///     JsonData::Object(Default::default()),
    ///     Default::default(),
    /// );
    /// stream.start_streaming().unwrap();
    ///
    /// let patches = vec![
    ///     (
    ///         FramePatch::set(JsonPath::root(), JsonData::Bool(true)),
    ///         Priority::LOW,
    ///     ),
    ///     (
    ///         FramePatch::set(JsonPath::root(), JsonData::Bool(false)),
    ///         Priority::HIGH,
    ///     ),
    /// ];
    ///
    /// // Chunk without mutating the stream — a caller can inspect priorities
    /// // and pick survivors before any sequence number is spent.
    /// let mut chunks = Stream::chunk_patches_for_commit(patches, 2);
    /// assert_eq!(chunks.len(), 2);
    /// chunks.sort_by_key(|(priority, _)| std::cmp::Reverse(*priority));
    ///
    /// // Only commit the highest-priority chunk.
    /// let (priority, frame_patches) = chunks.remove(0);
    /// let frame = stream
    ///     .finalize_patch_frame(priority, frame_patches)
    ///     .unwrap();
    /// assert_eq!(frame.priority(), Priority::HIGH);
    /// assert_eq!(stream.stats().total_frames, 1);
    /// ```
    pub fn chunk_patches_for_commit(
        patches: Vec<(FramePatch, Priority)>,
        max_frames: usize,
    ) -> Vec<(Priority, Vec<FramePatch>)> {
        if patches.is_empty() || max_frames == 0 {
            return Vec::new();
        }

        let chunk_size = patches.len().div_ceil(max_frames).max(1);
        let mut chunks = Vec::with_capacity(patches.len().div_ceil(chunk_size));
        let mut remaining = patches.into_iter();

        loop {
            let mut priority: Option<Priority> = None;
            let frame_patches: Vec<FramePatch> = remaining
                .by_ref()
                .take(chunk_size)
                .map(|(patch, patch_priority)| {
                    priority = priority.max(Some(patch_priority));
                    patch
                })
                .collect();
            if frame_patches.is_empty() {
                return chunks;
            }
            chunks.push((priority.unwrap_or(Priority::MEDIUM), frame_patches));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value_objects::PathSegment;

    #[test]
    fn test_stream_creation() {
        let session_id = SessionId::new();
        let source_data = serde_json::json!({
            "users": [
                {"id": 1, "name": "John"},
                {"id": 2, "name": "Jane"}
            ],
            "total": 2
        });

        let stream = Stream::new(
            session_id,
            source_data.clone().into(),
            StreamConfig::default(),
        );

        assert_eq!(stream.session_id(), session_id);
        assert_eq!(stream.state(), &StreamState::Preparing);
        assert!(stream.is_active());
        assert!(!stream.is_finished());
        assert_eq!(stream.progress(), 0.0);
    }

    #[test]
    fn test_stream_state_transitions() {
        let session_id = SessionId::new();
        let source_data = serde_json::json!({});
        let mut stream = Stream::new(session_id, source_data.into(), StreamConfig::default());

        // Start streaming
        assert!(stream.start_streaming().is_ok());
        assert_eq!(stream.state(), &StreamState::Streaming);

        // Complete stream
        assert!(stream.complete().is_ok());
        assert_eq!(stream.state(), &StreamState::Completed);
        assert!(stream.is_finished());
        assert_eq!(stream.progress(), 1.0);
    }

    #[test]
    fn test_invalid_state_transitions() {
        let session_id = SessionId::new();
        let source_data = serde_json::json!({});
        let mut stream = Stream::new(session_id, source_data.into(), StreamConfig::default());

        // Cannot complete from preparing state
        assert!(stream.complete().is_err());

        // Start and complete
        assert!(stream.start_streaming().is_ok());
        assert!(stream.complete().is_ok());

        // Cannot start again from completed state
        assert!(stream.start_streaming().is_err());
    }

    #[test]
    fn test_frame_creation() {
        let session_id = SessionId::new();
        let source_data = serde_json::json!({
            "test": "data"
        });
        let mut stream = Stream::new(session_id, source_data.into(), StreamConfig::default());

        // Cannot create frames before streaming
        assert!(stream.create_skeleton_frame().is_err());

        // Start streaming and create skeleton
        assert!(stream.start_streaming().is_ok());
        let skeleton = stream
            .create_skeleton_frame()
            .expect("Failed to create skeleton frame in test");

        assert_eq!(
            skeleton.frame_type(),
            &crate::entities::frame::FrameType::Skeleton
        );
        assert_eq!(skeleton.sequence(), 1);
        assert_eq!(stream.stats().skeleton_frames, 1);
    }

    #[test]
    fn test_stream_metadata() {
        let session_id = SessionId::new();
        let source_data = serde_json::json!({});
        let mut stream = Stream::new(session_id, source_data.into(), StreamConfig::default());

        stream.add_metadata("source".to_string(), "api".to_string());
        stream.add_metadata("version".to_string(), "1.0".to_string());

        assert_eq!(stream.metadata().len(), 2);
        assert_eq!(stream.metadata().get("source"), Some(&"api".to_string()));
    }

    #[test]
    fn test_create_patch_frames_emits_frames_for_typical_payload() {
        let session_id = SessionId::new();
        let source_data = serde_json::json!({
            "id": "abc-123",
            "name": "Alice",
            "items": [1, 2, 3]
        });
        let mut stream = Stream::new(session_id, source_data.into(), StreamConfig::default());

        stream
            .start_streaming()
            .expect("stream must enter streaming state");

        let frames = stream
            .create_patch_frames(Priority::BACKGROUND, 16)
            .expect("frame generation must succeed");

        assert!(
            !frames.is_empty(),
            "extract_patches must produce at least one patch for non-empty source data"
        );

        let id_frame_priority_max = frames
            .iter()
            .map(|f| f.priority())
            .max()
            .expect("non-empty frames must have a max priority");
        assert!(
            id_frame_priority_max >= Priority::CRITICAL,
            "frames carrying the `id` field must surface at critical priority"
        );
    }

    #[test]
    fn test_create_patch_frames_filters_below_threshold() {
        let session_id = SessionId::new();
        // `analytics` is forced to BACKGROUND priority by the heuristic, so
        // a CRITICAL threshold filters everything out.
        let source_data = serde_json::json!({
            "analytics": {"clicks": 1, "views": 2}
        });
        let mut stream = Stream::new(session_id, source_data.into(), StreamConfig::default());

        stream.start_streaming().expect("stream starts");
        let frames = stream
            .create_patch_frames(Priority::CRITICAL, 8)
            .expect("frame generation must succeed");

        assert!(
            frames.is_empty(),
            "patches below the priority threshold must be dropped"
        );
    }

    #[test]
    fn test_create_patch_frames_uses_max_priority_per_chunk() {
        let session_id = SessionId::new();
        // Mix of CRITICAL (`id`), HIGH (`name`), and BACKGROUND (`logs`).
        let source_data = serde_json::json!({
            "id": "x",
            "name": "y",
            "logs": "z"
        });
        let mut stream = Stream::new(session_id, source_data.into(), StreamConfig::default());

        stream.start_streaming().expect("stream starts");
        // Force a single chunk so we can assert max-priority aggregation.
        let frames = stream
            .create_patch_frames(Priority::BACKGROUND, 1)
            .expect("frame generation must succeed");

        assert_eq!(frames.len(), 1, "max_frames=1 must yield a single frame");
        assert_eq!(
            frames[0].priority(),
            Priority::CRITICAL,
            "frame priority must reflect the highest-priority patch in the chunk"
        );
    }

    fn streaming(data: serde_json::Value) -> Stream {
        let mut stream = Stream::new(SessionId::new(), data.into(), StreamConfig::default());
        stream.start_streaming().expect("stream starts");
        stream
    }

    #[test]
    fn test_extract_patches_address_unencodable_keys_without_a_root_set() {
        let stream = streaming(serde_json::json!({"a.b": {"c": 1}, "ok": 2}));

        let patches = stream
            .extract_prioritized_patches(Priority::BACKGROUND)
            .expect("patches");

        let mut paths: Vec<String> = patches.iter().map(|(p, _)| p.path().to_string()).collect();
        paths.sort();
        assert_eq!(paths, ["$.ok", r#"$["a.b"].c"#]);
    }

    /// Applies the skeleton, then only the extracted `Set` patches in order, with
    /// the reconstructor's rules (a parent must already exist).
    fn reconstruct(data: serde_json::Value, threshold: Priority) -> serde_json::Value {
        let stream = streaming(data.clone());
        let patches = stream
            .extract_prioritized_patches(threshold)
            .expect("patches");
        let skeleton = crate::services::skeleton(&data.into(), MAX_DESERIALIZE_DEPTH);
        let mut state = serde_json::to_value(&skeleton).expect("JsonData serializes");
        for (patch, _) in &patches {
            let value = serde_json::to_value(patch.value()).expect("JsonData serializes");
            let Some((last, parents)) = patch.path().segments().split_last() else {
                state = value;
                continue;
            };
            let mut node = &mut state;
            for segment in parents {
                node = match segment {
                    PathSegment::Key(key) => node.get_mut(key.as_str()),
                    PathSegment::Index(index) => node.get_mut(*index),
                }
                .expect("parent exists in the skeleton");
            }
            match last {
                PathSegment::Key(key) => {
                    node.as_object_mut()
                        .expect("object parent")
                        .insert(key.clone(), value);
                }
                PathSegment::Index(index) => node[*index] = value,
            }
        }
        state
    }

    #[test]
    fn test_skeleton_and_patches_reconstruct_unencodable_keys() {
        for data in [
            serde_json::json!({"a.b": {"c": 1}, "arr": [1, 2], "n": {"x": {"y": 2}, "z": [3]}, "ok": 5}),
            serde_json::json!({"n": {"a[0]": [1], "deep": {"k]": {"v": 1}, "w": "s"}}, "list": []}),
            serde_json::json!({"": null, "o": {}, "p": {"q": {"r": true}}}),
            serde_json::json!({"rows": [{"a.b": 1}, {"ok": {"x[": 2}}]}),
        ] {
            assert_eq!(reconstruct(data.clone(), Priority::BACKGROUND), data);
        }
    }

    #[test]
    fn test_skeleton_does_not_ship_unencodable_subtrees_verbatim() {
        let stream = streaming(serde_json::json!({"a.b": 7, "": {"x": 8}}));
        let data = stream.source_data.clone().expect("source data");

        let skeleton =
            serde_json::to_value(crate::services::skeleton(&data, MAX_DESERIALIZE_DEPTH))
                .expect("serializes");

        assert_eq!(skeleton, serde_json::json!({"a.b": 0, "": {"x": 0}}));
    }

    #[test]
    fn test_chunk_patches_huge_max_frames_sizes_output_by_chunk_count() {
        let patches = vec![(
            FramePatch::set(JsonPath::root(), JsonData::Null),
            Priority::HIGH,
        )];

        let chunks = Stream::chunk_patches_for_commit(patches, usize::MAX);

        assert_eq!(chunks.len(), 1);
    }
}
