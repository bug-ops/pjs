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
    ports::TimeProvider,
    value_objects::{JsonData, Priority, StreamId},
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

// Test fixtures
fn default_config() -> SessionConfig {
    SessionConfig::default()
}

/// Fake clock for deterministic expiry/timeout/timestamp assertions.
///
/// Replaces `std::thread::sleep` in tests that need "now" to advance: the
/// test controls exactly how far the clock moves instead of hoping a real
/// delay was long enough. Share one instance behind `Arc<ManualTimeProvider>`
/// between the test and the `StreamSession` it drives — the outer `Arc`
/// (required to satisfy `Arc<dyn TimeProvider>`) is the only indirection
/// needed, so this type does not implement `Clone`.
struct ManualTimeProvider {
    now: Mutex<chrono::DateTime<chrono::Utc>>,
}

impl ManualTimeProvider {
    /// Seeds from a fixed instant (the Unix epoch), not real wall-clock
    /// time, so the fake clock's starting value is itself deterministic.
    fn new() -> Self {
        Self {
            now: Mutex::new(
                chrono::DateTime::from_timestamp(0, 0).expect("epoch is a valid timestamp"),
            ),
        }
    }

    fn advance(&self, duration: chrono::Duration) {
        *self.now.lock().unwrap() += duration;
    }
}

impl TimeProvider for ManualTimeProvider {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        *self.now.lock().unwrap()
    }
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

/// Session config whose default stream config carries the given per-field
/// `priority_rules` overrides, so patch priority is fully deterministic
/// regardless of `JsonData::Object`'s `HashMap`-backed (unordered) field
/// iteration.
fn config_with_priority_rules(rules: HashMap<String, Priority>) -> SessionConfig {
    let default_stream_config = EntityStreamConfig {
        priority_rules: rules,
        ..EntityStreamConfig::default()
    };
    SessionConfig {
        default_stream_config,
        ..default_config()
    }
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
    let clock = Arc::new(ManualTimeProvider::new());
    let mut session = StreamSession::with_time_provider(default_config(), clock.clone());
    let initial_update = session.updated_at();

    clock.advance(chrono::Duration::milliseconds(10));

    session.activate().unwrap();

    assert!(session.updated_at() > initial_update);
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
    assert!(session.stream(stream_id).is_some());
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
    let clock = Arc::new(ManualTimeProvider::new());
    let mut session = StreamSession::with_time_provider(config, clock.clone());
    session.activate().unwrap();

    clock.advance(chrono::Duration::milliseconds(10));

    // Session is expired, so not active
    assert!(session.is_expired());
    assert!(!session.is_active());
}

#[test]
fn test_force_close_expired_success() {
    let config = custom_config(10, 0);
    let clock = Arc::new(ManualTimeProvider::new());
    let mut session = StreamSession::with_time_provider(config, clock.clone());
    session.activate().unwrap();

    clock.advance(chrono::Duration::milliseconds(10));

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
    let clock = Arc::new(ManualTimeProvider::new());
    let mut session = StreamSession::with_time_provider(config, clock.clone());
    session.activate().unwrap();

    clock.advance(chrono::Duration::milliseconds(10));
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
    let clock = Arc::new(ManualTimeProvider::new());
    let mut session = StreamSession::with_time_provider(config, clock.clone());
    session.activate().unwrap();

    // Create stream while session is still active
    let _stream_id = session.create_stream(json_str("test", "data")).unwrap();
    assert_eq!(session.streams().len(), 1);

    // Advance the clock past expiration
    clock.advance(chrono::Duration::milliseconds(1100));

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
    let clock = Arc::new(ManualTimeProvider::new());
    let mut session = StreamSession::with_time_provider(config, clock.clone());
    session.activate().unwrap();

    clock.advance(chrono::Duration::milliseconds(10));

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
    let clock = Arc::new(ManualTimeProvider::new());
    let mut session = StreamSession::with_time_provider(config, clock.clone());
    session.activate().unwrap();

    clock.advance(chrono::Duration::milliseconds(10));

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
    let clock = Arc::new(ManualTimeProvider::new());
    let mut session = StreamSession::with_time_provider(default_config(), clock.clone());
    let initial_update = session.updated_at();

    clock.advance(chrono::Duration::milliseconds(10));

    session.set_client_info("Test Client".to_string(), None, None);

    assert!(session.updated_at() > initial_update);
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
    let clock = Arc::new(ManualTimeProvider::new());
    let mut session = StreamSession::with_time_provider(default_config(), clock.clone());
    session.activate().unwrap();

    clock.advance(chrono::Duration::milliseconds(10));

    session.close().unwrap();

    let duration = session.duration();
    assert!(duration.is_some());
    assert_eq!(duration.unwrap().num_milliseconds(), 10);
}

