//! Stream entity representing a prioritized data stream

use crate::{
    DomainError, DomainResult,
    entities::Frame,
    value_objects::{JsonData, Priority, SessionId, StreamId},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Custom serde for SessionId within entities
mod serde_session_id {
    use crate::value_objects::SessionId;
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

/// Custom serde for StreamId within entities
mod serde_stream_id {
    use crate::value_objects::StreamId;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(id: &StreamId, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        id.as_uuid().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<StreamId, D::Error>
    where
        D: Deserializer<'de>,
    {
        let uuid = uuid::Uuid::deserialize(deserializer)?;
        Ok(StreamId::from_uuid(uuid))
    }
}

/// Custom serde for Priority within entities
#[allow(dead_code)]
mod serde_priority {
    use crate::value_objects::Priority;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(priority: &Priority, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        priority.value().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Priority, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u8::deserialize(deserializer)?;
        Priority::new(value).map_err(serde::de::Error::custom)
    }
}

/// Custom serde for HashMap<String, Priority>
mod serde_priority_map {
    use crate::value_objects::Priority;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::HashMap;

    pub fn serialize<S>(map: &HashMap<String, Priority>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let u8_map: HashMap<String, u8> = map.iter().map(|(k, v)| (k.clone(), v.value())).collect();
        u8_map.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<HashMap<String, Priority>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let u8_map: HashMap<String, u8> = HashMap::deserialize(deserializer)?;
        u8_map
            .into_iter()
            .map(|(k, v)| {
                Priority::new(v)
                    .map(|p| (k, p))
                    .map_err(serde::de::Error::custom)
            })
            .collect()
    }
}

/// Stream state in its lifecycle
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    #[serde(with = "serde_priority_map")]
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
    #[serde(with = "serde_stream_id")]
    id: StreamId,
    #[serde(with = "serde_session_id")]
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
        if !matches!(self.state, StreamState::Streaming) {
            return Err(DomainError::InvalidStreamState(
                "Stream must be in streaming state to create frames".to_string(),
            ));
        }

        let source_data = self.source_data.as_ref().ok_or_else(|| {
            DomainError::InvalidStreamState("No source data available for patches".to_string())
        })?;

        let patches = self.extract_patches(source_data, priority_threshold)?;
        let frames = self.batch_patches_into_frames(patches, max_frames)?;

        for frame in &frames {
            self.record_frame_created(frame);
        }

        Ok(frames)
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

    /// Private helper: Generate skeleton from source data
    fn generate_skeleton(&self, data: &JsonData) -> DomainResult<JsonData> {
        // Simplified skeleton generation - create empty structure
        match data {
            JsonData::Object(obj) => {
                let mut skeleton = HashMap::new();
                for (key, value) in obj.iter() {
                    skeleton.insert(
                        key.clone(),
                        match value {
                            JsonData::Array(_) => JsonData::Array(Vec::new()),
                            JsonData::Object(_) => self.generate_skeleton(value)?,
                            JsonData::Integer(_) => JsonData::Integer(0),
                            JsonData::Float(_) => JsonData::Float(0.0),
                            JsonData::String(_) => JsonData::Null,
                            JsonData::Bool(_) => JsonData::Bool(false),
                            JsonData::Null => JsonData::Null,
                        },
                    );
                }
                Ok(JsonData::Object(skeleton))
            }
            JsonData::Array(_) => Ok(JsonData::Array(Vec::new())),
            _ => Ok(JsonData::Null),
        }
    }

    /// Private helper: Extract patches with priority filtering
    fn extract_patches(
        &self,
        _data: &JsonData,
        _threshold: Priority,
    ) -> DomainResult<Vec<crate::entities::frame::FramePatch>> {
        // Simplified patch extraction - would need more sophisticated logic
        Ok(Vec::new())
    }

    /// Private helper: Batch patches into frames
    fn batch_patches_into_frames(
        &mut self,
        patches: Vec<crate::entities::frame::FramePatch>,
        max_frames: usize,
    ) -> DomainResult<Vec<Frame>> {
        if patches.is_empty() {
            return Ok(Vec::new());
        }

        let mut frames = Vec::new();
        let chunk_size = patches.len().div_ceil(max_frames);

        for patch_chunk in patches.chunks(chunk_size) {
            let priority = patch_chunk
                .iter()
                .map(|_| Priority::MEDIUM) // Simplified - would calculate from patch content
                .max()
                .unwrap_or(Priority::MEDIUM);

            let frame = Frame::patch(self.id, self.next_sequence, priority, patch_chunk.to_vec())?;

            frames.push(frame);
        }

        Ok(frames)
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
}
