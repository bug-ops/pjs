//! Acceptance tests for #752: a `Frame` carries a typed `FrameBody`, its wire
//! JSON is unchanged, and decoding validates what the constructors guarantee.

use pjson_rs_domain::Frame;
use pjson_rs_domain::entities::frame::{FrameBody, FramePatch, FrameType};
use pjson_rs_domain::value_objects::{JsonData, JsonPath, Priority, StreamId};
use serde_json::{Value, json};

const STREAM: &str = "22222222-2222-4222-8222-222222222222";
const TS: &str = "2024-01-02T03:04:05Z";

fn stream_id() -> StreamId {
    StreamId::from_string(STREAM).expect("uuid")
}

fn path(text: &str) -> JsonPath {
    JsonPath::new(text).expect("valid path")
}

fn patch_frame() -> Frame {
    Frame::patch(
        stream_id(),
        3,
        Priority::HIGH,
        vec![
            FramePatch::set(path("$.a"), JsonData::Integer(1)),
            FramePatch::set(path("$.n"), JsonData::Null),
            FramePatch::set(path(r#"$["b.c"][0]"#), JsonData::from(json!({"k": [true]}))),
        ],
    )
    .expect("patch frame")
}

fn every_constructor() -> Vec<Frame> {
    vec![
        Frame::skeleton(stream_id(), 1, JsonData::from(json!({"a": null}))),
        patch_frame(),
        Frame::complete(stream_id(), 4, None),
        Frame::complete(stream_id(), 5, Some("abc".to_owned())),
        Frame::error(stream_id(), 6, "boom".to_owned(), None),
        Frame::error(stream_id(), 7, "boom".to_owned(), Some("E1".to_owned())),
    ]
}

fn wire(frame_type: &str, priority: u8, payload: Value) -> Value {
    json!({
        "stream_id": STREAM,
        "frame_type": frame_type,
        "priority": priority,
        "sequence": 1,
        "timestamp": TS,
        "payload": payload,
        "metadata": {}
    })
}

fn decode(value: Value) -> Result<Frame, serde_json::Error> {
    serde_json::from_value(value)
}

#[test]
fn frame_body_is_the_typed_payload_of_each_constructor() {
    let skeleton = Frame::skeleton(stream_id(), 1, JsonData::from(json!({"a": null})));
    assert!(
        matches!(skeleton.body(), FrameBody::Skeleton(data) if *data == JsonData::from(json!({"a": null})))
    );
    assert_eq!(*skeleton.frame_type(), FrameType::Skeleton);
    assert_eq!(skeleton.priority(), Priority::CRITICAL);

    let patch = patch_frame();
    assert_eq!(*patch.frame_type(), FrameType::Patch);
    assert_eq!(patch.priority(), Priority::HIGH);
    let FrameBody::Patch(patches) = patch.body() else {
        panic!("expected a Patch body, got {:?}", patch.body());
    };
    assert_eq!(patches.len(), 3);
    assert_eq!(*patches[0].path(), path("$.a"));
    assert_eq!(*patches[0].value(), JsonData::Integer(1));
    assert_eq!(*patches[2].path(), path(r#"$["b.c"][0]"#));

    let complete = Frame::complete(stream_id(), 4, Some("abc".to_owned()));
    assert_eq!(*complete.frame_type(), FrameType::Complete);
    assert_eq!(complete.priority(), Priority::CRITICAL);
    assert!(matches!(complete.body(), FrameBody::Complete { checksum: Some(c) } if c == "abc"));

    let error = Frame::error(stream_id(), 6, "boom".to_owned(), Some("E1".to_owned()));
    assert_eq!(*error.frame_type(), FrameType::Error);
    assert_eq!(error.priority(), Priority::CRITICAL);
    assert!(matches!(
        error.body(),
        FrameBody::Error { message, code: Some(code) } if message == "boom" && code == "E1"
    ));
}

#[test]
fn frame_body_serializes_to_the_wire_payload() {
    let patch_payload = serde_json::to_value(patch_frame().body()).unwrap();
    assert_eq!(
        patch_payload,
        json!({"patches": [
            {"path": "$.a", "operation": "set", "value": 1},
            {"path": "$.n", "operation": "set", "value": null},
            {"path": r#"$["b.c"][0]"#, "operation": "set", "value": {"k": [true]}},
        ]})
    );
    let rows = [
        (
            Frame::skeleton(stream_id(), 1, JsonData::from(json!({"a": null}))),
            json!({"a": null}),
        ),
        (Frame::complete(stream_id(), 4, None), json!({})),
        (
            Frame::complete(stream_id(), 5, Some("abc".to_owned())),
            json!({"checksum": "abc"}),
        ),
        (
            Frame::error(stream_id(), 6, "boom".to_owned(), None),
            json!({"message": "boom"}),
        ),
        (
            Frame::error(stream_id(), 7, "boom".to_owned(), Some("E1".to_owned())),
            json!({"message": "boom", "code": "E1"}),
        ),
    ];
    for (frame, expected) in rows {
        assert_eq!(serde_json::to_value(frame.body()).unwrap(), expected);
    }
}

#[test]
fn frame_serializes_its_body_as_the_payload_field() {
    for frame in every_constructor() {
        let encoded = serde_json::to_value(&frame).unwrap();
        assert_eq!(
            encoded["payload"],
            serde_json::to_value(frame.body()).unwrap()
        );
        assert_eq!(encoded["stream_id"], json!(STREAM));
        assert_eq!(
            encoded["frame_type"],
            serde_json::to_value(frame.frame_type()).unwrap()
        );
        let fields: Vec<&str> = encoded
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(fields.len(), 7, "{fields:?}");
    }
}

#[test]
fn frame_round_trips_through_json_for_every_constructor() {
    for frame in every_constructor() {
        let encoded = serde_json::to_string(&frame).unwrap();
        let decoded: Frame = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, frame, "{encoded}");
        assert_eq!(decoded.body(), frame.body());
    }
}

#[test]
fn frame_deserialize_rejects_a_non_critical_priority_for_skeleton_complete_and_error() {
    let rows = [
        ("Skeleton", json!({"a": 1})),
        ("Complete", json!({})),
        ("Error", json!({"message": "boom"})),
    ];
    for (frame_type, payload) in rows {
        assert!(
            decode(wire(frame_type, 50, payload.clone())).is_err(),
            "{frame_type} at priority 50 must be rejected"
        );
        assert!(
            decode(wire(frame_type, 100, payload)).is_ok(),
            "{frame_type} at priority 100 must be accepted"
        );
    }
}

fn invalid_patch_payloads() -> Vec<(&'static str, Value)> {
    let ok = json!({"path": "$.a", "operation": "set", "value": 1});
    vec![
        ("null payload", json!(null)),
        ("array payload", json!([ok.clone()])),
        ("patches missing", json!({})),
        ("patches empty", json!({"patches": []})),
        ("patches not an array", json!({"patches": "x"})),
        ("element not an object", json!({"patches": [1]})),
        (
            "path missing",
            json!({"patches": [{"operation": "set", "value": 1}]}),
        ),
        (
            "path not a string",
            json!({"patches": [{"path": 7, "operation": "set", "value": 1}]}),
        ),
        (
            "path not a JsonPath",
            json!({"patches": [{"path": "users", "operation": "set", "value": 1}]}),
        ),
        (
            "operation append",
            json!({"patches": [{"path": "$.a", "operation": "append", "value": 1}]}),
        ),
        (
            "operation delete",
            json!({"patches": [{"path": "$.a", "operation": "delete", "value": null}]}),
        ),
        (
            "operation missing",
            json!({"patches": [{"path": "$.a", "value": 1}]}),
        ),
        (
            "value missing",
            json!({"patches": [{"path": "$.a", "operation": "set"}]}),
        ),
        (
            "unknown element key",
            json!({"patches": [{"path": "$.a", "operation": "set", "value": 1, "extra": 0}]}),
        ),
        ("unknown payload key", json!({"patches": [ok], "extra": 0})),
    ]
}

#[test]
fn frame_deserialize_rejects_every_invalid_patch_payload() {
    for (label, payload) in invalid_patch_payloads() {
        assert!(
            decode(wire("Patch", 80, payload)).is_err(),
            "Patch payload must be rejected: {label}"
        );
    }
}

#[test]
fn frame_deserialize_rejects_every_invalid_complete_payload() {
    let rows = [
        ("not an object", json!(null)),
        ("checksum not a string", json!({"checksum": 1})),
        ("unknown key", json!({"checksum": "a", "extra": 0})),
    ];
    for (label, payload) in rows {
        assert!(
            decode(wire("Complete", 100, payload)).is_err(),
            "Complete payload must be rejected: {label}"
        );
    }
}

#[test]
fn frame_deserialize_rejects_every_invalid_error_payload() {
    let rows = [
        ("not an object", json!("boom")),
        ("message missing", json!({"code": "E1"})),
        ("message not a string", json!({"message": 1})),
        ("code not a string", json!({"message": "boom", "code": 1})),
        ("unknown key", json!({"message": "boom", "extra": 0})),
    ];
    for (label, payload) in rows {
        assert!(
            decode(wire("Error", 100, payload)).is_err(),
            "Error payload must be rejected: {label}"
        );
    }
}