// ============================================================================
// Stream Accessors
// ============================================================================

#[test]
fn test_stream_nonexistent() {
    let mut session = StreamSession::new(default_config());
    session.activate().unwrap();

    let fake_id = StreamId::new();

    assert!(session.stream(fake_id).is_none());
}

// ============================================================================
// Stream Patch Frame Creation
// ============================================================================

#[test]
fn test_create_stream_patch_frames_updates_total_bytes() {
    let mut session = StreamSession::new(default_config());
    session.activate().unwrap();

    let stream_id = session
        .create_stream(json_str(
            "payload",
            "hello world payload, quite a few bytes here",
        ))
        .unwrap();
    session.start_stream(stream_id).unwrap();

    assert_eq!(session.stats().total_bytes, 0);

    let frames = session
        .create_stream_patch_frames(stream_id, Priority::LOW, 100)
        .unwrap();

    assert!(!frames.is_empty());
    let expected_bytes: u64 = frames.iter().map(|f| f.estimated_size() as u64).sum();
    assert_eq!(session.stats().total_bytes, expected_bytes);
    assert!(session.stats().total_bytes > 0);
}

#[test]
fn test_create_stream_patch_frames_zero_max_frames_does_not_increment_total_bytes() {
    let mut session = StreamSession::new(default_config());
    session.activate().unwrap();

    let stream_id = session
        .create_stream(json_str(
            "payload",
            "hello world payload, quite a few bytes here",
        ))
        .unwrap();
    session.start_stream(stream_id).unwrap();

    let frames = session
        .create_stream_patch_frames(stream_id, Priority::LOW, 0)
        .unwrap();

    assert!(frames.is_empty());
    assert_eq!(session.stats().total_frames, 0);
    assert_eq!(session.stats().total_bytes, 0);
}

// ============================================================================
// Priority Frame Creation (Complex Scenario)
// ============================================================================

#[test]
fn test_create_priority_frames_inactive_session() {
    let mut session = StreamSession::new(default_config());

    let result = session.create_priority_frames(Priority::BACKGROUND, 10);

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

    let frames = session
        .create_priority_frames(Priority::BACKGROUND, 10)
        .unwrap();

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
    let _ = session.create_priority_frames(Priority::BACKGROUND, 5);

    // Stats should be consistent (no panic)
    assert!(session.stats().total_frames >= initial_frame_count);
}

