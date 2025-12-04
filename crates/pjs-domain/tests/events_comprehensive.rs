//! Comprehensive tests for domain/events/mod.rs
//!
//! This test suite aims to achieve 80%+ coverage by testing:
//! - All DomainEvent variants
//! - Event properties (session_id, stream_id, timestamp, event_type)
//! - Event classification (is_critical, is_error, is_completion)
//! - EventStore implementations
//! - Event serialization/deserialization
//! - PriorityDistribution calculations
//! - EventId generation

use chrono::Utc;
use pjs_domain::{
    events::{
        DomainEvent, EventStore, InMemoryEventStore, PerformanceMetrics, PriorityDistribution,
        SessionState,
    },
    value_objects::{SessionId, StreamId},
};

mod domain_event_creation_tests {
    use super::*;

    #[test]
    fn test_session_activated_event() {
        let session_id = SessionId::new();
        let timestamp = Utc::now();

        let event = DomainEvent::SessionActivated {
            session_id,
            timestamp,
        };

        assert_eq!(event.session_id(), session_id);
        assert!(event.stream_id().is_none());
        assert_eq!(event.timestamp(), timestamp);
        assert_eq!(event.event_type(), "session_activated");
        assert!(!event.is_critical());
        assert!(!event.is_error());
        assert!(!event.is_completion());
    }

    #[test]
    fn test_session_closed_event() {
        let session_id = SessionId::new();
        let timestamp = Utc::now();

        let event = DomainEvent::SessionClosed {
            session_id,
            timestamp,
        };

        assert_eq!(event.session_id(), session_id);
        assert_eq!(event.event_type(), "session_closed");
        assert!(!event.is_critical());
        assert!(event.is_completion());
    }

    #[test]
    fn test_session_expired_event() {
        let session_id = SessionId::new();
        let timestamp = Utc::now();

        let event = DomainEvent::SessionExpired {
            session_id,
            timestamp,
        };

        assert_eq!(event.session_id(), session_id);
        assert_eq!(event.event_type(), "session_expired");
        assert!(event.is_critical());
        assert!(!event.is_error());
    }

    #[test]
    fn test_session_timed_out_event() {
        let session_id = SessionId::new();
        let timestamp = Utc::now();

        let event = DomainEvent::SessionTimedOut {
            session_id,
            original_state: SessionState::Active,
            timeout_duration: 3600,
            timestamp,
        };

        assert_eq!(event.session_id(), session_id);
        assert_eq!(event.event_type(), "session_timed_out");
        assert!(!event.is_critical()); // Not classified as critical
    }

    #[test]
    fn test_session_timeout_extended_event() {
        let session_id = SessionId::new();
        let timestamp = Utc::now();
        let new_expires_at = Utc::now() + chrono::Duration::seconds(7200);

        let event = DomainEvent::SessionTimeoutExtended {
            session_id,
            additional_seconds: 3600,
            new_expires_at,
            timestamp,
        };

        assert_eq!(event.session_id(), session_id);
        assert_eq!(event.event_type(), "session_timeout_extended");
    }

    #[test]
    fn test_stream_created_event() {
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
        assert_eq!(event.event_type(), "stream_created");
    }

    #[test]
    fn test_stream_started_event() {
        let session_id = SessionId::new();
        let stream_id = StreamId::new();
        let timestamp = Utc::now();

        let event = DomainEvent::StreamStarted {
            session_id,
            stream_id,
            timestamp,
        };

        assert_eq!(event.stream_id(), Some(stream_id));
        assert_eq!(event.event_type(), "stream_started");
    }

    #[test]
    fn test_stream_completed_event() {
        let session_id = SessionId::new();
        let stream_id = StreamId::new();
        let timestamp = Utc::now();

        let event = DomainEvent::StreamCompleted {
            session_id,
            stream_id,
            timestamp,
        };

        assert_eq!(event.stream_id(), Some(stream_id));
        assert_eq!(event.event_type(), "stream_completed");
        assert!(event.is_completion());
        assert!(!event.is_error());
    }

