//! Comprehensive tests for domain/entities/stream.rs
//!
//! This test suite aims to achieve 80%+ coverage by testing:
//! - Stream lifecycle and state transitions
//! - Frame creation (skeleton, patch, completion)
//! - Statistics tracking
//! - Metadata management
//! - Error conditions
//! - Progress calculation

use pjs_domain::{
    entities::{
        Stream,
        frame::FrameType,
        stream::{StreamConfig, StreamState},
    },
    value_objects::{JsonData, Priority, SessionId},
};
use std::collections::HashMap;

mod stream_lifecycle_tests {
    use super::*;

    #[test]
    fn test_stream_creation_with_default_config() {
        let session_id = SessionId::new();
        let source_data = JsonData::Object(HashMap::new());
        let stream = Stream::new(session_id, source_data, StreamConfig::default());

        assert_eq!(stream.session_id(), session_id);
        assert_eq!(stream.state(), &StreamState::Preparing);
        assert!(stream.is_active());
        assert!(!stream.is_finished());
    }

    #[test]
    fn test_stream_creation_with_custom_config() {
        let session_id = SessionId::new();
        let source_data = JsonData::Null;
        let config = StreamConfig {
            max_frame_size: 128 * 1024,
            max_frames_per_batch: 20,
            enable_compression: false,
            priority_rules: HashMap::new(),
        };

        let stream = Stream::new(session_id, source_data, config.clone());
        assert_eq!(stream.config().max_frame_size, 128 * 1024);
        assert_eq!(stream.config().max_frames_per_batch, 20);
        assert!(!stream.config().enable_compression);
    }

    #[test]
    fn test_stream_start() {
        let session_id = SessionId::new();
        let source_data = JsonData::Null;
        let mut stream = Stream::new(session_id, source_data, StreamConfig::default());

        assert!(stream.start_streaming().is_ok());
        assert_eq!(stream.state(), &StreamState::Streaming);
    }

    #[test]
    fn test_stream_cannot_start_from_completed() {
        let session_id = SessionId::new();
        let source_data = JsonData::Null;
        let mut stream = Stream::new(session_id, source_data, StreamConfig::default());

        assert!(stream.start_streaming().is_ok());
        assert!(stream.complete().is_ok());
        assert!(stream.start_streaming().is_err());
    }

    #[test]
    fn test_stream_complete() {
        let session_id = SessionId::new();
        let source_data = JsonData::Null;
        let mut stream = Stream::new(session_id, source_data, StreamConfig::default());

        assert!(stream.start_streaming().is_ok());
        assert!(stream.complete().is_ok());
        assert_eq!(stream.state(), &StreamState::Completed);
        assert!(stream.is_finished());
        assert!(stream.completed_at().is_some());
    }

    #[test]
    fn test_stream_cannot_complete_from_preparing() {
        let session_id = SessionId::new();
        let source_data = JsonData::Null;
        let mut stream = Stream::new(session_id, source_data, StreamConfig::default());

        let result = stream.complete();
        assert!(result.is_err());
    }

    #[test]
    fn test_stream_fail_from_preparing() {
        let session_id = SessionId::new();
        let source_data = JsonData::Null;
        let mut stream = Stream::new(session_id, source_data, StreamConfig::default());

        assert!(stream.fail("Test error".to_string()).is_ok());
        assert_eq!(stream.state(), &StreamState::Failed);
        assert!(stream.is_finished());
        assert_eq!(
            stream.metadata().get("error"),
            Some(&"Test error".to_string())
        );
    }

    #[test]
    fn test_stream_fail_from_streaming() {
        let session_id = SessionId::new();
        let source_data = JsonData::Null;
        let mut stream = Stream::new(session_id, source_data, StreamConfig::default());

        assert!(stream.start_streaming().is_ok());
        assert!(stream.fail("Connection lost".to_string()).is_ok());
        assert_eq!(stream.state(), &StreamState::Failed);
    }

    #[test]
    fn test_stream_cannot_fail_from_completed() {
        let session_id = SessionId::new();
        let source_data = JsonData::Null;
        let mut stream = Stream::new(session_id, source_data, StreamConfig::default());

        assert!(stream.start_streaming().is_ok());
        assert!(stream.complete().is_ok());
        assert!(stream.fail("Test error".to_string()).is_err());
    }