/// `create_priority_frames` generates frames per stream, then globally sorts
/// by priority and truncates to `batch_size` — discarding some generated
/// frames. Unlike `create_stream_patch_frames` (a simple before/after delta
/// of the child stream's own byte counter), `total_bytes` here must equal
/// the sum over only the frames actually retained, strictly less than the
/// sum over every frame every stream generated.
#[test]
fn test_create_priority_frames_updates_total_bytes_excludes_discarded_frames() {
    let mut session = StreamSession::new(default_config());
    session.activate().unwrap();

    // Three streams, each with two scalar leaf fields: with `create_priority_frames`'s
    // hardcoded `max_frames = 5` per stream, 2 patches per stream chunk to 2 frames
    // each (chunk_size = ceil(2/5) = 1), so 6 frames are generated in total.
    let mut stream_ids = Vec::new();
    for i in 0..3 {
        let payload = json_data_object(&[
            (
                "field_a",
                JsonData::String(format!("stream-{i}-field-a-payload-value")),
            ),
            (
                "field_b",
                JsonData::String(format!("stream-{i}-field-b-payload-value")),
            ),
        ]);
        let stream_id = session.create_stream(payload).unwrap();
        session.start_stream(stream_id).unwrap();
        stream_ids.push(stream_id);
    }

    // Retain fewer frames than the 6 generated, forcing truncation.
    let batch_size = 3;
    let frames = session
        .create_priority_frames(Priority::BACKGROUND, batch_size)
        .unwrap();
    assert_eq!(frames.len(), batch_size);

    let expected_bytes: u64 = frames.iter().map(|f| f.estimated_size() as u64).sum();
    assert_eq!(session.stats().total_bytes, expected_bytes);

    // Regression for #506: child stream stats must only reflect frames that
    // actually survived truncation, not every frame the stream built —
    // otherwise per-stream stats and `next_sequence` desync from what the
    // caller actually received.
    let total_child_bytes: u64 = stream_ids
        .iter()
        .map(|id| session.stream(*id).unwrap().stats().total_bytes)
        .sum();
    assert_eq!(
        session.stats().total_bytes,
        total_child_bytes,
        "child stream stats must exclude frames discarded by batch_size truncation, matching session-level total_bytes"
    );

    let total_child_frames: u64 = stream_ids
        .iter()
        .map(|id| session.stream(*id).unwrap().stats().total_frames)
        .sum();
    assert_eq!(
        total_child_frames,
        frames.len() as u64,
        "child stream total_frames must sum to exactly the frames actually returned, not the 6 built"
    );
}

/// Regression test for #477: a stream still `Preparing` (created but never
/// started) must not abort the whole batch — `create_priority_frames` skips
/// non-`Streaming` streams rather than erroring, and must produce frames
/// from the `Streaming` stream alongside it, with session stats reflecting
/// exactly the streams that actually contributed.
#[test]
fn test_create_priority_frames_skips_preparing_stream_without_erroring() {
    let mut session = StreamSession::new(default_config());
    session.activate().unwrap();

    let streaming_id = session.create_stream(json_str("field", "value")).unwrap();
    session.start_stream(streaming_id).unwrap();

    // Left in `Preparing` — never started.
    let _preparing_id = session.create_stream(json_str("other", "value")).unwrap();

    let frames = session
        .create_priority_frames(Priority::BACKGROUND, 10)
        .unwrap();

    assert!(
        !frames.is_empty(),
        "the Streaming stream's frames must still be produced"
    );
    assert!(frames.iter().all(|f| f.stream_id() == streaming_id));
    assert_eq!(session.stats().total_frames, frames.len() as u64);
}

/// Regression test for #506: a single `commit_patch_frames` call producing
/// multiple frames for the *same* stream (patch count exceeding `max_frames`,
/// forcing more than one chunk) must assign each frame a distinct, strictly
/// increasing sequence number. Before the fix, every frame built by one call
/// read `next_sequence` before any of them incremented it, so they all
/// collided on the same value.
#[test]
fn test_create_stream_patch_frames_multiple_frames_have_unique_increasing_sequences() {
    let mut session = StreamSession::new(default_config());
    session.activate().unwrap();

    let payload = json_data_object(&[
        ("f1", JsonData::String("value-1".to_string())),
        ("f2", JsonData::String("value-2".to_string())),
        ("f3", JsonData::String("value-3".to_string())),
        ("f4", JsonData::String("value-4".to_string())),
        ("f5", JsonData::String("value-5".to_string())),
        ("f6", JsonData::String("value-6".to_string())),
    ]);
    let stream_id = session.create_stream(payload).unwrap();
    session.start_stream(stream_id).unwrap();

    // 6 patches, max_frames = 2 => chunk_size = ceil(6/2) = 3 => 2 frames.
    let frames = session
        .create_stream_patch_frames(stream_id, Priority::BACKGROUND, 2)
        .unwrap();

    assert_eq!(frames.len(), 2, "setup must produce 2 frames from one call");
    let mut sequences: Vec<u64> = frames.iter().map(|f| f.sequence()).collect();
    let unique_count = {
        let mut s = sequences.clone();
        s.sort_unstable();
        s.dedup();
        s.len()
    };
    assert_eq!(
        unique_count,
        sequences.len(),
        "frames from one call must not share sequence numbers: {sequences:?}"
    );
    sequences.sort_unstable();
    assert_eq!(
        sequences,
        vec![1, 2],
        "sequence numbers must be consecutive starting at 1"
    );
}