    #[test]
    fn test_stream_failed_event() {
        let session_id = SessionId::new();
        let stream_id = StreamId::new();
        let timestamp = Utc::now();

        let event = DomainEvent::StreamFailed {
            session_id,
            stream_id,
            error: "Connection lost".to_string(),
            timestamp,
        };

        assert_eq!(event.event_type(), "stream_failed");
        assert!(event.is_critical());
        assert!(event.is_error());
        assert!(!event.is_completion());
    }

    #[test]
    fn test_stream_cancelled_event() {
        let session_id = SessionId::new();
        let stream_id = StreamId::new();
        let timestamp = Utc::now();

        let event = DomainEvent::StreamCancelled {
            session_id,
            stream_id,
            timestamp,
        };

        assert_eq!(event.event_type(), "stream_cancelled");
        assert!(!event.is_critical());
    }

    #[test]
    fn test_skeleton_generated_event() {
        let session_id = SessionId::new();
        let stream_id = StreamId::new();
        let timestamp = Utc::now();

        let event = DomainEvent::SkeletonGenerated {
            session_id,
            stream_id,
            frame_size_bytes: 1024,
            timestamp,
        };

        assert_eq!(event.stream_id(), Some(stream_id));
        assert_eq!(event.event_type(), "skeleton_generated");
    }

    #[test]
    fn test_patch_frames_generated_event() {
        let session_id = SessionId::new();
        let stream_id = StreamId::new();
        let timestamp = Utc::now();

        let event = DomainEvent::PatchFramesGenerated {
            session_id,
            stream_id,
            frame_count: 5,
            total_bytes: 2048,
            highest_priority: 255,
            timestamp,
        };

        assert_eq!(event.stream_id(), Some(stream_id));
        assert_eq!(event.event_type(), "patch_frames_generated");
    }

    #[test]
    fn test_frames_batched_event() {
        let session_id = SessionId::new();
        let timestamp = Utc::now();

        let event = DomainEvent::FramesBatched {
            session_id,
            frame_count: 10,
            timestamp,
        };

        assert_eq!(event.session_id(), session_id);
        assert!(event.stream_id().is_none());
        assert_eq!(event.event_type(), "frames_batched");
    }

    #[test]
    fn test_priority_threshold_adjusted_event() {
        let session_id = SessionId::new();
        let timestamp = Utc::now();

        let event = DomainEvent::PriorityThresholdAdjusted {
            session_id,
            old_threshold: 128,
            new_threshold: 192,
            reason: "Network congestion detected".to_string(),
            timestamp,
        };

        assert_eq!(event.event_type(), "priority_threshold_adjusted");
    }

    #[test]
    fn test_stream_config_updated_event() {
        let session_id = SessionId::new();
        let stream_id = StreamId::new();
        let timestamp = Utc::now();

        let event = DomainEvent::StreamConfigUpdated {
            session_id,
            stream_id,
            timestamp,
        };

        assert_eq!(event.stream_id(), Some(stream_id));
        assert_eq!(event.event_type(), "stream_config_updated");
    }

    #[test]
    fn test_performance_metrics_recorded_event() {
        let session_id = SessionId::new();
        let timestamp = Utc::now();
        let metrics = PerformanceMetrics {
            frames_per_second: 100.0,
            bytes_per_second: 1024000.0,
            average_frame_size: 10240.0,
            priority_distribution: PriorityDistribution::default(),
            latency_ms: Some(50),
        };

        let event = DomainEvent::PerformanceMetricsRecorded {
            session_id,
            metrics,
            timestamp,
        };

        assert_eq!(event.event_type(), "performance_metrics_recorded");
    }
}

mod event_metadata_tests {
    use super::*;

    #[test]
    fn test_event_id_generation() {
        let session_id = SessionId::new();
        let timestamp = Utc::now();

        let event = DomainEvent::SessionActivated {
            session_id,
            timestamp,
        };

        let event_id = event.event_id();
        let event_id_str = event_id.to_string();
        assert!(!event_id_str.is_empty());
    }

    #[test]
    fn test_event_id_consistency() {
        let session_id = SessionId::new();
        let timestamp = Utc::now();

        let event = DomainEvent::SessionActivated {
            session_id,
            timestamp,
        };

        let id1 = event.event_id();
        let id2 = event.event_id();
        assert_eq!(id1, id2); // Should be deterministic
    }