    #[test]
    fn test_stream_cancel_from_preparing() {
        let session_id = SessionId::new();
        let source_data = JsonData::Null;
        let mut stream = Stream::new(session_id, source_data, StreamConfig::default());

        assert!(stream.cancel().is_ok());
        assert_eq!(stream.state(), &StreamState::Cancelled);
        assert!(stream.is_finished());
    }

    #[test]
    fn test_stream_cancel_from_streaming() {
        let session_id = SessionId::new();
        let source_data = JsonData::Null;
        let mut stream = Stream::new(session_id, source_data, StreamConfig::default());

        assert!(stream.start_streaming().is_ok());
        assert!(stream.cancel().is_ok());
        assert_eq!(stream.state(), &StreamState::Cancelled);
    }

    #[test]
    fn test_stream_cannot_cancel_from_failed() {
        let session_id = SessionId::new();
        let source_data = JsonData::Null;
        let mut stream = Stream::new(session_id, source_data, StreamConfig::default());

        assert!(stream.fail("Error".to_string()).is_ok());
        assert!(stream.cancel().is_err());
    }
}

mod frame_creation_tests {
    use super::*;

    #[test]
    fn test_create_skeleton_frame() {
        let session_id = SessionId::new();
        let mut obj = HashMap::new();
        obj.insert("key".to_string(), JsonData::String("value".to_string()));
        let source_data = JsonData::Object(obj);
        let mut stream = Stream::new(session_id, source_data, StreamConfig::default());

        assert!(stream.start_streaming().is_ok());
        let frame = stream
            .create_skeleton_frame()
            .expect("should create skeleton");

        assert_eq!(frame.frame_type(), &FrameType::Skeleton);
        assert_eq!(frame.sequence(), 1);
        assert_eq!(stream.stats().skeleton_frames, 1);
        assert_eq!(stream.stats().total_frames, 1);
    }

    #[test]
    fn test_create_skeleton_frame_without_streaming() {
        let session_id = SessionId::new();
        let source_data = JsonData::Null;
        let mut stream = Stream::new(session_id, source_data, StreamConfig::default());

        let result = stream.create_skeleton_frame();
        assert!(result.is_err());
    }

    #[test]
    fn test_create_patch_frames() {
        let session_id = SessionId::new();
        let source_data = JsonData::String("test".to_string());
        let mut stream = Stream::new(session_id, source_data, StreamConfig::default());

        assert!(stream.start_streaming().is_ok());
        let frames = stream
            .create_patch_frames(Priority::MEDIUM, 5)
            .expect("should create patches");

        // Simplified implementation returns empty vec
        assert_eq!(frames.len(), 0);
    }

    #[test]
    fn test_create_patch_frames_without_streaming() {
        let session_id = SessionId::new();
        let source_data = JsonData::Null;
        let mut stream = Stream::new(session_id, source_data, StreamConfig::default());

        let result = stream.create_patch_frames(Priority::HIGH, 10);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_completion_frame() {
        let session_id = SessionId::new();
        let source_data = JsonData::Null;
        let mut stream = Stream::new(session_id, source_data, StreamConfig::default());

        assert!(stream.start_streaming().is_ok());
        let frame = stream
            .create_completion_frame(Some("checksum123".to_string()))
            .expect("should create completion frame");

        assert_eq!(frame.frame_type(), &FrameType::Complete);
        assert_eq!(stream.stats().complete_frames, 1);
    }

    #[test]
    fn test_create_completion_frame_without_checksum() {
        let session_id = SessionId::new();
        let source_data = JsonData::Null;
        let mut stream = Stream::new(session_id, source_data, StreamConfig::default());

        assert!(stream.start_streaming().is_ok());
        let frame = stream
            .create_completion_frame(None)
            .expect("should create completion frame without checksum");

        assert_eq!(frame.frame_type(), &FrameType::Complete);
    }

    #[test]
    fn test_create_completion_frame_without_streaming() {
        let session_id = SessionId::new();
        let source_data = JsonData::Null;
        let mut stream = Stream::new(session_id, source_data, StreamConfig::default());

        let result = stream.create_completion_frame(None);
        assert!(result.is_err());
    }

    #[test]
    fn test_multiple_frame_sequence() {
        let session_id = SessionId::new();
        let source_data = JsonData::Object(HashMap::new());
        let mut stream = Stream::new(session_id, source_data, StreamConfig::default());

        assert!(stream.start_streaming().is_ok());

        let skeleton = stream
            .create_skeleton_frame()
            .expect("should create skeleton");
        assert_eq!(skeleton.sequence(), 1);

        let completion = stream
            .create_completion_frame(None)
            .expect("should create completion");
        assert_eq!(completion.sequence(), 2);

        assert_eq!(stream.stats().total_frames, 2);
    }
}

mod statistics_tests {
    use super::*;

