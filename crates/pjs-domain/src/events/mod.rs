//! Domain events for event sourcing and integration

use crate::value_objects::{SessionId, StreamId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Session state in its lifecycle
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionState {
    /// Session is being initialized
    Initializing,
    /// Session is active with streams
    Active,
    /// Session is gracefully closing
    Closing,
    /// Session completed successfully
    Completed,
    /// Session failed with error
    Failed,
}

/// Domain events that represent business-relevant state changes
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event_type", rename_all = "snake_case")]
pub enum DomainEvent {
    /// Session was activated and is ready to accept streams
    SessionActivated {
        /// ID of the activated session
        session_id: SessionId,
        /// When the session was activated
        timestamp: DateTime<Utc>,
    },

    /// Session was closed gracefully
    SessionClosed {
        /// ID of the closed session
        session_id: SessionId,
        /// When the session was closed
        timestamp: DateTime<Utc>,
    },

    /// Session expired due to timeout
    SessionExpired {
        /// ID of the expired session
        session_id: SessionId,
        /// When the session expired
        timestamp: DateTime<Utc>,
    },

    /// Session was forcefully closed due to timeout
    SessionTimedOut {
        /// ID of the timed out session
        session_id: SessionId,
        /// State the session was in before timeout
        original_state: SessionState,
        /// Duration in seconds before timeout occurred
        timeout_duration: u64,
        /// When the timeout occurred
        timestamp: DateTime<Utc>,
    },

    /// Session timeout was extended
    SessionTimeoutExtended {
        /// ID of the session with extended timeout
        session_id: SessionId,
        /// Additional seconds added to the timeout
        additional_seconds: u64,
        /// New expiration timestamp
        new_expires_at: DateTime<Utc>,
        /// When the extension was applied
        timestamp: DateTime<Utc>,
    },

    /// New stream was created in the session
    StreamCreated {
        /// ID of the session containing the stream
        session_id: SessionId,
        /// ID of the newly created stream
        stream_id: StreamId,
        /// When the stream was created
        timestamp: DateTime<Utc>,
    },

    /// Stream started sending data
    StreamStarted {
        /// ID of the session containing the stream
        session_id: SessionId,
        /// ID of the stream that started
        stream_id: StreamId,
        /// When the stream started
        timestamp: DateTime<Utc>,
    },

    /// Stream completed successfully
    StreamCompleted {
        /// ID of the session containing the stream
        session_id: SessionId,
        /// ID of the completed stream
        stream_id: StreamId,
        /// When the stream completed
        timestamp: DateTime<Utc>,
    },

    /// Stream failed with error
    StreamFailed {
        /// ID of the session containing the stream
        session_id: SessionId,
        /// ID of the failed stream
        stream_id: StreamId,
        /// Error message describing the failure
        error: String,
        /// When the stream failed
        timestamp: DateTime<Utc>,
    },

    /// Stream was cancelled
    StreamCancelled {
        /// ID of the session containing the stream
        session_id: SessionId,
        /// ID of the cancelled stream
        stream_id: StreamId,
        /// When the stream was cancelled
        timestamp: DateTime<Utc>,
    },

    /// Skeleton frame was generated for a stream
    SkeletonGenerated {
        /// ID of the session containing the stream
        session_id: SessionId,
        /// ID of the stream that generated the skeleton
        stream_id: StreamId,
        /// Size of the skeleton frame in bytes
        frame_size_bytes: u64,
        /// When the skeleton was generated
        timestamp: DateTime<Utc>,
    },

    /// Patch frames were generated for a stream
    PatchFramesGenerated {
        /// ID of the session containing the stream
        session_id: SessionId,
        /// ID of the stream that generated patches
        stream_id: StreamId,
        /// Number of patch frames generated
        frame_count: usize,
        /// Total size of all patches in bytes
        total_bytes: u64,
        /// Highest priority level among the patches
        highest_priority: u8,
        /// When the patches were generated
        timestamp: DateTime<Utc>,
    },

    /// Multiple frames were batched for efficient sending
    FramesBatched {
        /// ID of the session containing the frames
        session_id: SessionId,
        /// Number of frames in the batch
        frame_count: usize,
        /// When the batch was created
        timestamp: DateTime<Utc>,
    },

    /// Priority threshold was adjusted for adaptive streaming
    PriorityThresholdAdjusted {
        /// ID of the session with adjusted threshold
        session_id: SessionId,
        /// Previous priority threshold value
        old_threshold: u8,
        /// New priority threshold value
        new_threshold: u8,
        /// Reason for the adjustment
        reason: String,
        /// When the threshold was adjusted
        timestamp: DateTime<Utc>,
    },

    /// Stream configuration was updated
    StreamConfigUpdated {
        /// ID of the session containing the stream
        session_id: SessionId,
        /// ID of the stream with updated configuration
        stream_id: StreamId,
        /// When the configuration was updated
        timestamp: DateTime<Utc>,
    },

    /// Performance metrics were recorded
    PerformanceMetricsRecorded {
        /// ID of the session being measured
        session_id: SessionId,
        /// Recorded performance metrics
        metrics: PerformanceMetrics,
        /// When the metrics were recorded
        timestamp: DateTime<Utc>,
    },
}

/// Performance metrics for monitoring and optimization
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    /// Number of frames transmitted per second
    pub frames_per_second: f64,
    /// Number of bytes transmitted per second
    pub bytes_per_second: f64,
    /// Average size of frames in bytes
    pub average_frame_size: f64,
    /// Distribution of frames across priority levels
    pub priority_distribution: PriorityDistribution,
    /// Network latency in milliseconds, if available
    pub latency_ms: Option<u64>,
}

