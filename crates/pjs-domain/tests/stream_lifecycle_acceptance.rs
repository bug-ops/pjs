//! Acceptance tests for #753: the `Stream` lifecycle owns `completed_at` and
//! the failure reason, so they exist exactly when the state says so, the
//! reason cannot be forged through metadata, and decoding validates.

use pjson_rs_domain::Stream;
use pjson_rs_domain::entities::stream::{StreamConfig, StreamState};
use pjson_rs_domain::value_objects::{JsonData, SessionId};
use serde_json::{Value, json};

const TS: &str = "2024-01-02T03:04:05Z";

fn new_stream() -> Stream {
    Stream::new(
        SessionId::new(),
        JsonData::from(json!({"a": 1})),
        StreamConfig::default(),
    )
}

fn streaming() -> Stream {
    let mut stream = new_stream();
    stream.start_streaming().unwrap();
    stream
}

fn completed() -> Stream {
    let mut stream = streaming();
    stream.complete().unwrap();
    stream
}

fn failed() -> Stream {
    let mut stream = streaming();
    stream.fail("boom".to_owned()).unwrap();
    stream
}

fn cancelled() -> Stream {
    let mut stream = streaming();
    stream.cancel().unwrap();
    stream
}

/// Every state the transitions can reach, including failing before streaming started.
fn reachable() -> Vec<(&'static str, Stream)> {
    let mut failed_early = new_stream();
    failed_early.fail("early".to_owned()).unwrap();
    let mut cancelled_early = new_stream();
    cancelled_early.cancel().unwrap();
    vec![
        ("preparing", new_stream()),
        ("streaming", streaming()),
        ("completed", completed()),
        ("failed", failed()),
        ("cancelled", cancelled()),
        ("failed before streaming", failed_early),
        ("cancelled before streaming", cancelled_early),
    ]
}

#[test]
fn stream_completed_at_and_failure_reason_follow_the_state() {
    for (label, stream) in reachable() {
        let terminal = stream.is_finished();
        assert_eq!(stream.completed_at().is_some(), terminal, "{label}");
        assert_eq!(stream.duration().is_some(), terminal, "{label}");
        let is_failed = *stream.state() == StreamState::Failed;
        assert_eq!(stream.failure_reason().is_some(), is_failed, "{label}");
    }
}

#[test]
fn stream_fail_stores_the_reason_outside_metadata() {
    let stream = failed();
    assert_eq!(stream.failure_reason(), Some("boom"));
    assert!(stream.metadata().is_empty(), "{:?}", stream.metadata());
}

#[test]
fn stream_failure_reason_cannot_be_forged_or_overwritten_through_metadata() {
    let mut healthy = streaming();
    healthy.add_metadata("error".to_owned(), "forged".to_owned());
    assert_eq!(healthy.failure_reason(), None);

    let mut broken = failed();
    broken.add_metadata("error".to_owned(), "overwritten".to_owned());
    assert_eq!(broken.failure_reason(), Some("boom"));
}

#[test]
fn stream_serializes_failure_reason_only_when_failed() {
    let failed_json = serde_json::to_value(failed()).unwrap();
    assert_eq!(failed_json["state"], json!("Failed"));
    assert_eq!(failed_json["failure_reason"], json!("boom"));
    assert!(failed_json["completed_at"].is_string());

    for (label, stream) in reachable() {
        if *stream.state() != StreamState::Failed {
            let encoded = serde_json::to_value(&stream).unwrap();
            assert!(
                encoded.get("failure_reason").is_none(),
                "{label}: {encoded}"
            );
        }
    }
}

#[test]
fn stream_round_trips_through_json_for_every_reachable_state() {
    for (label, stream) in reachable() {
        let encoded = serde_json::to_string(&stream).unwrap();
        let decoded: Stream = serde_json::from_str(&encoded).unwrap();
        assert_eq!(serde_json::to_string(&decoded).unwrap(), encoded, "{label}");
        assert_eq!(decoded.state(), stream.state(), "{label}");
        assert_eq!(decoded.completed_at(), stream.completed_at(), "{label}");
        assert_eq!(decoded.failure_reason(), stream.failure_reason(), "{label}");
        assert_eq!(decoded.metadata(), stream.metadata(), "{label}");
    }
}

fn mutated(base: &Stream, edit: impl FnOnce(&mut serde_json::Map<String, Value>)) -> Value {
    let mut value = serde_json::to_value(base).unwrap();
    edit(value.as_object_mut().unwrap());
    value
}

fn set(key: &'static str, value: Value) -> impl FnOnce(&mut serde_json::Map<String, Value>) {
    move |map| {
        map.insert(key.to_owned(), value);
    }
}

#[test]
fn stream_deserialize_rejects_state_and_terminal_data_that_disagree() {
    let stamp = json!(TS);
    let reason = json!("boom");
    let rows: Vec<(&str, Value)> = vec![
        (
            "preparing with completed_at",
            mutated(&new_stream(), set("completed_at", stamp.clone())),
        ),
        (
            "preparing with failure_reason",
            mutated(&new_stream(), set("failure_reason", reason.clone())),
        ),
        (
            "streaming with completed_at",
            mutated(&streaming(), set("completed_at", stamp.clone())),
        ),
        (
            "streaming with failure_reason",
            mutated(&streaming(), set("failure_reason", reason.clone())),
        ),
        (
            "completed without completed_at",
            mutated(&completed(), set("completed_at", Value::Null)),
        ),
        (
            "completed with failure_reason",
            mutated(&completed(), set("failure_reason", reason.clone())),
        ),
        (
            "cancelled without completed_at",
            mutated(&cancelled(), set("completed_at", Value::Null)),
        ),
        (
            "cancelled with failure_reason",
            mutated(&cancelled(), set("failure_reason", reason.clone())),
        ),
        (
            "failed without completed_at",
            mutated(&failed(), set("completed_at", Value::Null)),
        ),
        (
            "failed without failure_reason",
            mutated(&failed(), |map| {
                map.remove("failure_reason");
            }),
        ),
        (
            "failed with neither",
            mutated(&failed(), |map| {
                map.remove("failure_reason");
                map.insert("completed_at".to_owned(), Value::Null);
            }),
        ),
    ];
    for (label, value) in rows {
        assert!(
            serde_json::from_value::<Stream>(value).is_err(),
            "must be rejected: {label}"
        );
    }
}

#[test]
fn stream_deserialize_rejects_a_zero_next_sequence() {
    let value = mutated(&new_stream(), set("next_sequence", json!(0)));
    assert!(serde_json::from_value::<Stream>(value).is_err());
}