/// Regression test for #506: when a truncating `create_priority_frames` pass
/// keeps only some of a stream's built candidates (here, 1 of 3 for the
/// lower-priority stream, all 3 for the higher-priority one), each stream's
/// kept frames must carry strictly consecutive sequence numbers with no
/// gaps, and a later call on the same stream must continue from the count of
/// frames actually kept — not the count built and discarded.
#[test]
fn test_create_priority_frames_next_sequence_continues_from_kept_count() {
    let mut rules = HashMap::new();
    rules.insert("hi_1".to_string(), Priority::new(90).unwrap());
    rules.insert("hi_2".to_string(), Priority::new(90).unwrap());
    rules.insert("hi_3".to_string(), Priority::new(90).unwrap());
    rules.insert("lo_1".to_string(), Priority::new(15).unwrap());
    rules.insert("lo_2".to_string(), Priority::new(15).unwrap());
    rules.insert("lo_3".to_string(), Priority::new(15).unwrap());
    let mut session = StreamSession::new(config_with_priority_rules(rules));
    session.activate().unwrap();

    let hi_payload = json_data_object(&[
        ("hi_1", JsonData::String("a".to_string())),
        ("hi_2", JsonData::String("b".to_string())),
        ("hi_3", JsonData::String("c".to_string())),
    ]);
    let hi_id = session.create_stream(hi_payload).unwrap();
    session.start_stream(hi_id).unwrap();

    let lo_payload = json_data_object(&[
        ("lo_1", JsonData::String("a".to_string())),
        ("lo_2", JsonData::String("b".to_string())),
        ("lo_3", JsonData::String("c".to_string())),
    ]);
    let lo_id = session.create_stream(lo_payload).unwrap();
    session.start_stream(lo_id).unwrap();

    // Each stream has 3 patches, hardcoded per-stream max_frames = 5 inside
    // `commit_priority_frames` => chunk_size = 1 => 3 candidate frames per
    // stream, all sharing that stream's override priority. batch_size = 4
    // keeps all 3 hi-priority candidates plus exactly 1 lo-priority one.
    let frames = session
        .create_priority_frames(Priority::BACKGROUND, 4)
        .unwrap();
    assert_eq!(frames.len(), 4);

    let hi_frames: Vec<u64> = {
        let mut v: Vec<u64> = frames
            .iter()
            .filter(|f| f.stream_id() == hi_id)
            .map(|f| f.sequence())
            .collect();
        v.sort_unstable();
        v
    };
    let lo_frames: Vec<u64> = {
        let mut v: Vec<u64> = frames
            .iter()
            .filter(|f| f.stream_id() == lo_id)
            .map(|f| f.sequence())
            .collect();
        v.sort_unstable();
        v
    };

    assert_eq!(
        hi_frames,
        vec![1, 2, 3],
        "the fully-kept hi-priority stream must have consecutive sequences 1..=3"
    );
    assert_eq!(
        lo_frames.len(),
        1,
        "exactly 1 of the lo-priority stream's 3 candidates must survive truncation"
    );
    assert_eq!(
        lo_frames[0], 1,
        "the lo-priority stream's single kept frame must be sequence 1 (its first ever frame)"
    );

    assert_eq!(session.stream(hi_id).unwrap().stats().total_frames, 3);
    assert_eq!(session.stream(lo_id).unwrap().stats().total_frames, 1);

    // A further call on the lo-priority stream must continue from the count
    // actually kept (1), not the count built and discarded (3): the bug
    // would have advanced `next_sequence` to 4 (1 kept + 3 built-but-dropped
    // candidates from this same stream in the earlier call), so the next
    // frame would start at 4 instead of 2.
    let next_frame = session
        .create_stream_patch_frames(lo_id, Priority::BACKGROUND, 16)
        .unwrap();
    let min_next_sequence = next_frame.iter().map(|f| f.sequence()).min().unwrap();
    assert_eq!(
        min_next_sequence,
        2,
        "next_sequence must continue from the 1 frame actually kept, not the 3 built, got {:?}",
        next_frame.iter().map(|f| f.sequence()).collect::<Vec<_>>()
    );
}