/// Distribution of frames by priority level
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PriorityDistribution {
    /// Number of critical priority frames
    pub critical_frames: u64,
    /// Number of high priority frames
    pub high_frames: u64,
    /// Number of medium priority frames
    pub medium_frames: u64,
    /// Number of low priority frames
    pub low_frames: u64,
    /// Number of background priority frames
    pub background_frames: u64,
}

impl PriorityDistribution {
    /// Create new empty distribution
    pub fn new() -> Self {
        Self::default()
    }

    /// Get total frames count
    pub fn total_frames(&self) -> u64 {
        self.critical_frames
            + self.high_frames
            + self.medium_frames
            + self.low_frames
            + self.background_frames
    }

    /// Convert to percentages (0.0-1.0)
    pub fn as_percentages(&self) -> PriorityPercentages {
        let total = self.total_frames() as f64;
        if total == 0.0 {
            return PriorityPercentages::default();
        }

        PriorityPercentages {
            critical: self.critical_frames as f64 / total,
            high: self.high_frames as f64 / total,
            medium: self.medium_frames as f64 / total,
            low: self.low_frames as f64 / total,
            background: self.background_frames as f64 / total,
        }
    }

    /// Convert from count-based version
    pub fn from_counts(
        critical_count: u64,
        high_count: u64,
        medium_count: u64,
        low_count: u64,
        background_count: u64,
    ) -> Self {
        Self {
            critical_frames: critical_count,
            high_frames: high_count,
            medium_frames: medium_count,
            low_frames: low_count,
            background_frames: background_count,
        }
    }
}

/// Priority distribution as percentages (for demos and visualization)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriorityPercentages {
    /// Critical priority percentage (0.0-1.0)
    pub critical: f64,
    /// High priority percentage (0.0-1.0)
    pub high: f64,
    /// Medium priority percentage (0.0-1.0)
    pub medium: f64,
    /// Low priority percentage (0.0-1.0)
    pub low: f64,
    /// Background priority percentage (0.0-1.0)
    pub background: f64,
}

impl Default for PriorityPercentages {
    fn default() -> Self {
        Self {
            critical: 0.0,
            high: 0.0,
            medium: 0.0,
            low: 0.0,
            background: 0.0,
        }
    }
}

