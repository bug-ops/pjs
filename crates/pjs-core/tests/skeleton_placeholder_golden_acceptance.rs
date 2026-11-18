//! Golden test for #758 (track A): the skeleton entry points reachable from
//! `pjson-rs` — the domain `Stream` skeleton frame and `PriorityStreamer` —
//! produce identical placeholders, including `0.0` for floats. It uses only
//! items that exist today, so it compiles before track C's #758 step; the
//! shared `services::skeleton` function is pinned in
//! `pjs-domain/tests/skeleton_service_acceptance.rs`.

use pjson_rs::stream::priority::{PriorityStreamFrame, PriorityStreamer};
use pjson_rs_domain::Stream;
use pjson_rs_domain::entities::frame::FrameBody;
use pjson_rs_domain::entities::stream::StreamConfig;
use pjson_rs_domain::value_objects::{JsonData, SessionId};
use serde_json::{Value, json};

fn golden_cases() -> Vec<(&'static str, Value, Value)> {
    vec![
        (
            "object members",
            json!({"s": "text", "i": 7, "f": 1.5, "b": true, "n": null, "a": [1, 2],
                   "o": {"s": "x", "f": 2.5, "a": [3]}}),
            json!({"s": null, "i": 0, "f": 0.0, "b": false, "n": null, "a": [],
                   "o": {"s": null, "f": 0.0, "a": []}}),
        ),
        ("root array", json!([1, {"a": 2}]), json!([])),
        ("root string", json!("text"), json!(null)),
        ("root integer", json!(7), json!(null)),
        ("root float", json!(1.5), json!(null)),
        ("root bool", json!(true), json!(null)),
        ("root null", json!(null), json!(null)),
    ]
}

fn stream_skeleton(document: &Value) -> Value {
    let mut stream = Stream::new(
        SessionId::new(),
        JsonData::from(document.clone()),
        StreamConfig::default(),
    );
    stream.start_streaming().expect("starts");
    let frame = stream.create_skeleton_frame().expect("skeleton frame");
    match frame.body() {
        FrameBody::Skeleton(data) => serde_json::to_value(data).expect("serializes"),
        other => panic!("expected a skeleton body, got {other:?}"),
    }
}

fn streamer_skeleton(document: &Value) -> Value {
    let plan = PriorityStreamer::new().analyze(document).expect("plan");
    plan.frames()
        .find_map(|frame| match frame {
            PriorityStreamFrame::Skeleton { data, .. } => Some(data.clone()),
            _ => None,
        })
        .expect("a skeleton frame")
}

#[test]
fn stream_skeleton_frame_matches_the_golden_placeholders() {
    for (name, document, expected) in golden_cases() {
        assert_eq!(stream_skeleton(&document), expected, "{name}");
    }
}

#[test]
fn priority_streamer_skeleton_matches_the_golden_placeholders() {
    for (name, document, expected) in golden_cases() {
        assert_eq!(streamer_skeleton(&document), expected, "{name}");
    }
}

#[test]
fn priority_streamer_float_placeholder_is_a_float() {
    let skeleton = streamer_skeleton(&json!({"x": 1.5, "n": 3}));

    assert!(skeleton["x"].is_f64(), "{skeleton}");
    assert!(!skeleton["n"].is_f64(), "{skeleton}");
}
