//! Stream entity representing a prioritized data stream

use crate::{
    DomainError, DomainResult,
    entities::{Frame, frame::FramePatch},
    value_objects::{JsonData, JsonPath, PathSegment, Priority, SessionId, StreamId},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamConfig {
    /// Maximum frame size in bytes
    pub max_frame_size: usize,
    /// Maximum frames per batch
    pub max_frames_per_batch: usize,
    /// Compression settings
    pub enable_compression: bool,
    /// Custom priority rules
    #[serde(with = "crate::serde_support::priority_map")]
    pub priority_rules: HashMap<String, Priority>,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            max_frame_size: 64 * 1024, // 64KB
            max_frames_per_batch: 10,
            enable_compression: true,
            priority_rules: HashMap::new(),
        }
    }
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

/// Priority data stream entity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stream {
    #[serde(with = "crate::serde_support::stream_id")]
    id: StreamId,
    #[serde(with = "crate::serde_support::session_id")]
    session_id: SessionId,
    state: StreamState,
    config: StreamConfig,
    stats: StreamStats,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
    next_sequence: u64,
    source_data: Option<JsonData>,
    metadata: HashMap<String, String>,
}

impl Stream {
    /// Create new stream
    pub fn new(session_id: SessionId, source_data: JsonData, config: StreamConfig) -> Self {
        let now = Utc::now();

        Self {
            id: StreamId::new(),
            session_id,
            state: StreamState::Preparing,
            config,
            stats: StreamStats::default(),
            created_at: now,
            updated_at: now,
            completed_at: None,
            next_sequence: 1,
            source_data: Some(source_data),
            metadata: HashMap::new(),
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
        &self.state
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
    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    /// Get last update timestamp
    pub fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }

    /// Get completion timestamp
    pub fn completed_at(&self) -> Option<DateTime<Utc>> {
        self.completed_at
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
        match self.state {
            StreamState::Preparing => {
                self.state = StreamState::Streaming;
                self.update_timestamp();
                Ok(())
            }
            _ => Err(DomainError::InvalidStateTransition(format!(
                "Cannot start streaming from state: {:?}",
                self.state
            ))),
        }
    }

    /// Complete stream successfully
    pub fn complete(&mut self) -> DomainResult<()> {
        match self.state {
            StreamState::Streaming => {
                self.state = StreamState::Completed;
                self.completed_at = Some(Utc::now());
                self.update_timestamp();
                Ok(())
            }
            _ => Err(DomainError::InvalidStateTransition(format!(
                "Cannot complete stream from state: {:?}",
                self.state
            ))),
        }
    }

    /// Fail stream with error
    pub fn fail(&mut self, error: String) -> DomainResult<()> {
        match self.state {
            StreamState::Preparing | StreamState::Streaming => {
                self.state = StreamState::Failed;
                self.completed_at = Some(Utc::now());
                self.add_metadata("error".to_string(), error);
                Ok(())
            }
            _ => Err(DomainError::InvalidStateTransition(format!(
                "Cannot fail stream from state: {:?}",
                self.state
            ))),
        }
    }

    /// Cancel stream
    pub fn cancel(&mut self) -> DomainResult<()> {
        match self.state {
            StreamState::Preparing | StreamState::Streaming => {
                self.state = StreamState::Cancelled;
                self.completed_at = Some(Utc::now());
                self.update_timestamp();
                Ok(())
            }
            _ => Err(DomainError::InvalidStateTransition(format!(
                "Cannot cancel stream from state: {:?}",
                self.state
            ))),
        }
    }

    /// Generate skeleton frame for the stream
    pub fn create_skeleton_frame(&mut self) -> DomainResult<Frame> {
        if !matches!(self.state, StreamState::Streaming) {
            return Err(DomainError::InvalidStreamState(
                "Stream must be in streaming state to create frames".to_string(),
            ));
        }

        let skeleton_data = self.source_data.as_ref().ok_or_else(|| {
            DomainError::InvalidStreamState("No source data available for skeleton".to_string())
        })?;

        let skeleton = self.generate_skeleton(skeleton_data)?;
        let frame = Frame::skeleton(self.id, self.next_sequence, skeleton);

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
        if !matches!(self.state, StreamState::Streaming) {
            return Err(DomainError::InvalidStreamState(
                "Stream must be in streaming state to create frames".to_string(),
            ));
        }

        let source_data = self.source_data.as_ref().ok_or_else(|| {
            DomainError::InvalidStreamState("No source data available for patches".to_string())
        })?;

        self.extract_patches(source_data, priority_threshold)
    }

    /// Turn already-extracted prioritized patches (from
    /// [`Self::extract_prioritized_patches`]) into frames and commit their
    /// bookkeeping (`next_sequence`, `stats`) — the other half of
    /// [`Self::create_patch_frames`].
    ///
    /// `max_frames` only bounds the number of frames returned, not the work
    /// done to produce them: every patch in `patches` is still cloned into
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
        if !matches!(self.state, StreamState::Streaming) {
            return Err(DomainError::InvalidStreamState(
                "Stream must be in streaming state to create frames".to_string(),
            ));
        }

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
        if !matches!(self.state, StreamState::Streaming) {
            return Err(DomainError::InvalidStreamState(
                "Stream must be in streaming state to create frames".to_string(),
            ));
        }

        let frame = Frame::patch(self.id, self.next_sequence, priority, frame_patches)?;
        self.record_frame_created(&frame);
        Ok(frame)
    }