    #[test]
    fn test_stats_initial() {
        let session_id = SessionId::new();
        let source_data = JsonData::Null;
        let stream = Stream::new(session_id, source_data, StreamConfig::default());

        let stats = stream.stats();
        assert_eq!(stats.total_frames, 0);
        assert_eq!(stats.skeleton_frames, 0);
        assert_eq!(stats.patch_frames, 0);
        assert_eq!(stats.complete_frames, 0);
        assert_eq!(stats.error_frames, 0);
        assert_eq!(stats.total_bytes, 0);
        assert_eq!(stats.critical_bytes, 0);
        assert_eq!(stats.high_priority_bytes, 0);
        assert_eq!(stats.average_frame_size, 0.0);
    }

    #[test]
    fn test_stats_after_skeleton() {
        let session_id = SessionId::new();
        let source_data = JsonData::Object(HashMap::new());
        let mut stream = Stream::new(session_id, source_data, StreamConfig::default());

        assert!(stream.start_streaming().is_ok());
        let _ = stream.create_skeleton_frame();

        assert_eq!(stream.stats().skeleton_frames, 1);
        assert_eq!(stream.stats().total_frames, 1);
        assert!(stream.stats().total_bytes > 0);
        assert!(stream.stats().average_frame_size > 0.0);
    }

    #[test]
    fn test_stats_after_completion() {
        let session_id = SessionId::new();
        let source_data = JsonData::Null;
        let mut stream = Stream::new(session_id, source_data, StreamConfig::default());

        assert!(stream.start_streaming().is_ok());
        let _ = stream.create_completion_frame(None);

        assert_eq!(stream.stats().complete_frames, 1);
        assert_eq!(stream.stats().total_frames, 1);
    }
}

mod metadata_tests {
    use super::*;

    #[test]
    fn test_metadata_initial() {
        let session_id = SessionId::new();
        let source_data = JsonData::Null;
        let stream = Stream::new(session_id, source_data, StreamConfig::default());

        assert_eq!(stream.metadata().len(), 0);
    }

    #[test]
    fn test_add_metadata() {
        let session_id = SessionId::new();
        let source_data = JsonData::Null;
        let mut stream = Stream::new(session_id, source_data, StreamConfig::default());

        stream.add_metadata("key1".to_string(), "value1".to_string());
        stream.add_metadata("key2".to_string(), "value2".to_string());

        assert_eq!(stream.metadata().len(), 2);
        assert_eq!(stream.metadata().get("key1"), Some(&"value1".to_string()));
        assert_eq!(stream.metadata().get("key2"), Some(&"value2".to_string()));
    }

    #[test]
    fn test_metadata_overwrite() {
        let session_id = SessionId::new();
        let source_data = JsonData::Null;
        let mut stream = Stream::new(session_id, source_data, StreamConfig::default());

        stream.add_metadata("key".to_string(), "value1".to_string());
        stream.add_metadata("key".to_string(), "value2".to_string());

        assert_eq!(stream.metadata().len(), 1);
        assert_eq!(stream.metadata().get("key"), Some(&"value2".to_string()));
    }

    #[test]
    fn test_fail_adds_error_metadata() {
        let session_id = SessionId::new();
        let source_data = JsonData::Null;
        let mut stream = Stream::new(session_id, source_data, StreamConfig::default());

        assert!(stream.fail("Connection timeout".to_string()).is_ok());
        assert_eq!(
            stream.metadata().get("error"),
            Some(&"Connection timeout".to_string())
        );
    }
}

mod progress_tests {
    use super::*;

