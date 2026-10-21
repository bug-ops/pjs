//! Wire-format guards for the shared `serde_support` helpers on pjs-core types.
//!
//! `Priority` must stay a bare `u8`, `SessionId`/`StreamId` UUID strings, and
//! `StreamSession::streams` a map keyed by the stream UUID string.

use pjson_rs::domain::aggregates::StreamSession;
use pjson_rs::domain::aggregates::stream_session::SessionConfig;
use pjson_rs::domain::value_objects::{JsonData, JsonPath, Priority};
use pjson_rs::stream::StreamFrame;
use pjson_rs::stream::priority::{JsonPatch, PatchOperation, PriorityStreamFrame};
use serde_json::{Value, json};
use std::collections::HashMap;

fn session_with_stream() -> (StreamSession, String) {
    let mut session = StreamSession::new(SessionConfig::default());
    session.activate().expect("fresh session activates");
    let stream_id = session
        .create_stream(JsonData::Null)
        .expect("stream creation within limits");
    (session, stream_id.as_uuid().to_string())
}

#[test]
fn stream_session_encodes_ids_as_uuid_strings_and_map_keys() {
    let (session, stream_uuid) = session_with_stream();
    let value = serde_json::to_value(&session).unwrap();

    assert_eq!(value["id"], session.id().as_uuid().to_string());
    let streams = value["streams"].as_object().expect("streams is a map");
    assert_eq!(streams.len(), 1);
    let stream = &streams[&stream_uuid];
    assert_eq!(stream["id"], stream_uuid);
    assert_eq!(stream["session_id"], session.id().as_uuid().to_string());
}

#[test]
fn stream_session_round_trip_is_byte_identical() {
    let (session, _) = session_with_stream();
    let encoded = serde_json::to_string(&session).unwrap();
    let decoded: StreamSession = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded.id(), session.id());
    assert_eq!(decoded.streams().len(), 1);
    let value: Value = serde_json::from_str(&encoded).unwrap();
    assert_eq!(serde_json::to_value(&decoded).unwrap(), value);
}

#[test]
fn stream_session_rejects_malformed_stream_map_key() {
    let (session, stream_uuid) = session_with_stream();
    let mut value = serde_json::to_value(&session).unwrap();
    let streams = value["streams"].as_object_mut().unwrap();
    let entry = streams.remove(&stream_uuid).unwrap();
    streams.insert("not-a-uuid".to_string(), entry);
    assert!(serde_json::from_value::<StreamSession>(value).is_err());
}

#[test]
fn stream_session_rejects_malformed_session_id() {
    let (session, _) = session_with_stream();
    let mut value = serde_json::to_value(&session).unwrap();
    value["id"] = json!("not-a-uuid");
    assert!(serde_json::from_value::<StreamSession>(value).is_err());
}

#[test]
fn stream_frame_encodes_priority_as_u8() {
    let frame = StreamFrame {
        data: json!({"k": 1}),
        priority: Priority::new(42).unwrap(),
        metadata: HashMap::new(),
    };
    assert_eq!(
        serde_json::to_string(&frame).unwrap(),
        r#"{"data":{"k":1},"priority":42,"metadata":{}}"#
    );
}

#[test]
fn json_patch_encodes_priority_as_u8() {
    let patch = JsonPatch {
        path: JsonPath::new("$.a").unwrap(),
        operation: PatchOperation::Remove,
        priority: Priority::new(9).unwrap(),
    };
    let value = serde_json::to_value(&patch).unwrap();
    assert_eq!(value["priority"], 9);
    assert_eq!(value["operation"], "Remove");
}

#[test]
fn priority_stream_frame_encodes_priority_as_u8() {
    let skeleton = PriorityStreamFrame::Skeleton {
        data: json!({}),
        priority: Priority::new(200).unwrap(),
        complete: true,
    };
    assert_eq!(
        serde_json::to_string(&skeleton).unwrap(),
        r#"{"Skeleton":{"data":{},"priority":200,"complete":true}}"#
    );

    let patch = PriorityStreamFrame::Patch {
        patches: Vec::new(),
        priority: Priority::new(5).unwrap(),
    };
    assert_eq!(
        serde_json::to_string(&patch).unwrap(),
        r#"{"Patch":{"patches":[],"priority":5}}"#
    );
}
