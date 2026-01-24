//! Comprehensive tests for StreamSession aggregate root
//!
//! Coverage targets:
//! - Session lifecycle (create, activate, close, expire)
//! - Stream management within sessions
//! - State transitions and validations
//! - Error cases and edge cases
//! - Event generation
//! - Statistics tracking

use pjson_rs::domain::{
    DomainError,
    aggregates::{StreamSession, stream_session::SessionConfig},
    entities::stream::StreamConfig as EntityStreamConfig,
    events::{DomainEvent, SessionState},
    value_objects::{JsonData, StreamId},
};
use std::collections::HashMap;

// Test fixtures
fn default_config() -> SessionConfig {
    SessionConfig::default()
}

fn custom_config(max_streams: usize, timeout: u64) -> SessionConfig {
    SessionConfig {
        max_concurrent_streams: max_streams,
        session_timeout_seconds: timeout,
        default_stream_config: EntityStreamConfig::default(),
        enable_compression: true,
        metadata: HashMap::new(),
    }
}

/// Helper to create JsonData objects for tests
fn json_data_object(pairs: &[(&str, JsonData)]) -> JsonData {
    let map: HashMap<String, JsonData> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect();
    JsonData::Object(map)
}

/// Helper to create simple string key-value JsonData
fn json_str(key: &str, value: &str) -> JsonData {
    json_data_object(&[(key, JsonData::String(value.to_string()))])
}

/// Helper to create simple integer key-value JsonData
fn json_int(key: &str, value: i64) -> JsonData {
    json_data_object(&[(key, JsonData::Integer(value))])
}

// ============================================================================
// Session Creation and Initialization
// ============================================================================

#[test]
fn test_new_session_initializing_state() {
    let session = StreamSession::new(default_config());

    assert_eq!(session.state(), &SessionState::Initializing);
    assert!(!session.is_active());
}

#[test]
fn test_new_session_has_unique_id() {
    let session1 = StreamSession::new(default_config());
    let session2 = StreamSession::new(default_config());

    assert_ne!(session1.id(), session2.id());
}

#[test]
fn test_new_session_timestamps_set() {
    let session = StreamSession::new(default_config());

    assert!(session.created_at() <= chrono::Utc::now());
    assert!(session.updated_at() <= chrono::Utc::now());
    assert!(session.expires_at() > session.created_at());
    assert_eq!(session.completed_at(), None);
}

#[test]
fn test_new_session_expiration_calculated() {
    let timeout_seconds = 7200;
    let config = custom_config(10, timeout_seconds);
    let session = StreamSession::new(config);

    let expected_duration = chrono::Duration::seconds(timeout_seconds as i64);
    let actual_duration = session.expires_at() - session.created_at();

    // Allow 1 second tolerance for test execution time
    assert!((actual_duration - expected_duration).num_seconds().abs() <= 1);
}

#[test]
fn test_new_session_empty_streams() {
    let session = StreamSession::new(default_config());

    assert_eq!(session.streams().len(), 0);
}

#[test]
fn test_new_session_default_stats() {
    let session = StreamSession::new(default_config());
    let stats = session.stats();

    assert_eq!(stats.total_streams, 0);
    assert_eq!(stats.active_streams, 0);
    assert_eq!(stats.completed_streams, 0);
    assert_eq!(stats.failed_streams, 0);
    assert_eq!(stats.total_frames, 0);
    assert_eq!(stats.total_bytes, 0);
    assert_eq!(stats.average_stream_duration_ms, 0.0);
}

#[test]
fn test_new_session_no_client_info() {
    let session = StreamSession::new(default_config());

    // Client info is private, but we can verify session is created
    assert!(!session.id().to_string().is_empty());
}

// ============================================================================
// Session Activation
// ============================================================================

#[test]
fn test_activate_from_initializing() {
    let mut session = StreamSession::new(default_config());

    let result = session.activate();

    assert!(result.is_ok());
    assert_eq!(session.state(), &SessionState::Active);
    assert!(session.is_active());
}