    #[test]
    fn test_progress_preparing() {
        let session_id = SessionId::new();
        let source_data = JsonData::Null;
        let stream = Stream::new(session_id, source_data, StreamConfig::default());

        assert_eq!(stream.progress(), 0.0);
    }

    #[test]
    fn test_progress_streaming_no_frames() {
        let session_id = SessionId::new();
        let source_data = JsonData::Null;
        let mut stream = Stream::new(session_id, source_data, StreamConfig::default());

        assert!(stream.start_streaming().is_ok());
        assert_eq!(stream.progress(), 0.1);
    }

    #[test]
    fn test_progress_completed() {
        let session_id = SessionId::new();
        let source_data = JsonData::Null;
        let mut stream = Stream::new(session_id, source_data, StreamConfig::default());

        assert!(stream.start_streaming().is_ok());
        assert!(stream.complete().is_ok());
        assert_eq!(stream.progress(), 1.0);
    }

    #[test]
    fn test_progress_failed() {
        let session_id = SessionId::new();
        let source_data = JsonData::Null;
        let mut stream = Stream::new(session_id, source_data, StreamConfig::default());

        assert!(stream.fail("Error".to_string()).is_ok());
        let progress = stream.progress();
        // Failed without any frames returns 0.0
        assert!((0.0..1.0).contains(&progress));
    }

    #[test]
    fn test_progress_cancelled() {
        let session_id = SessionId::new();
        let source_data = JsonData::Null;
        let mut stream = Stream::new(session_id, source_data, StreamConfig::default());

        assert!(stream.start_streaming().is_ok());
        assert!(stream.cancel().is_ok());
        let progress = stream.progress();
        assert!((0.0..1.0).contains(&progress));
    }
}

mod duration_tests {
    use super::*;

    #[test]
    fn test_duration_not_completed() {
        let session_id = SessionId::new();
        let source_data = JsonData::Null;
        let stream = Stream::new(session_id, source_data, StreamConfig::default());

        assert!(stream.duration().is_none());
    }

    #[test]
    fn test_duration_after_completion() {
        let session_id = SessionId::new();
        let source_data = JsonData::Null;
        let mut stream = Stream::new(session_id, source_data, StreamConfig::default());

        assert!(stream.start_streaming().is_ok());
        assert!(stream.complete().is_ok());

        let duration = stream.duration();
        assert!(duration.is_some());
        assert!(duration.unwrap().num_milliseconds() >= 0);
    }

    #[test]
    fn test_duration_after_failure() {
        let session_id = SessionId::new();
        let source_data = JsonData::Null;
        let mut stream = Stream::new(session_id, source_data, StreamConfig::default());

        assert!(stream.fail("Error".to_string()).is_ok());

        let duration = stream.duration();
        assert!(duration.is_some());
    }
}

mod config_update_tests {
    use super::*;

    #[test]
    fn test_update_config_while_preparing() {
        let session_id = SessionId::new();
        let source_data = JsonData::Null;
        let mut stream = Stream::new(session_id, source_data, StreamConfig::default());

        let new_config = StreamConfig {
            max_frame_size: 256 * 1024,
            ..Default::default()
        };

        assert!(stream.update_config(new_config).is_ok());
        assert_eq!(stream.config().max_frame_size, 256 * 1024);
    }

    #[test]
    fn test_update_config_while_streaming() {
        let session_id = SessionId::new();
        let source_data = JsonData::Null;
        let mut stream = Stream::new(session_id, source_data, StreamConfig::default());

        assert!(stream.start_streaming().is_ok());

        let new_config = StreamConfig {
            max_frames_per_batch: 15,
            ..Default::default()
        };

        assert!(stream.update_config(new_config).is_ok());
        assert_eq!(stream.config().max_frames_per_batch, 15);
    }