impl DomainEvent {
    /// Get the session ID associated with this event
    pub fn session_id(&self) -> SessionId {
        match self {
            Self::SessionActivated { session_id, .. } => *session_id,
            Self::SessionClosed { session_id, .. } => *session_id,
            Self::SessionExpired { session_id, .. } => *session_id,
            Self::StreamCreated { session_id, .. } => *session_id,
            Self::StreamStarted { session_id, .. } => *session_id,
            Self::StreamCompleted { session_id, .. } => *session_id,
            Self::StreamFailed { session_id, .. } => *session_id,
            Self::StreamCancelled { session_id, .. } => *session_id,
            Self::SkeletonGenerated { session_id, .. } => *session_id,
            Self::PatchFramesGenerated { session_id, .. } => *session_id,
            Self::FramesBatched { session_id, .. } => *session_id,
            Self::PriorityThresholdAdjusted { session_id, .. } => *session_id,
            Self::StreamConfigUpdated { session_id, .. } => *session_id,
            Self::PerformanceMetricsRecorded { session_id, .. } => *session_id,
            Self::SessionTimedOut { session_id, .. } => *session_id,
            Self::SessionTimeoutExtended { session_id, .. } => *session_id,
        }
    }

    /// Get the stream ID if this is a stream-specific event
    pub fn stream_id(&self) -> Option<StreamId> {
        match self {
            Self::StreamCreated { stream_id, .. } => Some(*stream_id),
            Self::StreamStarted { stream_id, .. } => Some(*stream_id),
            Self::StreamCompleted { stream_id, .. } => Some(*stream_id),
            Self::StreamFailed { stream_id, .. } => Some(*stream_id),
            Self::StreamCancelled { stream_id, .. } => Some(*stream_id),
            Self::SkeletonGenerated { stream_id, .. } => Some(*stream_id),
            Self::PatchFramesGenerated { stream_id, .. } => Some(*stream_id),
            Self::StreamConfigUpdated { stream_id, .. } => Some(*stream_id),
            _ => None,
        }
    }

    /// Get the timestamp of this event
    pub fn timestamp(&self) -> DateTime<Utc> {
        match self {
            Self::SessionActivated { timestamp, .. } => *timestamp,
            Self::SessionClosed { timestamp, .. } => *timestamp,
            Self::SessionExpired { timestamp, .. } => *timestamp,
            Self::StreamCreated { timestamp, .. } => *timestamp,
            Self::StreamStarted { timestamp, .. } => *timestamp,
            Self::StreamCompleted { timestamp, .. } => *timestamp,
            Self::StreamFailed { timestamp, .. } => *timestamp,
            Self::StreamCancelled { timestamp, .. } => *timestamp,
            Self::SkeletonGenerated { timestamp, .. } => *timestamp,
            Self::PatchFramesGenerated { timestamp, .. } => *timestamp,
            Self::FramesBatched { timestamp, .. } => *timestamp,
            Self::PriorityThresholdAdjusted { timestamp, .. } => *timestamp,
            Self::StreamConfigUpdated { timestamp, .. } => *timestamp,
            Self::PerformanceMetricsRecorded { timestamp, .. } => *timestamp,
            Self::SessionTimedOut { timestamp, .. } => *timestamp,
            Self::SessionTimeoutExtended { timestamp, .. } => *timestamp,
        }
    }

    /// Get the event type as a string
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::SessionActivated { .. } => "session_activated",
            Self::SessionClosed { .. } => "session_closed",
            Self::SessionExpired { .. } => "session_expired",
            Self::StreamCreated { .. } => "stream_created",
            Self::StreamStarted { .. } => "stream_started",
            Self::StreamCompleted { .. } => "stream_completed",
            Self::StreamFailed { .. } => "stream_failed",
            Self::StreamCancelled { .. } => "stream_cancelled",
            Self::SkeletonGenerated { .. } => "skeleton_generated",
            Self::PatchFramesGenerated { .. } => "patch_frames_generated",
            Self::FramesBatched { .. } => "frames_batched",
            Self::PriorityThresholdAdjusted { .. } => "priority_threshold_adjusted",
            Self::StreamConfigUpdated { .. } => "stream_config_updated",
            Self::PerformanceMetricsRecorded { .. } => "performance_metrics_recorded",
            Self::SessionTimedOut { .. } => "session_timed_out",
            Self::SessionTimeoutExtended { .. } => "session_timeout_extended",
        }
    }

    /// Check if this is a critical event that requires immediate attention
    pub fn is_critical(&self) -> bool {
        matches!(
            self,
            Self::StreamFailed { .. } | Self::SessionExpired { .. }
        )
    }

    /// Check if this is an error event
    pub fn is_error(&self) -> bool {
        matches!(self, Self::StreamFailed { .. })
    }

    /// Check if this is a completion event
    pub fn is_completion(&self) -> bool {
        matches!(
            self,
            Self::StreamCompleted { .. } | Self::SessionClosed { .. }
        )
    }
}