#[test]
fn test_activate_generates_event() {
    let mut session = StreamSession::new(default_config());

    session.activate().unwrap();

    let events = session.pending_events();
    assert!(!events.is_empty());

    // Should have SessionActivated event
    let has_activated_event = events
        .iter()
        .any(|e| matches!(e, DomainEvent::SessionActivated { .. }));
    assert!(has_activated_event);
}

#[test]
fn test_activate_from_active_fails() {
    let mut session = StreamSession::new(default_config());
    session.activate().unwrap();

    let result = session.activate();

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        DomainError::InvalidStateTransition(_)
    ));
}

#[test]
fn test_activate_updates_timestamp() {
    let mut session = StreamSession::new(default_config());
    let initial_update = session.updated_at();

    // Small delay to ensure timestamp difference
    std::thread::sleep(std::time::Duration::from_millis(10));

    session.activate().unwrap();

    assert!(session.updated_at() >= initial_update);
}

// ============================================================================
// Stream Creation
// ============================================================================

#[test]
fn test_create_stream_in_active_session() {
    let mut session = StreamSession::new(default_config());
    session.activate().unwrap();

    let data = json_str("key", "value");
    let result = session.create_stream(data);

    assert!(result.is_ok());
    let stream_id = result.unwrap();

    assert_eq!(session.streams().len(), 1);
    assert!(session.get_stream(stream_id).is_some());
}

#[test]
fn test_create_stream_updates_stats() {
    let mut session = StreamSession::new(default_config());
    session.activate().unwrap();

    let data = json_str("test", "data");
    session.create_stream(data).unwrap();

    assert_eq!(session.stats().total_streams, 1);
    assert_eq!(session.stats().active_streams, 1);
}

#[test]
fn test_create_stream_generates_event() {
    let mut session = StreamSession::new(default_config());
    session.activate().unwrap();
    session.take_events(); // Clear activation event

    let data = json_str("test", "data");
    session.create_stream(data).unwrap();

    let events = session.pending_events();
    let has_stream_created = events
        .iter()
        .any(|e| matches!(e, DomainEvent::StreamCreated { .. }));
    assert!(has_stream_created);
}

#[test]
fn test_create_stream_before_activation_fails() {
    let mut session = StreamSession::new(default_config());

    let data = json_str("key", "value");
    let result = session.create_stream(data);

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        DomainError::InvalidSessionState(_)
    ));
}

#[test]
fn test_create_stream_respects_max_concurrent() {
    let config = custom_config(2, 3600);
    let mut session = StreamSession::new(config);
    session.activate().unwrap();

    // Create max streams
    session.create_stream(json_int("stream", 1)).unwrap();
    session.create_stream(json_int("stream", 2)).unwrap();

    // Third should fail
    let result = session.create_stream(json_int("stream", 3));

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        DomainError::TooManyStreams(_)
    ));
}

#[test]
fn test_create_multiple_streams() {
    let mut session = StreamSession::new(default_config());
    session.activate().unwrap();

    let stream1 = session.create_stream(json_int("id", 1)).unwrap();
    let stream2 = session.create_stream(json_int("id", 2)).unwrap();
    let stream3 = session.create_stream(json_int("id", 3)).unwrap();

    assert_ne!(stream1, stream2);
    assert_ne!(stream2, stream3);
    assert_eq!(session.streams().len(), 3);
    assert_eq!(session.stats().total_streams, 3);
}

// ============================================================================
// Stream Operations
// ============================================================================

#[test]
fn test_start_stream_success() {
    let mut session = StreamSession::new(default_config());
    session.activate().unwrap();

    let stream_id = session.create_stream(json_str("test", "data")).unwrap();

    let result = session.start_stream(stream_id);

    assert!(result.is_ok());
}