    #[test]
    fn test_cannot_update_config_after_completion() {
        let session_id = SessionId::new();
        let source_data = JsonData::Null;
        let mut stream = Stream::new(session_id, source_data, StreamConfig::default());

        assert!(stream.start_streaming().is_ok());
        assert!(stream.complete().is_ok());

        let new_config = StreamConfig::default();
        assert!(stream.update_config(new_config).is_err());
    }

    #[test]
    fn test_cannot_update_config_after_failure() {
        let session_id = SessionId::new();
        let source_data = JsonData::Null;
        let mut stream = Stream::new(session_id, source_data, StreamConfig::default());

        assert!(stream.fail("Error".to_string()).is_ok());

        let new_config = StreamConfig::default();
        assert!(stream.update_config(new_config).is_err());
    }
}

mod source_data_tests {
    use super::*;

    #[test]
    fn test_source_data_null() {
        let session_id = SessionId::new();
        let source_data = JsonData::Null;
        let stream = Stream::new(session_id, source_data, StreamConfig::default());

        assert_eq!(stream.source_data(), Some(&JsonData::Null));
    }

    #[test]
    fn test_source_data_string() {
        let session_id = SessionId::new();
        let source_data = JsonData::String("test".to_string());
        let stream = Stream::new(session_id, source_data.clone(), StreamConfig::default());

        assert_eq!(stream.source_data(), Some(&source_data));
    }

    #[test]
    fn test_source_data_object() {
        let session_id = SessionId::new();
        let mut obj = HashMap::new();
        obj.insert("key".to_string(), JsonData::Integer(42));
        let source_data = JsonData::Object(obj);
        let stream = Stream::new(session_id, source_data.clone(), StreamConfig::default());

        assert_eq!(stream.source_data(), Some(&source_data));
    }

    #[test]
    fn test_source_data_array() {
        let session_id = SessionId::new();
        let source_data = JsonData::Array(vec![JsonData::Integer(1), JsonData::Integer(2)]);
        let stream = Stream::new(session_id, source_data.clone(), StreamConfig::default());

        assert_eq!(stream.source_data(), Some(&source_data));
    }
}

mod timestamps_tests {
    use super::*;

    #[test]
    fn test_created_at() {
        let session_id = SessionId::new();
        let source_data = JsonData::Null;
        let stream = Stream::new(session_id, source_data, StreamConfig::default());

        let created_at = stream.created_at();
        assert!(created_at <= chrono::Utc::now());
    }

    #[test]
    fn test_updated_at_changes() {
        let session_id = SessionId::new();
        let source_data = JsonData::Null;
        let mut stream = Stream::new(session_id, source_data, StreamConfig::default());

        let initial_updated = stream.updated_at();
        std::thread::sleep(std::time::Duration::from_millis(10));

        stream.add_metadata("key".to_string(), "value".to_string());
        let after_metadata = stream.updated_at();

        assert!(after_metadata >= initial_updated);
    }

    #[test]
    fn test_completed_at_none_initially() {
        let session_id = SessionId::new();
        let source_data = JsonData::Null;
        let stream = Stream::new(session_id, source_data, StreamConfig::default());

        assert!(stream.completed_at().is_none());
    }

    #[test]
    fn test_completed_at_set_on_completion() {
        let session_id = SessionId::new();
        let source_data = JsonData::Null;
        let mut stream = Stream::new(session_id, source_data, StreamConfig::default());

        assert!(stream.start_streaming().is_ok());
        assert!(stream.complete().is_ok());

        assert!(stream.completed_at().is_some());
    }
}

mod stream_config_tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = StreamConfig::default();
        assert_eq!(config.max_frame_size, 64 * 1024);
        assert_eq!(config.max_frames_per_batch, 10);
        assert!(config.enable_compression);
        assert_eq!(config.priority_rules.len(), 0);
    }

    #[test]
    fn test_config_with_priority_rules() {
        let mut priority_rules = HashMap::new();
        priority_rules.insert("critical_path".to_string(), Priority::CRITICAL);
        priority_rules.insert("background_task".to_string(), Priority::BACKGROUND);

        let config = StreamConfig {
            priority_rules,
            ..Default::default()
        };

        assert_eq!(config.priority_rules.len(), 2);
        assert_eq!(
            config.priority_rules.get("critical_path"),
            Some(&Priority::CRITICAL)
        );
    }
}
