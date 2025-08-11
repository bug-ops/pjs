//! Domain events for event sourcing and integration

use crate::domain::value_objects::{SessionId, StreamId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Domain events that represent business-relevant state changes
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event_type", rename_all = "snake_case")]
pub enum DomainEvent {
    /// Session was activated and is ready to accept streams
    SessionActivated {
        session_id: SessionId,
        timestamp: DateTime<Utc>,
    },

    /// Session was closed gracefully
    SessionClosed {
        session_id: SessionId,
        timestamp: DateTime<Utc>,
    },

    /// Session expired due to timeout
    SessionExpired {
        session_id: SessionId,
        timestamp: DateTime<Utc>,
    },

    /// New stream was created in the session
    StreamCreated {
        session_id: SessionId,
        stream_id: StreamId,
        timestamp: DateTime<Utc>,
    },

    /// Stream started sending data
    StreamStarted {
        session_id: SessionId,
        stream_id: StreamId,
        timestamp: DateTime<Utc>,
    },

    /// Stream completed successfully
    StreamCompleted {
        session_id: SessionId,
        stream_id: StreamId,
        timestamp: DateTime<Utc>,
    },

    /// Stream failed with error
    StreamFailed {
        session_id: SessionId,
        stream_id: StreamId,
        error: String,
        timestamp: DateTime<Utc>,
    },

    /// Stream was cancelled
    StreamCancelled {
        session_id: SessionId,
        stream_id: StreamId,
        timestamp: DateTime<Utc>,
    },

    /// Skeleton frame was generated for a stream
    SkeletonGenerated {
        session_id: SessionId,
        stream_id: StreamId,
        frame_size_bytes: u64,
        timestamp: DateTime<Utc>,
    },

    /// Patch frames were generated for a stream
    PatchFramesGenerated {
        session_id: SessionId,
        stream_id: StreamId,
        frame_count: usize,
        total_bytes: u64,
        highest_priority: u8,
        timestamp: DateTime<Utc>,
    },

    /// Multiple frames were batched for efficient sending
    FramesBatched {
        session_id: SessionId,
        frame_count: usize,
        timestamp: DateTime<Utc>,
    },

    /// Priority threshold was adjusted for adaptive streaming
    PriorityThresholdAdjusted {
        session_id: SessionId,
        old_threshold: u8,
        new_threshold: u8,
        reason: String,
        timestamp: DateTime<Utc>,
    },

    /// Stream configuration was updated
    StreamConfigUpdated {
        session_id: SessionId,
        stream_id: StreamId,
        timestamp: DateTime<Utc>,
    },

    /// Performance metrics were recorded
    PerformanceMetricsRecorded {
        session_id: SessionId,
        metrics: PerformanceMetrics,
        timestamp: DateTime<Utc>,
    },
}

/// Performance metrics for monitoring and optimization
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub frames_per_second: f64,
    pub bytes_per_second: f64,
    pub average_frame_size: f64,
    pub priority_distribution: PriorityDistribution,
    pub latency_ms: Option<u64>,
}

/// Distribution of frames by priority level
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriorityDistribution {
    pub critical_frames: u64,
    pub high_frames: u64,
    pub medium_frames: u64,
    pub low_frames: u64,
    pub background_frames: u64,
}

impl Default for PriorityDistribution {
    fn default() -> Self {
        Self {
            critical_frames: 0,
            high_frames: 0,
            medium_frames: 0,
            low_frames: 0,
            background_frames: 0,
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

/// Event sourcing support
pub trait EventStore {
    /// Append events to the store
    fn append_events(&mut self, events: Vec<DomainEvent>) -> Result<(), String>;

    /// Get events for a specific session
    fn get_events_for_session(&self, session_id: SessionId) -> Result<Vec<DomainEvent>, String>;

    /// Get events for a specific stream
    fn get_events_for_stream(&self, stream_id: StreamId) -> Result<Vec<DomainEvent>, String>;

    /// Get all events since a specific timestamp
    fn get_events_since(&self, since: DateTime<Utc>) -> Result<Vec<DomainEvent>, String>;
}

/// Simple in-memory event store for testing
#[derive(Debug, Clone, Default)]
pub struct InMemoryEventStore {
    events: Vec<DomainEvent>,
}

impl InMemoryEventStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn all_events(&self) -> &[DomainEvent] {
        &self.events
    }

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
    use crate::domain::value_objects::{SessionId, StreamId};

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

        // TODO: Handle unwrap() - add proper error handling for event store operations in tests
        store.append_events(events.clone()).unwrap();
        assert_eq!(store.event_count(), 2);

        // TODO: Handle unwrap() - add proper error handling for session events retrieval in tests
        let session_events = store.get_events_for_session(session_id).unwrap();
        assert_eq!(session_events.len(), 2);

        // TODO: Handle unwrap() - add proper error handling for stream events retrieval in tests
        let stream_events = store.get_events_for_stream(stream_id).unwrap();
        assert_eq!(stream_events.len(), 1);
    }

    #[test]
    fn test_event_serialization() {
        let session_id = SessionId::new();
        let event = DomainEvent::SessionActivated {
            session_id,
            timestamp: Utc::now(),
        };

        // TODO: Handle unwrap() - add proper error handling for event serialization in tests
        let serialized = serde_json::to_string(&event).unwrap();
        // TODO: Handle unwrap() - add proper error handling for event deserialization in tests
        let deserialized: DomainEvent = serde_json::from_str(&serialized).unwrap();

        assert_eq!(event, deserialized);
    }
}

/// Event identifier for tracking and correlation
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

/// Trait for event subscribers that handle domain events
#[async_trait::async_trait]
pub trait EventSubscriber {
    /// Handle a domain event
    async fn handle(&self, event: &DomainEvent) -> crate::domain::DomainResult<()>;
}

/// Extension methods for DomainEvent
impl DomainEvent {
    /// Get event ID for tracking (generated if not exists)
    pub fn event_id(&self) -> EventId {
        // For now, generate deterministic ID based on event content
        // In future versions, this should be stored with the event
        let content = format!("{:?}", self);
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hash = DefaultHasher::new();
        content.hash(&mut hash);
        let hash_val = hash.finish();
        let uuid = uuid::Uuid::from_bytes([
            (hash_val >> 56) as u8, (hash_val >> 48) as u8,
            (hash_val >> 40) as u8, (hash_val >> 32) as u8,
            (hash_val >> 24) as u8, (hash_val >> 16) as u8,
            (hash_val >> 8) as u8, hash_val as u8,
            0, 0, 0, 0, 0, 0, 0, 0
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
            | DomainEvent::PerformanceMetricsRecorded { timestamp, .. } => *timestamp,
        }
    }
    
    
    /// Get event payload as JSON
    pub fn payload(&self) -> &serde_json::Value {
        // TODO: Fix architecture violation - domain events should not depend on serde_json::Value
        // For now, return empty object - this should be implemented with domain value objects
        use std::sync::LazyLock;
        static EMPTY: LazyLock<serde_json::Value> = LazyLock::new(|| {
            serde_json::Value::Object(serde_json::Map::new())
        });
        &*EMPTY
    }
}