#[test]
fn test_start_stream_generates_event() {
    let mut session = StreamSession::new(default_config());
    session.activate().unwrap();
    let stream_id = session.create_stream(json_str("test", "data")).unwrap();
    session.take_events(); // Clear previous events

    session.start_stream(stream_id).unwrap();

    let events = session.pending_events();
    let has_stream_started = events
        .iter()
        .any(|e| matches!(e, DomainEvent::StreamStarted { .. }));
    assert!(has_stream_started);
}

#[test]
fn test_start_nonexistent_stream_fails() {
    let mut session = StreamSession::new(default_config());
    session.activate().unwrap();

    let fake_stream_id = StreamId::new();
    let result = session.start_stream(fake_stream_id);

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        DomainError::StreamNotFound(_)
    ));
}

#[test]
fn test_complete_stream_success() {
    let mut session = StreamSession::new(default_config());
    session.activate().unwrap();

    let stream_id = session.create_stream(json_str("test", "data")).unwrap();
    session.start_stream(stream_id).unwrap();

    let result = session.complete_stream(stream_id);

    assert!(result.is_ok());
}

#[test]
fn test_complete_stream_updates_stats() {
    let mut session = StreamSession::new(default_config());
    session.activate().unwrap();

    let stream_id = session.create_stream(json_str("test", "data")).unwrap();
    session.start_stream(stream_id).unwrap();

    assert_eq!(session.stats().active_streams, 1);
    assert_eq!(session.stats().completed_streams, 0);

    session.complete_stream(stream_id).unwrap();

    assert_eq!(session.stats().active_streams, 0);
    assert_eq!(session.stats().completed_streams, 1);
}

#[test]
fn test_complete_stream_generates_event() {
    let mut session = StreamSession::new(default_config());
    session.activate().unwrap();
    let stream_id = session.create_stream(json_str("test", "data")).unwrap();
    session.start_stream(stream_id).unwrap();
    session.take_events(); // Clear previous events

    session.complete_stream(stream_id).unwrap();

    let events = session.pending_events();
    let has_stream_completed = events
        .iter()
        .any(|e| matches!(e, DomainEvent::StreamCompleted { .. }));
    assert!(has_stream_completed);
}

#[test]
fn test_fail_stream_success() {
    let mut session = StreamSession::new(default_config());
    session.activate().unwrap();

    let stream_id = session.create_stream(json_str("test", "data")).unwrap();
    session.start_stream(stream_id).unwrap();

    let result = session.fail_stream(stream_id, "test error".to_string());

    assert!(result.is_ok());
}

#[test]
fn test_fail_stream_updates_stats() {
    let mut session = StreamSession::new(default_config());
    session.activate().unwrap();

    let stream_id = session.create_stream(json_str("test", "data")).unwrap();
    session.start_stream(stream_id).unwrap();

    assert_eq!(session.stats().active_streams, 1);
    assert_eq!(session.stats().failed_streams, 0);

    session.fail_stream(stream_id, "error".to_string()).unwrap();

    assert_eq!(session.stats().active_streams, 0);
    assert_eq!(session.stats().failed_streams, 1);
}

#[test]
fn test_fail_stream_generates_event() {
    let mut session = StreamSession::new(default_config());
    session.activate().unwrap();
    let stream_id = session.create_stream(json_str("test", "data")).unwrap();
    session.start_stream(stream_id).unwrap();
    session.take_events(); // Clear previous events

    session
        .fail_stream(stream_id, "test error".to_string())
        .unwrap();

    let events = session.pending_events();
    let has_stream_failed = events
        .iter()
        .any(|e| matches!(e, DomainEvent::StreamFailed { .. }));
    assert!(has_stream_failed);
}

// ============================================================================
// Session Closure
// ============================================================================

#[test]
fn test_close_active_session() {
    let mut session = StreamSession::new(default_config());
    session.activate().unwrap();

    let result = session.close();

    assert!(result.is_ok());
    assert_eq!(session.state(), &SessionState::Completed);
    assert!(session.completed_at().is_some());
}