    #[test]
    fn test_occurred_at() {
        let session_id = SessionId::new();
        let timestamp = Utc::now();

        let event = DomainEvent::SessionActivated {
            session_id,
            timestamp,
        };

        assert_eq!(event.occurred_at(), timestamp);
    }

    #[test]
    fn test_metadata_extraction() {
        let session_id = SessionId::new();
        let stream_id = StreamId::new();
        let timestamp = Utc::now();

        let event = DomainEvent::StreamCreated {
            session_id,
            stream_id,
            timestamp,
        };

        let metadata = event.metadata();
        assert_eq!(
            metadata.get("event_type"),
            Some(&"stream_created".to_string())
        );
        assert_eq!(metadata.get("session_id"), Some(&session_id.to_string()));
        assert_eq!(metadata.get("stream_id"), Some(&stream_id.to_string()));
        assert!(metadata.contains_key("timestamp"));
    }

    #[test]
    fn test_metadata_without_stream_id() {
        let session_id = SessionId::new();
        let timestamp = Utc::now();

        let event = DomainEvent::SessionActivated {
            session_id,
            timestamp,
        };

        let metadata = event.metadata();
        assert!(!metadata.contains_key("stream_id"));
    }
}

mod event_store_tests {
    use super::*;

    #[test]
    fn test_in_memory_store_creation() {
        let store = InMemoryEventStore::new();
        assert_eq!(store.event_count(), 0);
        assert_eq!(store.all_events().len(), 0);
    }

    #[test]
    fn test_append_events() {
        let mut store = InMemoryEventStore::new();
        let session_id = SessionId::new();

        let events = vec![
            DomainEvent::SessionActivated {
                session_id,
                timestamp: Utc::now(),
            },
            DomainEvent::SessionClosed {
                session_id,
                timestamp: Utc::now(),
            },
        ];

        assert!(store.append_events(events).is_ok());
        assert_eq!(store.event_count(), 2);
    }

    #[test]
    fn test_get_events_for_session() {
        let mut store = InMemoryEventStore::new();
        let session_id1 = SessionId::new();
        let session_id2 = SessionId::new();

        let events = vec![
            DomainEvent::SessionActivated {
                session_id: session_id1,
                timestamp: Utc::now(),
            },
            DomainEvent::SessionActivated {
                session_id: session_id2,
                timestamp: Utc::now(),
            },
            DomainEvent::SessionClosed {
                session_id: session_id1,
                timestamp: Utc::now(),
            },
        ];

        store.append_events(events).unwrap();

        let session1_events = store.get_events_for_session(session_id1).unwrap();
        assert_eq!(session1_events.len(), 2);

        let session2_events = store.get_events_for_session(session_id2).unwrap();
        assert_eq!(session2_events.len(), 1);
    }

    #[test]
    fn test_get_events_for_stream() {
        let mut store = InMemoryEventStore::new();
        let session_id = SessionId::new();
        let stream_id1 = StreamId::new();
        let stream_id2 = StreamId::new();

        let events = vec![
            DomainEvent::StreamCreated {
                session_id,
                stream_id: stream_id1,
                timestamp: Utc::now(),
            },
            DomainEvent::StreamCreated {
                session_id,
                stream_id: stream_id2,
                timestamp: Utc::now(),
            },
            DomainEvent::StreamStarted {
                session_id,
                stream_id: stream_id1,
                timestamp: Utc::now(),
            },
        ];

        store.append_events(events).unwrap();

        let stream1_events = store.get_events_for_stream(stream_id1).unwrap();
        assert_eq!(stream1_events.len(), 2);

        let stream2_events = store.get_events_for_stream(stream_id2).unwrap();
        assert_eq!(stream2_events.len(), 1);
    }

