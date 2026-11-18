//! Acceptance tests for #769, #760 and #759 (track E): `MaxConcurrentStreams`
//! is a validated newtype bounded by `MAX_CONCURRENT_STREAMS_PER_SESSION`,
//! `SessionConfig` has no inert `enable_compression`, and a `StreamSession`
//! hands its injected clock to the streams it creates.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use jiff::{SignedDuration, Timestamp};
use pjson_rs::domain::DomainError;
use pjson_rs::domain::aggregates::StreamSession;
use pjson_rs::domain::aggregates::stream_session::{
    MaxConcurrentStreams, SessionConfig, SessionTimeout,
};
use pjson_rs::domain::config::limits::MAX_CONCURRENT_STREAMS_PER_SESSION;
use pjson_rs::domain::entities::stream::StreamConfig;
use pjson_rs::domain::ports::TimeProvider;
use pjson_rs::domain::value_objects::JsonData;
use serde_json::{Value, json};

struct ManualTimeProvider {
    now: Mutex<Timestamp>,
}

impl ManualTimeProvider {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            now: Mutex::new(Timestamp::UNIX_EPOCH),
        })
    }

    fn advance(&self, by: SignedDuration) {
        *self.now.lock().unwrap() += by;
    }
}

impl TimeProvider for ManualTimeProvider {
    fn now(&self) -> Timestamp {
        *self.now.lock().unwrap()
    }
}

fn streams(count: usize) -> MaxConcurrentStreams {
    MaxConcurrentStreams::try_from(count).expect("in range")
}

fn config_json(streams: Value) -> Value {
    let mut value = serde_json::to_value(SessionConfig::default()).expect("config encodes");
    value["max_concurrent_streams"] = streams;
    value
}

#[test]
fn max_concurrent_streams_try_from_accepts_exactly_one_to_the_bound() {
    assert_eq!(MAX_CONCURRENT_STREAMS_PER_SESSION, 1_000);
    for accepted in [1, 10, MAX_CONCURRENT_STREAMS_PER_SESSION] {
        assert_eq!(streams(accepted).get(), accepted);
    }
    for rejected in [0, MAX_CONCURRENT_STREAMS_PER_SESSION + 1, usize::MAX] {
        let result = MaxConcurrentStreams::try_from(rejected);
        assert!(
            matches!(result, Err(DomainError::InvalidInput(_))),
            "{rejected}: {result:?}"
        );
    }
}

#[test]
fn max_concurrent_streams_default_is_ten() {
    assert_eq!(MaxConcurrentStreams::default().get(), 10);
    assert_eq!(SessionConfig::default().max_concurrent_streams.get(), 10);
}

#[test]
fn session_config_decode_validates_max_concurrent_streams() {
    for rejected in [json!(0), json!(1001), json!(u64::MAX)] {
        let result = serde_json::from_value::<SessionConfig>(config_json(rejected.clone()));
        assert!(result.is_err(), "{rejected} must fail to decode");
    }
    for accepted in [json!(1), json!(1000)] {
        let result = serde_json::from_value::<SessionConfig>(config_json(accepted.clone()));
        assert!(result.is_ok(), "{accepted}: {:?}", result.err());
    }
}

#[test]
fn session_config_wire_keeps_a_plain_number_and_drops_enable_compression() {
    let config = SessionConfig {
        max_concurrent_streams: streams(7),
        session_timeout: SessionTimeout::default(),
        default_stream_config: StreamConfig::default(),
        metadata: HashMap::new(),
    };

    let encoded = serde_json::to_value(&config).expect("encodes");

    assert_eq!(encoded["max_concurrent_streams"], json!(7));
    assert!(encoded.get("enable_compression").is_none(), "{encoded}");
}

#[test]
fn create_stream_refuses_a_stream_over_the_session_limit() {
    let config = SessionConfig {
        max_concurrent_streams: streams(2),
        ..SessionConfig::default()
    };
    let mut session = StreamSession::new(config);
    session.activate().expect("activates");
    let data = || JsonData::from(json!({"k": 1}));

    session.create_stream(data()).expect("first");
    session.create_stream(data()).expect("second");
    let third = session.create_stream(data());

    assert!(
        matches!(third, Err(DomainError::TooManyStreams(_))),
        "{third:?}"
    );
}

#[test]
fn streams_created_by_a_session_use_the_sessions_injected_clock() {
    let clock = ManualTimeProvider::new();
    let mut session = StreamSession::with_time_provider(SessionConfig::default(), clock.clone());
    session.activate().expect("activates");

    let id = session
        .create_stream(JsonData::from(json!({"k": 1})))
        .expect("creates");
    clock.advance(SignedDuration::from_secs(10));
    session.start_stream(id).expect("starts");
    clock.advance(SignedDuration::from_secs(20));
    session.complete_stream(id).expect("completes");

    let stream = session.stream(id).expect("stream");
    assert_eq!(stream.created_at(), Timestamp::UNIX_EPOCH);
    assert_eq!(
        stream.completed_at(),
        Some(Timestamp::UNIX_EPOCH + SignedDuration::from_secs(30))
    );
    assert_eq!(stream.duration(), Some(SignedDuration::from_secs(30)));
}