#[test]
fn test_close_session_generates_event() {
    let mut session = StreamSession::new(default_config());
    session.activate().unwrap();
    session.take_events(); // Clear previous events

    session.close().unwrap();

    let events = session.pending_events();
    let has_closed_event = events
        .iter()
        .any(|e| matches!(e, DomainEvent::SessionClosed { .. }));
    assert!(has_closed_event);
}

#[test]
fn test_close_before_activation_fails() {
    let mut session = StreamSession::new(default_config());

    let result = session.close();

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        DomainError::InvalidStateTransition(_)
    ));
}

#[test]
fn test_close_cancels_active_streams() {
    let mut session = StreamSession::new(default_config());
    session.activate().unwrap();

    // Create and start streams
    let stream_id1 = session.create_stream(json_int("id", 1)).unwrap();
    let stream_id2 = session.create_stream(json_int("id", 2)).unwrap();
    session.start_stream(stream_id1).unwrap();
    session.start_stream(stream_id2).unwrap();

    session.close().unwrap();

    // Streams should be canceled (checked via state)
    assert_eq!(session.state(), &SessionState::Completed);
}

// ============================================================================
// Session Expiration
// ============================================================================

#[test]
fn test_is_expired_initially_false() {
    let session = StreamSession::new(default_config());

    assert!(!session.is_expired());
}

#[test]
fn test_is_active_expired_session() {
    let config = custom_config(10, 0); // Zero timeout for immediate expiry
    let mut session = StreamSession::new(config);
    session.activate().unwrap();

    // Small delay to ensure expiration
    std::thread::sleep(std::time::Duration::from_millis(10));

    // Session is expired, so not active
    assert!(session.is_expired());
    assert!(!session.is_active());
}

#[test]
fn test_force_close_expired_success() {
    let config = custom_config(10, 0);
    let mut session = StreamSession::new(config);
    session.activate().unwrap();

    std::thread::sleep(std::time::Duration::from_millis(10));

    let result = session.force_close_expired();

    assert!(result.is_ok());
    assert!(result.unwrap());
    assert_eq!(session.state(), &SessionState::Failed);
    assert!(session.completed_at().is_some());
}

#[test]
fn test_force_close_non_expired_no_op() {
    let mut session = StreamSession::new(default_config());
    session.activate().unwrap();

    let result = session.force_close_expired();

    assert!(result.is_ok());
    assert!(!result.unwrap());
    assert_eq!(session.state(), &SessionState::Active);
}

#[test]
fn test_force_close_expired_generates_event() {
    let config = custom_config(10, 0);
    let mut session = StreamSession::new(config);
    session.activate().unwrap();

    std::thread::sleep(std::time::Duration::from_millis(10));
    session.take_events(); // Clear previous events

    session.force_close_expired().unwrap();

    let events = session.pending_events();
    let has_timeout_event = events
        .iter()
        .any(|e| matches!(e, DomainEvent::SessionTimedOut { .. }));
    assert!(has_timeout_event);
}

#[test]
fn test_force_close_expired_clears_streams() {
    // Use longer timeout to create stream before expiration
    let config = custom_config(10, 1); // 1 second timeout
    let mut session = StreamSession::new(config);
    session.activate().unwrap();

    // Create stream while session is still active
    let _stream_id = session.create_stream(json_str("test", "data")).unwrap();
    assert_eq!(session.streams().len(), 1);

    // Wait for expiration
    std::thread::sleep(std::time::Duration::from_millis(1100));

    session.force_close_expired().unwrap();

    assert_eq!(session.streams().len(), 0);
}

#[test]
fn test_extend_timeout_success() {
    let mut session = StreamSession::new(default_config());
    session.activate().unwrap();

    let initial_expires = session.expires_at();

    let result = session.extend_timeout(1800);

    assert!(result.is_ok());
    assert!(session.expires_at() > initial_expires);
}

