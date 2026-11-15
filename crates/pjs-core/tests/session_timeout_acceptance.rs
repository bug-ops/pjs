//! Acceptance tests for #596 (`SessionTimeout` newtype) and #573 (session/stream
//! id cross-check on decode).
//!
//! Assumed path: `pjson_rs::domain::aggregates::stream_session::SessionTimeout`.

use pjson_rs::domain::aggregates::StreamSession;
use pjson_rs::domain::aggregates::stream_session::{SessionConfig, SessionTimeout};
use pjson_rs::domain::value_objects::{JsonData, SessionId};
use serde_json::{Value, json};

const MAX_SECONDS: u64 = 604_800;

fn config_json(timeout: Value) -> Value {
    let mut value = serde_json::to_value(SessionConfig::default()).expect("config encodes");
    value["session_timeout_seconds"] = timeout;
    value
}

#[test]
fn test_session_timeout_try_from_accepts_bounds() {
    assert!(SessionTimeout::try_from(1u64).is_ok());
    assert!(SessionTimeout::try_from(MAX_SECONDS).is_ok());
}

#[test]
fn test_session_timeout_try_from_rejects_out_of_range() {
    for value in [0u64, MAX_SECONDS + 1, 9_223_372_036_854_776, u64::MAX] {
        assert!(
            SessionTimeout::try_from(value).is_err(),
            "{value} must be rejected"
        );
    }
}

#[test]
fn test_session_timeout_as_duration_matches_seconds() {
    let timeout = SessionTimeout::try_from(1800u64).expect("in range");
    assert_eq!(timeout.as_duration(), std::time::Duration::from_secs(1800));
}

#[test]
fn test_session_config_decode_rejects_out_of_range_timeout() {
    for value in [json!(0u64), json!(MAX_SECONDS + 1), json!(u64::MAX)] {
        let result = serde_json::from_value::<SessionConfig>(config_json(value.clone()));
        assert!(result.is_err(), "{value} must fail to decode");
    }
}

#[test]
fn test_session_config_wire_key_and_value_unchanged() {
    let decoded: SessionConfig =
        serde_json::from_value(config_json(json!(1800u64))).expect("in range decodes");
    let encoded = serde_json::to_value(&decoded).expect("encodes");
    assert_eq!(encoded["session_timeout_seconds"], json!(1800u64));
}

#[test]
fn test_stream_session_new_with_max_timeout_is_not_expired() {
    let config: SessionConfig =
        serde_json::from_value(config_json(json!(MAX_SECONDS))).expect("max decodes");
    let session = StreamSession::new(config);
    assert!(!session.is_expired());
}

#[test]
fn test_extend_timeout_moves_expiry_strictly_forward() {
    let mut session = StreamSession::new(SessionConfig::default());
    let before = session.expires_at();
    session
        .extend_timeout(SessionTimeout::try_from(60u64).expect("in range"))
        .expect("extension succeeds");
    assert!(session.expires_at() > before);
}

fn session_json_with_stream() -> (Value, String) {
    let mut session = StreamSession::new(SessionConfig::default());
    session.activate().expect("activates");
    let stream_id = session
        .create_stream(JsonData::Null)
        .expect("stream created");
    (
        serde_json::to_value(&session).expect("session encodes"),
        stream_id.as_uuid().to_string(),
    )
}

#[test]
fn test_stream_session_decode_rejects_stream_with_foreign_session_id() {
    let (mut value, stream_uuid) = session_json_with_stream();
    let foreign = SessionId::new().as_uuid().to_string();
    value["streams"][&stream_uuid]["session_id"] = json!(foreign);
    assert!(serde_json::from_value::<StreamSession>(value).is_err());
}

#[test]
fn test_stream_session_decode_accepts_consistent_ids() {
    let (value, _) = session_json_with_stream();
    assert!(serde_json::from_value::<StreamSession>(value).is_ok());
}