/// Edge case regression for #506: a stream whose *every* candidate is
/// discarded by truncation must have its stats and `next_sequence` left
/// byte-for-byte unchanged by the call that discarded them.
#[test]
fn test_create_priority_frames_fully_discarded_stream_stats_unchanged() {
    let mut rules = HashMap::new();
    rules.insert("hi_1".to_string(), Priority::new(90).unwrap());
    rules.insert("hi_2".to_string(), Priority::new(90).unwrap());
    rules.insert("hi_3".to_string(), Priority::new(90).unwrap());
    rules.insert("lo_1".to_string(), Priority::new(15).unwrap());
    rules.insert("lo_2".to_string(), Priority::new(15).unwrap());
    rules.insert("lo_3".to_string(), Priority::new(15).unwrap());
    rules.insert("warmup".to_string(), Priority::new(15).unwrap());
    let mut session = StreamSession::new(config_with_priority_rules(rules));
    session.activate().unwrap();

    let hi_payload = json_data_object(&[
        ("hi_1", JsonData::String("a".to_string())),
        ("hi_2", JsonData::String("b".to_string())),
        ("hi_3", JsonData::String("c".to_string())),
    ]);
    let hi_id = session.create_stream(hi_payload).unwrap();
    session.start_stream(hi_id).unwrap();

    let lo_payload = json_data_object(&[
        ("lo_1", JsonData::String("a".to_string())),
        ("lo_2", JsonData::String("b".to_string())),
        ("lo_3", JsonData::String("c".to_string())),
        ("warmup", JsonData::String("pre-existing".to_string())),
    ]);
    let lo_id = session.create_stream(lo_payload).unwrap();
    session.start_stream(lo_id).unwrap();

    // Commit a single warm-up frame on the lo-priority stream directly, so
    // "unchanged by the batch call" is a non-trivial claim (not just "still
    // zero").
    let warmup_frames = session
        .create_stream_patch_frames(lo_id, Priority::new(15).unwrap(), 1)
        .unwrap();
    assert_eq!(warmup_frames.len(), 1);
    let stats_before = session.stream(lo_id).unwrap().stats().clone();
    assert_eq!(stats_before.total_frames, 1);

    // Extraction re-reads `source_data` rather than consuming it, so this
    // call re-extracts all 4 lo-stream patches (lo_1..lo_3 plus warmup, all
    // priority 15) as candidates, alongside the hi stream's 3 candidates
    // (priority 90). batch_size = 3 keeps only the hi stream's frames,
    // discarding all 4 lo candidates.
    let frames = session
        .create_priority_frames(Priority::BACKGROUND, 3)
        .unwrap();
    assert_eq!(frames.len(), 3);
    assert!(frames.iter().all(|f| f.stream_id() == hi_id));

    let stats_after = session.stream(lo_id).unwrap().stats().clone();
    assert_eq!(
        stats_after.total_frames, stats_before.total_frames,
        "fully-discarded stream's total_frames must be unchanged"
    );
    assert_eq!(
        stats_after.total_bytes, stats_before.total_bytes,
        "fully-discarded stream's total_bytes must be unchanged"
    );

    // next_sequence must still continue from the 1 warm-up frame, not from
    // 1 + 3 discarded candidates.
    let post_frame = session
        .create_stream_patch_frames(lo_id, Priority::new(15).unwrap(), 16)
        .unwrap();
    assert!(
        post_frame.iter().any(|f| f.sequence() == 2),
        "next committed frame on the fully-discarded stream must reuse sequence 2, got {:?}",
        post_frame.iter().map(|f| f.sequence()).collect::<Vec<_>>()
    );
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
