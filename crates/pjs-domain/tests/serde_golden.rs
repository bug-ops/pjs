//! Golden wire-format tests for the shared `serde_support` helpers on real types.
//!
//! The literals encode the pre-centralization encoding: `Priority` as a bare
//! `u8`, `SessionId`/`StreamId` as UUID strings, `Option<StreamId>` omitted when
//! `None`. Each test decodes the literal into the real type and requires the
//! re-encoding to be byte-identical.

use pjson_rs_domain::{DomainEvent, Frame, Stream};
use serde::{Serialize, de::DeserializeOwned};

const SESSION: &str = "11111111-1111-4111-8111-111111111111";
const STREAM: &str = "22222222-2222-4222-8222-222222222222";
const TS: &str = "2024-01-02T03:04:05Z";

fn assert_golden<T: Serialize + DeserializeOwned>(literal: &str) {
    let value: T = serde_json::from_str(literal).expect("golden literal must decode");
    assert_eq!(serde_json::to_string(&value).unwrap(), literal);
}

fn frame_literal(stream_id: &str, priority: &str) -> String {
    format!(
        r#"{{"stream_id":"{stream_id}","frame_type":"Complete","priority":{priority},"sequence":4,"timestamp":"{TS}","payload":{{}},"metadata":{{}}}}"#
    )
}

fn stream_literal(session_id: &str, stream_id: &str, rule_priority: &str) -> String {
    format!(
        r#"{{"id":"{stream_id}","session_id":"{session_id}","state":"Preparing","config":{{"max_frame_size":65536,"max_frames_per_batch":10,"enable_compression":true,"priority_rules":{{"$.a":{rule_priority}}}}},"stats":{{"total_frames":0,"skeleton_frames":0,"patch_frames":0,"complete_frames":0,"error_frames":0,"total_bytes":0,"critical_bytes":0,"high_priority_bytes":0,"average_frame_size":0.0}},"created_at":"{TS}","updated_at":"{TS}","completed_at":null,"next_sequence":1,"source_data":null,"metadata":{{}}}}"#
    )
}

#[test]
fn frame_wire_format_is_unchanged() {
    assert_golden::<Frame>(&frame_literal(STREAM, "100"));
}

#[test]
fn frame_rejects_zero_priority() {
    let literal = frame_literal(STREAM, "0");
    assert!(serde_json::from_str::<Frame>(&literal).is_err());
}

#[test]
fn frame_rejects_malformed_stream_id() {
    let literal = frame_literal("not-a-uuid", "100");
    assert!(serde_json::from_str::<Frame>(&literal).is_err());
}

#[test]
fn stream_wire_format_is_unchanged() {
    assert_golden::<Stream>(&stream_literal(SESSION, STREAM, "3"));
}

#[test]
fn stream_rejects_zero_priority_in_priority_rules() {
    let literal = stream_literal(SESSION, STREAM, "0");
    assert!(serde_json::from_str::<Stream>(&literal).is_err());
}

#[test]
fn stream_rejects_malformed_ids() {
    assert!(serde_json::from_str::<Stream>(&stream_literal("bad", STREAM, "3")).is_err());
    assert!(serde_json::from_str::<Stream>(&stream_literal(SESSION, "bad", "3")).is_err());
}

#[test]
fn event_wire_format_is_unchanged() {
    assert_golden::<DomainEvent>(&format!(
        r#"{{"event_type":"stream_created","session_id":"{SESSION}","stream_id":"{STREAM}","timestamp":"{TS}"}}"#
    ));
}

#[test]
fn backpressure_event_with_and_without_stream_id() {
    assert_golden::<DomainEvent>(&format!(
        r#"{{"event_type":"backpressure_received","session_id":"{SESSION}","stream_id":"{STREAM}","signal":"SlowDown","timestamp":"{TS}"}}"#
    ));
    assert_golden::<DomainEvent>(&format!(
        r#"{{"event_type":"backpressure_received","session_id":"{SESSION}","signal":"Pause","timestamp":"{TS}"}}"#
    ));
}

#[test]
fn event_rejects_malformed_ids() {
    let bad_session = format!(
        r#"{{"event_type":"stream_created","session_id":"bad","stream_id":"{STREAM}","timestamp":"{TS}"}}"#
    );
    assert!(serde_json::from_str::<DomainEvent>(&bad_session).is_err());

    let bad_option_stream = format!(
        r#"{{"event_type":"backpressure_received","session_id":"{SESSION}","stream_id":"bad","signal":"Ok","timestamp":"{TS}"}}"#
    );
    assert!(serde_json::from_str::<DomainEvent>(&bad_option_stream).is_err());
}