#[test]
fn test_extend_timeout_generates_event() {
    let mut session = StreamSession::new(default_config());
    session.activate().unwrap();
    session.take_events(); // Clear previous events

    session.extend_timeout(1800).unwrap();

    let events = session.pending_events();
    let has_extend_event = events
        .iter()
        .any(|e| matches!(e, DomainEvent::SessionTimeoutExtended { .. }));
    assert!(has_extend_event);
}

#[test]
fn test_extend_timeout_on_expired_fails() {
    let config = custom_config(10, 0);
    let mut session = StreamSession::new(config);
    session.activate().unwrap();

    std::thread::sleep(std::time::Duration::from_millis(10));

    let result = session.extend_timeout(1800);

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        DomainError::InvalidStateTransition(_)
    ));
}

// ============================================================================
// Event Management
// ============================================================================

#[test]
fn test_take_events_clears_queue() {
    let mut session = StreamSession::new(default_config());
    session.activate().unwrap();

    assert!(!session.pending_events().is_empty());

    let events = session.take_events();

    assert!(!events.is_empty());
    assert!(session.pending_events().is_empty());
}

#[test]
fn test_events_contain_session_id() {
    let mut session = StreamSession::new(default_config());
    let session_id = session.id();

    session.activate().unwrap();

    let events = session.take_events();
    for event in events {
        assert_eq!(event.session_id(), session_id);
    }
}

// ============================================================================
// Health Check
// ============================================================================

#[test]
fn test_health_check_active_session() {
    let mut session = StreamSession::new(default_config());
    session.activate().unwrap();

    let health = session.health_check();

    assert!(health.is_healthy);
    assert_eq!(health.active_streams, 0);
    assert_eq!(health.failed_streams, 0);
    assert!(!health.is_expired);
    assert!(health.uptime_seconds >= 0);
}

#[test]
fn test_health_check_with_active_streams() {
    let mut session = StreamSession::new(default_config());
    session.activate().unwrap();

    let stream1 = session.create_stream(json_int("id", 1)).unwrap();
    let stream2 = session.create_stream(json_int("id", 2)).unwrap();

    // Start streams to make them active
    session.start_stream(stream1).unwrap();
    session.start_stream(stream2).unwrap();

    let health = session.health_check();

    assert_eq!(health.active_streams, 2); // Started streams are active
}

#[test]
fn test_health_check_expired_session() {
    let config = custom_config(10, 0);
    let mut session = StreamSession::new(config);
    session.activate().unwrap();

    std::thread::sleep(std::time::Duration::from_millis(10));

    let health = session.health_check();

    assert!(health.is_expired);
    assert!(!health.is_healthy);
}

// ============================================================================
// Session Metadata
// ============================================================================

#[test]
fn test_set_client_info() {
    let mut session = StreamSession::new(default_config());

    session.set_client_info(
        "Test Client".to_string(),
        Some("Mozilla/5.0".to_string()),
        Some("127.0.0.1".to_string()),
    );

    // Client info is private, but we can verify it doesn't panic
    assert!(!session.id().to_string().is_empty());
}

#[test]
fn test_set_client_info_updates_timestamp() {
    let mut session = StreamSession::new(default_config());
    let initial_update = session.updated_at();

    std::thread::sleep(std::time::Duration::from_millis(10));

    session.set_client_info("Test Client".to_string(), None, None);

    assert!(session.updated_at() >= initial_update);
}

// ============================================================================
// Session Duration
// ============================================================================

#[test]
fn test_duration_none_when_not_completed() {
    let session = StreamSession::new(default_config());

    assert_eq!(session.duration(), None);
}

#[test]
fn test_duration_some_when_completed() {
    let mut session = StreamSession::new(default_config());
    session.activate().unwrap();

    std::thread::sleep(std::time::Duration::from_millis(10));

    session.close().unwrap();

    let duration = session.duration();
    assert!(duration.is_some());
    assert!(duration.unwrap().num_milliseconds() >= 10);
}