    /// Create completion frame
    pub fn create_completion_frame(&mut self, checksum: Option<String>) -> DomainResult<Frame> {
        if !matches!(self.state, StreamState::Streaming) {
            return Err(DomainError::InvalidStreamState(
                "Stream must be in streaming state to create frames".to_string(),
            ));
        }

        let frame = Frame::complete(self.id, self.next_sequence, checksum);
        self.record_frame_created(&frame);

        Ok(frame)
    }

    /// Check if stream is active
    pub fn is_active(&self) -> bool {
        matches!(self.state, StreamState::Preparing | StreamState::Streaming)
    }

    /// Check if stream is finished
    pub fn is_finished(&self) -> bool {
        matches!(
            self.state,
            StreamState::Completed | StreamState::Failed | StreamState::Cancelled
        )
    }

    /// Get stream duration
    pub fn duration(&self) -> Option<chrono::Duration> {
        self.completed_at.map(|end| end - self.created_at)
    }

    /// Calculate stream progress (0.0 to 1.0)
    pub fn progress(&self) -> f64 {
        match self.state {
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

    /// Private helper: Update timestamp
    fn update_timestamp(&mut self) {
        self.updated_at = Utc::now();
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

        self.update_timestamp();
    }

    /// Private helper: Generate skeleton from source data.
    ///
    /// Values under keys [`JsonPath`] cannot encode are kept verbatim: no patch
    /// can address them, so the skeleton is the only carrier for those subtrees.
    fn generate_skeleton(&self, data: &JsonData) -> DomainResult<JsonData> {
        // Simplified skeleton generation - create empty structure
        match data {
            JsonData::Object(obj) => {
                let mut skeleton = HashMap::new();
                for (key, value) in obj.iter() {
                    let skeleton_value = if !JsonPath::is_valid_key(key) {
                        value.clone()
                    } else {
                        match value {
                            JsonData::Array(_) => JsonData::Array(Vec::new()),
                            JsonData::Object(_) => self.generate_skeleton(value)?,
                            JsonData::Integer(_) => JsonData::Integer(0),
                            JsonData::Float(_) => JsonData::Float(0.0),
                            JsonData::String(_) => JsonData::Null,
                            JsonData::Bool(_) => JsonData::Bool(false),
                            JsonData::Null => JsonData::Null,
                        }
                    };
                    skeleton.insert(key.clone(), skeleton_value);
                }
                Ok(JsonData::Object(skeleton))
            }
            JsonData::Array(_) => Ok(JsonData::Array(Vec::new())),
            _ => Ok(JsonData::Null),
        }
    }

    /// Private helper: the part of `data` that no emitted patch can carry.
    ///
    /// Holds the values under [`JsonPath`]-unencodable keys verbatim, empty
    /// objects, and exactly the object parents that those values or an emitted
    /// leaf patch (`emitted`) need to exist. Other leaves and arrays are omitted,
    /// and so are objects whose leaves were all dropped by the threshold, so a
    /// dropped value stays absent instead of appearing as a placeholder.
    /// Returns `None` when nothing under `data` is needed.
    fn unencodable_projection(
        data: &JsonData,
        path: &JsonPath,
        emitted: &HashSet<&[PathSegment]>,
    ) -> Option<HashMap<String, JsonData>> {
        let JsonData::Object(obj) = data else {
            return None;
        };
        let mut projection = HashMap::new();
        let mut needed = false;
        for (key, value) in obj.iter() {
            if !JsonPath::is_valid_key(key) {
                projection.insert(key.clone(), value.clone());
                continue;
            }
            let child = path.append_key(key);
            match value {
                JsonData::Object(children) if children.is_empty() => {
                    projection.insert(key.clone(), value.clone());
                }
                JsonData::Object(_) => {
                    if let Some(nested) = Self::unencodable_projection(value, &child, emitted) {
                        projection.insert(key.clone(), JsonData::Object(nested));
                    }
                }
                _ => needed |= emitted.contains(child.segments()),
            }
        }
        (needed || !projection.is_empty()).then_some(projection)
    }

    /// Private helper: Extract patches with priority filtering.
    ///
    /// Walks `data` recursively, emitting one `Set` patch per leaf-level
    /// value (primitives and arrays — objects are traversed without emitting
    /// a patch, since their structure is already conveyed by the skeleton
    /// frame). Each patch is paired with a computed priority so that
    /// [`Self::chunk_patches_for_commit`] can group chunks by maximum
    /// priority. Patches whose priority falls below `threshold` are dropped.
    ///
    /// When some object key cannot be encoded as a [`JsonPath`] segment, its
    /// subtree has no patch address, so a root `Set` of the object structure with
    /// those subtrees verbatim and no placeholders for other values is inserted
    /// ahead of every other patch.
    /// Its priority is the maximum of `CRITICAL` and every other patch
    /// priority, so the stable sort keeps it first; like any patch it is
    /// dropped when below `threshold`. Each extraction is a self-contained
    /// snapshot: the root `Set` resets earlier deltas, and a cross-stream
    /// truncation can deliver it without the leaf patches that follow.
    fn extract_patches(
        &self,
        data: &JsonData,
        threshold: Priority,
    ) -> DomainResult<Vec<(crate::entities::frame::FramePatch, Priority)>> {
        let mut patches = Vec::new();
        let has_unencodable_keys =
            self.collect_patches(data, &JsonPath::root(), threshold, &mut patches)?;
        if has_unencodable_keys {
            let priority = patches
                .iter()
                .map(|(_, p)| *p)
                .fold(Priority::CRITICAL, Priority::max);
            if priority >= threshold {
                let emitted = patches
                    .iter()
                    .map(|(patch, _)| patch.path.segments())
                    .collect();
                let structure = Self::unencodable_projection(data, &JsonPath::root(), &emitted)
                    .unwrap_or_default();
                let root = FramePatch::set(JsonPath::root(), JsonData::Object(structure));
                patches.insert(0, (root, priority));
            }
        }
        // Sort by priority descending so high-priority patches land in earlier
        // frames within the chunk-based batch layout.
        patches.sort_by_key(|p| core::cmp::Reverse(p.1));
        Ok(patches)
    }

    /// Recursive walker that emits prioritized patches into `out`.
    ///
    /// Returns `true` when any object key at or below `data` cannot be encoded
    /// as a [`JsonPath`] segment; such subtrees are not descended into.
    fn collect_patches(
        &self,
        data: &JsonData,
        path: &JsonPath,
        threshold: Priority,
        out: &mut Vec<(crate::entities::frame::FramePatch, Priority)>,
    ) -> DomainResult<bool> {
        if let JsonData::Object(map) = data {
            let mut has_unencodable_keys = false;
            for (key, value) in map.iter() {
                if !JsonPath::is_valid_key(key) {
                    has_unencodable_keys = true;
                    continue;
                }
                let child_path = path.append_key(key);
                has_unencodable_keys |= self.collect_patches(value, &child_path, threshold, out)?;
            }
            return Ok(has_unencodable_keys);
        }

        let priority = self.compute_priority(path, data);
        if priority >= threshold {
            let patch = crate::entities::frame::FramePatch::set(path.clone(), data.clone());
            out.push((patch, priority));
        }
        Ok(false)
    }

    /// Compute a priority for a patch by delegating to
    /// [`crate::services::compute_priority`].
    ///
    /// The per-stream `priority_rules` map is mapped onto
    /// [`PriorityHeuristicConfig::overrides`] so that user-provided rules keep
    /// winning over the shared heuristic. This is the single entry point used
    /// by both the HTTP transport (via `extract_patches`) and the WebAssembly
    /// bindings; see #242 for the divergence this resolves.
    fn compute_priority(&self, path: &JsonPath, value: &JsonData) -> Priority {
        let mut cfg = crate::services::PriorityHeuristicConfig::default();
        if !self.config.priority_rules.is_empty() {
            cfg.overrides = self.config.priority_rules.clone();
        }
        crate::services::compute_priority(&cfg, path, value)
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

        patches
            .chunks(chunk_size)
            .map(|chunk| {
                let priority = chunk
                    .iter()
                    .map(|(_, p)| *p)
                    .max()
                    .unwrap_or(Priority::MEDIUM);

                let frame_patches: Vec<FramePatch> =
                    chunk.iter().map(|(patch, _)| patch.clone()).collect();

                (priority, frame_patches)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_extract_patches_root_set_carries_unencodable_subtree() {
        let stream = streaming(serde_json::json!({"a.b": {"c": 1}, "ok": 2}));

        let patches = stream
            .extract_prioritized_patches(Priority::BACKGROUND)
            .expect("patches");

        let (root, _) = patches.first().expect("root set first");
        assert_eq!(root.path, JsonPath::root());
        let expected: JsonData = serde_json::json!({"a.b": {"c": 1}}).into();
        assert_eq!(root.value, expected);
    }

    #[test]
    fn test_extract_patches_root_set_survives_critical_threshold() {
        let stream = streaming(serde_json::json!({"a.b": 1, "logs": "z"}));

        let patches = stream
            .extract_prioritized_patches(Priority::CRITICAL)
            .expect("patches");

        let (root, priority) = patches.first().expect("root set first");
        assert_eq!(root.path, JsonPath::root());
        assert_eq!(*priority, Priority::CRITICAL);
    }

    #[test]
    fn test_extract_patches_no_root_set_for_encodable_keys() {
        let stream = streaming(serde_json::json!({"a": {"b": 1}, "c": [1]}));

        let patches = stream
            .extract_prioritized_patches(Priority::BACKGROUND)
            .expect("patches");

        assert!(patches.iter().all(|(p, _)| !p.path.segments().is_empty()));
    }

    /// Applies only the extracted `Set` patches, in order, to an initial `{}`
    /// with the reconstructor's rules (a parent must already exist): the HTTP
    /// path, where no skeleton frame is sent.
    fn apply_patches_only(patches: &[(FramePatch, Priority)]) -> serde_json::Value {
        let mut state = serde_json::json!({});
        for (patch, _) in patches {
            let value = serde_json::to_value(&patch.value).expect("JsonData serializes");
            let Some((last, parents)) = patch.path.segments().split_last() else {
                state = value;
                continue;
            };
            let mut node = &mut state;
            for segment in parents {
                node = match segment {
                    PathSegment::Key(key) => node.get_mut(key.as_str()),
                    PathSegment::Index(index) => node.get_mut(*index),
                }
                .expect("parent exists: root Set carries the structure");
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

    fn reconstruct_without_skeleton(data: serde_json::Value) -> serde_json::Value {
        let patches = streaming(data)
            .extract_prioritized_patches(Priority::BACKGROUND)
            .expect("patches");
        apply_patches_only(&patches)
    }

    #[test]
    fn test_patches_only_reconstruct_unencodable_keys_next_to_arrays_and_nested_objects() {
        for data in [
            serde_json::json!({"a.b": {"c": 1}, "arr": [1, 2], "n": {"x": {"y": 2}, "z": [3]}, "ok": 5}),
            serde_json::json!({"n": {"a[0]": [1], "deep": {"k]": {"v": 1}, "w": "s"}}, "list": []}),
            serde_json::json!({"": null, "o": {}, "p": {"q": {"r": true}}}),
        ] {
            assert_eq!(reconstruct_without_skeleton(data.clone()), data);
        }
    }

    #[test]
    fn test_patches_only_keep_unencodable_keys_inside_array_elements() {
        let data = serde_json::json!({"rows": [{"a.b": 1}, {"ok": {"x[": 2}}]});

        let patches = streaming(data.clone())
            .extract_prioritized_patches(Priority::BACKGROUND)
            .expect("patches");

        assert!(
            patches.iter().all(|(p, _)| !p.path.segments().is_empty()),
            "array elements travel inside their array's Set, no root Set"
        );
        assert_eq!(apply_patches_only(&patches), data);
    }

    #[test]
    fn test_patches_only_object_with_all_leaves_dropped_stays_absent() {
        let data = serde_json::json!({"a.b": 1, "n": {"logs": "z"}, "e": {}});
        let patches = streaming(data)
            .extract_prioritized_patches(Priority::CRITICAL)
            .expect("patches");

        let state = apply_patches_only(&patches);

        assert_eq!(state, serde_json::json!({"a.b": 1, "e": {}}));
    }

    #[test]
    fn test_patches_only_dropped_leaves_stay_absent() {
        let data = serde_json::json!({"a.b": 1, "n": {"id": 1, "logs": "z"}, "logs": [1]});
        let patches = streaming(data)
            .extract_prioritized_patches(Priority::CRITICAL)
            .expect("patches");

        let state = apply_patches_only(&patches);

        assert_eq!(state, serde_json::json!({"a.b": 1, "n": {"id": 1}}));
    }
}