/// Event sourcing support for storing and retrieving domain events
pub trait EventStore {
    /// Append events to the store
    ///
    /// # Errors
    ///
    /// Returns an error if events cannot be persisted
    fn append_events(&mut self, events: Vec<DomainEvent>) -> Result<(), String>;

    /// Get events for a specific session
    ///
    /// # Errors
    ///
    /// Returns an error if events cannot be retrieved
    fn get_events_for_session(&self, session_id: SessionId) -> Result<Vec<DomainEvent>, String>;

    /// Get events for a specific stream
    ///
    /// # Errors
    ///
    /// Returns an error if events cannot be retrieved
    fn get_events_for_stream(&self, stream_id: StreamId) -> Result<Vec<DomainEvent>, String>;

    /// Get all events since a specific timestamp
    ///
    /// # Errors
    ///
    /// Returns an error if events cannot be retrieved
    fn get_events_since(&self, since: DateTime<Utc>) -> Result<Vec<DomainEvent>, String>;
}

/// Simple in-memory event store for testing
#[derive(Debug, Clone, Default)]
pub struct InMemoryEventStore {
    events: Vec<DomainEvent>,
}

impl InMemoryEventStore {
    /// Create a new empty event store
    pub fn new() -> Self {
        Self::default()
    }

    /// Get all events in the store
    pub fn all_events(&self) -> &[DomainEvent] {
        &self.events
    }

    /// Get the total number of events in the store
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
}

impl EventStore for InMemoryEventStore {
    fn append_events(&mut self, mut events: Vec<DomainEvent>) -> Result<(), String> {
        self.events.append(&mut events);
        Ok(())
    }

    fn get_events_for_session(&self, session_id: SessionId) -> Result<Vec<DomainEvent>, String> {
        Ok(self
            .events
            .iter()
            .filter(|e| e.session_id() == session_id)
            .cloned()
            .collect())
    }

    fn get_events_for_stream(&self, stream_id: StreamId) -> Result<Vec<DomainEvent>, String> {
        Ok(self
            .events
            .iter()
            .filter(|e| e.stream_id() == Some(stream_id))
            .cloned()
            .collect())
    }