// ============================================================================
// Stream Accessors
// ============================================================================

#[test]
fn test_get_stream_nonexistent() {
    let mut session = StreamSession::new(default_config());
    session.activate().unwrap();

    let fake_id = StreamId::new();

    assert!(session.get_stream(fake_id).is_none());
}

#[test]
fn test_get_stream_mut_nonexistent() {
    let mut session = StreamSession::new(default_config());
    session.activate().unwrap();

    let fake_id = StreamId::new();

    assert!(session.get_stream_mut(fake_id).is_none());
}

// ============================================================================
// Priority Frame Creation (Complex Scenario)
// ============================================================================

#[test]
fn test_create_priority_frames_inactive_session() {
    let mut session = StreamSession::new(default_config());

    let result = session.create_priority_frames(10);

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        DomainError::InvalidSessionState(_)
    ));
}

#[test]
fn test_create_priority_frames_no_active_streams() {
    let mut session = StreamSession::new(default_config());
    session.activate().unwrap();

    let frames = session.create_priority_frames(10).unwrap();

    assert_eq!(frames.len(), 0);
}

#[test]
fn test_create_priority_frames_updates_stats() {
    let mut session = StreamSession::new(default_config());
    session.activate().unwrap();

    let stream_id = session.create_stream(json_str("test", "data")).unwrap();
    session.start_stream(stream_id).unwrap();

    let initial_frame_count = session.stats().total_frames;

    // Try to create frames (may be empty depending on stream state)
    let _ = session.create_priority_frames(5);

    // Stats should be consistent (no panic)
    assert!(session.stats().total_frames >= initial_frame_count);
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_multiple_stream_completions() {
    let mut session = StreamSession::new(default_config());
    session.activate().unwrap();

    let stream1 = session.create_stream(json_int("id", 1)).unwrap();
    let stream2 = session.create_stream(json_int("id", 2)).unwrap();
    let stream3 = session.create_stream(json_int("id", 3)).unwrap();

    session.start_stream(stream1).unwrap();
    session.start_stream(stream2).unwrap();
    session.start_stream(stream3).unwrap();

    session.complete_stream(stream1).unwrap();
    session.complete_stream(stream2).unwrap();
    session.complete_stream(stream3).unwrap();

    assert_eq!(session.stats().completed_streams, 3);
    assert_eq!(session.stats().active_streams, 0);
}

#[test]
fn test_mixed_stream_outcomes() {
    let mut session = StreamSession::new(default_config());
    session.activate().unwrap();

    let stream1 = session.create_stream(json_int("id", 1)).unwrap();
    let stream2 = session.create_stream(json_int("id", 2)).unwrap();
    let stream3 = session.create_stream(json_int("id", 3)).unwrap();

    session.start_stream(stream1).unwrap();
    session.start_stream(stream2).unwrap();
    session.start_stream(stream3).unwrap();

    session.complete_stream(stream1).unwrap();
    session.fail_stream(stream2, "error".to_string()).unwrap();
    session.complete_stream(stream3).unwrap();

    assert_eq!(session.stats().completed_streams, 2);
    assert_eq!(session.stats().failed_streams, 1);
    assert_eq!(session.stats().active_streams, 0);
}

#[test]
fn test_session_config_metadata() {
    let mut metadata = HashMap::new();
    metadata.insert("key1".to_string(), "value1".to_string());
    metadata.insert("key2".to_string(), "value2".to_string());

    let config = SessionConfig {
        max_concurrent_streams: 10,
        session_timeout_seconds: 3600,
        default_stream_config: EntityStreamConfig::default(),
        enable_compression: false,
        metadata: metadata.clone(),
    };

    let session = StreamSession::new(config);

    assert_eq!(session.config().metadata, metadata);
    assert!(!session.config().enable_compression);
}