    #[test]
    fn test_get_events_since() {
        let mut store = InMemoryEventStore::new();
        let session_id = SessionId::new();
        let now = Utc::now();
        let past = now - chrono::Duration::seconds(10);

        let events = vec![
            DomainEvent::SessionActivated {
                session_id,
                timestamp: past,
            },
            DomainEvent::SessionClosed {
                session_id,
                timestamp: now,
            },
        ];

        store.append_events(events).unwrap();

        let recent_events = store
            .get_events_since(now - chrono::Duration::seconds(5))
            .unwrap();
        assert_eq!(recent_events.len(), 1);
    }

    #[test]
    fn test_all_events() {
        let mut store = InMemoryEventStore::new();
        let session_id = SessionId::new();

        let events = vec![
            DomainEvent::SessionActivated {
                session_id,
                timestamp: Utc::now(),
            },
            DomainEvent::SessionClosed {
                session_id,
                timestamp: Utc::now(),
            },
        ];

        store.append_events(events.clone()).unwrap();
        assert_eq!(store.all_events().len(), 2);
    }
}

mod priority_distribution_tests {
    use super::*;

    #[test]
    fn test_priority_distribution_new() {
        let dist = PriorityDistribution::new();
        assert_eq!(dist.critical_frames, 0);
        assert_eq!(dist.high_frames, 0);
        assert_eq!(dist.medium_frames, 0);
        assert_eq!(dist.low_frames, 0);
        assert_eq!(dist.background_frames, 0);
    }

    #[test]
    fn test_priority_distribution_default() {
        let dist = PriorityDistribution::default();
        assert_eq!(dist.total_frames(), 0);
    }

    #[test]
    fn test_priority_distribution_total_frames() {
        let dist = PriorityDistribution {
            critical_frames: 10,
            high_frames: 20,
            medium_frames: 30,
            low_frames: 25,
            background_frames: 15,
        };
        assert_eq!(dist.total_frames(), 100);
    }

    #[test]
    fn test_priority_distribution_from_counts() {
        let dist = PriorityDistribution::from_counts(5, 10, 15, 20, 25);
        assert_eq!(dist.critical_frames, 5);
        assert_eq!(dist.high_frames, 10);
        assert_eq!(dist.medium_frames, 15);
        assert_eq!(dist.low_frames, 20);
        assert_eq!(dist.background_frames, 25);
        assert_eq!(dist.total_frames(), 75);
    }

    #[test]
    fn test_priority_distribution_as_percentages() {
        let dist = PriorityDistribution {
            critical_frames: 10,
            high_frames: 20,
            medium_frames: 30,
            low_frames: 20,
            background_frames: 20,
        };

        let percentages = dist.as_percentages();
        assert_eq!(percentages.critical, 0.1);
        assert_eq!(percentages.high, 0.2);
        assert_eq!(percentages.medium, 0.3);
        assert_eq!(percentages.low, 0.2);
        assert_eq!(percentages.background, 0.2);
    }

    #[test]
    fn test_priority_distribution_as_percentages_empty() {
        let dist = PriorityDistribution::default();
        let percentages = dist.as_percentages();
        assert_eq!(percentages.critical, 0.0);
        assert_eq!(percentages.high, 0.0);
        assert_eq!(percentages.medium, 0.0);
        assert_eq!(percentages.low, 0.0);
        assert_eq!(percentages.background, 0.0);
    }
}

mod event_serialization_tests {
    use super::*;

    #[test]
    fn test_serialize_session_activated() {
        let session_id = SessionId::new();
        let event = DomainEvent::SessionActivated {
            session_id,
            timestamp: Utc::now(),
        };

        let serialized = serde_json::to_string(&event).expect("should serialize");
        assert!(serialized.contains("session_activated"));
    }

    #[test]
    fn test_deserialize_session_activated() {
        let session_id = SessionId::new();
        let event = DomainEvent::SessionActivated {
            session_id,
            timestamp: Utc::now(),
        };

        let serialized = serde_json::to_string(&event).unwrap();
        let deserialized: DomainEvent =
            serde_json::from_str(&serialized).expect("should deserialize");

        assert_eq!(event, deserialized);
    }

    #[test]
    fn test_serialize_stream_failed() {
        let session_id = SessionId::new();
        let stream_id = StreamId::new();
        let event = DomainEvent::StreamFailed {
            session_id,
            stream_id,
            error: "Test error".to_string(),
            timestamp: Utc::now(),
        };

        let serialized = serde_json::to_string(&event).expect("should serialize");
        assert!(serialized.contains("stream_failed"));
        assert!(serialized.contains("Test error"));
    }