    fn get_events_since(&self, since: DateTime<Utc>) -> Result<Vec<DomainEvent>, String> {
        Ok(self
            .events
            .iter()
            .filter(|e| e.timestamp() > since)
            .cloned()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value_objects::{SessionId, StreamId};

    #[test]
    fn test_domain_event_properties() {
        let session_id = SessionId::new();
        let stream_id = StreamId::new();
        let timestamp = Utc::now();

        let event = DomainEvent::StreamCreated {
            session_id,
            stream_id,
            timestamp,
        };

        assert_eq!(event.session_id(), session_id);
        assert_eq!(event.stream_id(), Some(stream_id));
        assert_eq!(event.timestamp(), timestamp);
        assert_eq!(event.event_type(), "stream_created");
        assert!(!event.is_critical());
        assert!(!event.is_error());
    }

    #[test]
    fn test_critical_events() {
        let session_id = SessionId::new();
        let stream_id = StreamId::new();

        let error_event = DomainEvent::StreamFailed {
            session_id,
            stream_id,
            error: "Connection lost".to_string(),
            timestamp: Utc::now(),
        };

        assert!(error_event.is_critical());
        assert!(error_event.is_error());
        assert!(!error_event.is_completion());
    }

    #[test]
    fn test_event_store() {
        let mut store = InMemoryEventStore::new();
        let session_id = SessionId::new();
        let stream_id = StreamId::new();

        let events = vec![
            DomainEvent::SessionActivated {
                session_id,
                timestamp: Utc::now(),
            },
            DomainEvent::StreamCreated {
                session_id,
                stream_id,
                timestamp: Utc::now(),
            },
        ];

        store
            .append_events(events.clone())
            .expect("Failed to append events to store in test");
        assert_eq!(store.event_count(), 2);

        let session_events = store
            .get_events_for_session(session_id)
            .expect("Failed to retrieve session events in test");
        assert_eq!(session_events.len(), 2);

        let stream_events = store
            .get_events_for_stream(stream_id)
            .expect("Failed to retrieve stream events in test");
        assert_eq!(stream_events.len(), 1);
    }

    #[test]
    fn test_event_serialization() {
        let session_id = SessionId::new();
        let event = DomainEvent::SessionActivated {
            session_id,
            timestamp: Utc::now(),
        };

        let serialized = serde_json::to_string(&event).expect("Failed to serialize event in test");
        let deserialized: DomainEvent =
            serde_json::from_str(&serialized).expect("Failed to deserialize event in test");

        assert_eq!(event, deserialized);
    }
}

/// Event identifier for tracking and correlation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventId(uuid::Uuid);

impl EventId {
    /// Generate new unique event ID
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }

    /// Create from existing UUID
    pub fn from_uuid(uuid: uuid::Uuid) -> Self {
        Self(uuid)
    }

    /// Get inner UUID
    pub fn inner(&self) -> uuid::Uuid {
        self.0
    }
}

impl std::fmt::Display for EventId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Default for EventId {
    fn default() -> Self {
        Self::new()
    }
}

/// GAT-based trait for event subscribers that handle domain events
pub trait EventSubscriber {
    /// Future type for handling events
    type HandleFuture<'a>: std::future::Future<Output = crate::DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    /// Handle a domain event
    fn handle(&self, event: &DomainEvent) -> Self::HandleFuture<'_>;
}

/// Extension methods for DomainEvent
impl DomainEvent {
    /// Get event ID for tracking (generated if not exists)
    pub fn event_id(&self) -> EventId {
        // For now, generate deterministic ID based on event content
        // In future versions, this should be stored with the event
        let content = format!("{self:?}");
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hash = DefaultHasher::new();
        content.hash(&mut hash);
        let hash_val = hash.finish();
        let uuid = uuid::Uuid::from_bytes([
            (hash_val >> 56) as u8,
            (hash_val >> 48) as u8,
            (hash_val >> 40) as u8,
            (hash_val >> 32) as u8,
            (hash_val >> 24) as u8,
            (hash_val >> 16) as u8,
            (hash_val >> 8) as u8,
            hash_val as u8,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ]);
        EventId::from_uuid(uuid)
    }

    /// Get event timestamp
    pub fn occurred_at(&self) -> DateTime<Utc> {
        match self {
            DomainEvent::SessionActivated { timestamp, .. }
            | DomainEvent::SessionClosed { timestamp, .. }
            | DomainEvent::SessionExpired { timestamp, .. }
            | DomainEvent::StreamCreated { timestamp, .. }
            | DomainEvent::StreamStarted { timestamp, .. }
            | DomainEvent::StreamCompleted { timestamp, .. }
            | DomainEvent::StreamFailed { timestamp, .. }
            | DomainEvent::StreamCancelled { timestamp, .. }
            | DomainEvent::SkeletonGenerated { timestamp, .. }
            | DomainEvent::PatchFramesGenerated { timestamp, .. }
            | DomainEvent::FramesBatched { timestamp, .. }
            | DomainEvent::PriorityThresholdAdjusted { timestamp, .. }
            | DomainEvent::StreamConfigUpdated { timestamp, .. }
            | DomainEvent::PerformanceMetricsRecorded { timestamp, .. }
            | DomainEvent::SessionTimedOut { timestamp, .. }
            | DomainEvent::SessionTimeoutExtended { timestamp, .. } => *timestamp,
        }
    }

    /// Get event metadata as key-value pairs
    pub fn metadata(&self) -> std::collections::HashMap<String, String> {
        let mut metadata = std::collections::HashMap::new();
        metadata.insert("event_type".to_string(), self.event_type().to_string());
        metadata.insert("session_id".to_string(), self.session_id().to_string());
        metadata.insert("timestamp".to_string(), self.occurred_at().to_rfc3339());

        if let Some(stream_id) = self.stream_id() {
            metadata.insert("stream_id".to_string(), stream_id.to_string());
        }

        metadata
    }
}