    #[test]
    fn test_serialize_performance_metrics() {
        let session_id = SessionId::new();
        let metrics = PerformanceMetrics {
            frames_per_second: 100.0,
            bytes_per_second: 1024000.0,
            average_frame_size: 10240.0,
            priority_distribution: PriorityDistribution::default(),
            latency_ms: Some(50),
        };

        let event = DomainEvent::PerformanceMetricsRecorded {
            session_id,
            metrics,
            timestamp: Utc::now(),
        };

        let serialized = serde_json::to_string(&event).expect("should serialize");
        assert!(serialized.contains("performance_metrics_recorded"));
    }
}

mod session_state_tests {
    use super::*;

    #[test]
    fn test_session_state_variants() {
        assert_eq!(SessionState::Initializing, SessionState::Initializing);
        assert_eq!(SessionState::Active, SessionState::Active);
        assert_eq!(SessionState::Closing, SessionState::Closing);
        assert_eq!(SessionState::Completed, SessionState::Completed);
        assert_eq!(SessionState::Failed, SessionState::Failed);
    }

    #[test]
    fn test_session_state_clone() {
        let state = SessionState::Active;
        let cloned = state.clone();
        assert_eq!(state, cloned);
    }

    #[test]
    fn test_session_state_serialization() {
        let state = SessionState::Active;
        let serialized = serde_json::to_string(&state).expect("should serialize");
        let deserialized: SessionState =
            serde_json::from_str(&serialized).expect("should deserialize");
        assert_eq!(state, deserialized);
    }
}

mod event_id_tests {
    use pjs_domain::events::EventId;

    #[test]
    fn test_event_id_new() {
        let id1 = EventId::new();
        let id2 = EventId::new();
        assert_ne!(id1, id2); // Should be unique
    }

    #[test]
    fn test_event_id_default() {
        let id = EventId::default();
        let id_str = id.to_string();
        assert!(!id_str.is_empty());
    }

    #[test]
    fn test_event_id_from_uuid() {
        let uuid = uuid::Uuid::new_v4();
        let event_id = EventId::from_uuid(uuid);
        assert_eq!(event_id.inner(), uuid);
    }

    #[test]
    fn test_event_id_display() {
        let id = EventId::new();
        let display = format!("{}", id);
        assert!(!display.is_empty());
    }

    #[test]
    fn test_event_id_equality() {
        let uuid = uuid::Uuid::new_v4();
        let id1 = EventId::from_uuid(uuid);
        let id2 = EventId::from_uuid(uuid);
        assert_eq!(id1, id2);
    }
}

mod event_classification_tests {
    use super::*;

    #[test]
    fn test_all_critical_events() {
        let session_id = SessionId::new();
        let stream_id = StreamId::new();
        let timestamp = Utc::now();

        let critical_events: Vec<DomainEvent> = vec![
            DomainEvent::StreamFailed {
                session_id,
                stream_id,
                error: "Error".to_string(),
                timestamp,
            },
            DomainEvent::SessionExpired {
                session_id,
                timestamp,
            },
        ];

        for event in critical_events {
            assert!(event.is_critical());
        }
    }

    #[test]
    fn test_all_error_events() {
        let session_id = SessionId::new();
        let stream_id = StreamId::new();
        let timestamp = Utc::now();

        let error_event = DomainEvent::StreamFailed {
            session_id,
            stream_id,
            error: "Error".to_string(),
            timestamp,
        };

        assert!(error_event.is_error());
    }

    #[test]
    fn test_all_completion_events() {
        let session_id = SessionId::new();
        let stream_id = StreamId::new();
        let timestamp = Utc::now();

        let completion_events: Vec<DomainEvent> = vec![
            DomainEvent::StreamCompleted {
                session_id,
                stream_id,
                timestamp,
            },
            DomainEvent::SessionClosed {
                session_id,
                timestamp,
            },
        ];

        for event in completion_events {
            assert!(event.is_completion());
        }
    }
}
