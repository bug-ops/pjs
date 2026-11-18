//! Acceptance tests for #667: a non-object root (array or scalar) must survive
//! WASM frame generation through a single CRITICAL root `set` patch.
//!
//! Frame generation is crate-private, so this file is a test-only module of the
//! crate (see `lib.rs`), like `frame_generation_acceptance.rs`.

use crate::frame_generation::payload_data;
use crate::{PjsParser, PriorityStream};
use pjson_rs_domain::entities::Frame;
use pjson_rs_domain::entities::frame::FrameType;
use pjson_rs_domain::value_objects::{JsonData, Priority, StreamId};
use serde_json::json;

fn non_object_roots() -> Vec<(&'static str, serde_json::Value)> {
    vec![
        ("flat_array", json!([1, 2])),
        ("array_of_objects", json!([{"a": 1}])),
        ("nested_arrays", json!([[1], [2]])),
        ("empty_array", json!([])),
        ("scalar_int", json!(5)),
        ("scalar_string", json!("s")),
        ("scalar_null", json!(null)),
    ]
}

fn min_priorities() -> Vec<Priority> {
    vec![
        Priority::BACKGROUND,
        Priority::LOW,
        Priority::CRITICAL,
        Priority::new(255).expect("255 is a valid priority"),
    ]
}

fn stream_frames(input: &JsonData, min: Priority) -> Vec<Frame> {
    PriorityStream::new()
        .generate_frames_internal(input, StreamId::new(), min)
        .expect("PriorityStream frame generation must succeed")
}

fn parser_frames(input: &JsonData, min: Priority) -> Vec<Frame> {
    PjsParser::new()
        .generate_frames_internal(input, StreamId::new(), min)
        .expect("PjsParser frame generation must succeed")
}

fn patch_frames(frames: &[Frame]) -> Vec<&Frame> {
    frames
        .iter()
        .filter(|f| matches!(f.frame_type(), FrameType::Patch))
        .collect()
}

fn patches_of(frame: &Frame) -> Vec<JsonData> {
    payload_data(frame)
        .get("patches")
        .and_then(JsonData::as_array)
        .expect("patch frame payload carries a patches array")
        .to_vec()
}

/// Applies skeleton then every patch; only root `$` `set` patches are valid for a non-object root.
fn reconstruct_root_only(frames: &[Frame]) -> JsonData {
    let mut state = frames
        .iter()
        .find(|f| matches!(f.frame_type(), FrameType::Skeleton))
        .map(payload_data)
        .expect("a skeleton frame is present");
    for frame in patch_frames(frames) {
        for patch in patches_of(frame) {
            let path = patch.get("path").and_then(JsonData::as_str).expect("path");
            assert_eq!(
                path, "$",
                "non-object roots emit no indexed or keyed patches"
            );
            assert_eq!(
                patch.get("operation").and_then(JsonData::as_str),
                Some("set")
            );
            state = patch.get("value").expect("value").clone();
        }
    }
    state
}

#[test]
fn test_generate_frames_reconstructs_non_object_root_at_background() {
    for (case, input) in non_object_roots() {
        let input = JsonData::from(input);

        assert_eq!(
            reconstruct_root_only(&stream_frames(&input, Priority::BACKGROUND)),
            input,
            "{case}: PriorityStream"
        );
        assert_eq!(
            reconstruct_root_only(&parser_frames(&input, Priority::BACKGROUND)),
            input,
            "{case}: PjsParser"
        );
    }
}

#[test]
fn test_generate_frames_non_object_root_survives_any_min_priority() {
    for (case, input) in non_object_roots() {
        let input = JsonData::from(input);
        for min in min_priorities() {
            assert_eq!(
                reconstruct_root_only(&stream_frames(&input, min)),
                input,
                "{case}: PriorityStream min={}",
                min.value()
            );
            assert_eq!(
                reconstruct_root_only(&parser_frames(&input, min)),
                input,
                "{case}: PjsParser min={}",
                min.value()
            );
        }
    }
}

#[test]
fn test_generate_frames_non_object_root_emits_one_critical_root_set_frame() {
    for (case, input) in non_object_roots() {
        let input = JsonData::from(input);
        for min in min_priorities() {
            let frames = stream_frames(&input, min);

            assert_eq!(frames.len(), 3, "{case}: skeleton, root patch, completion");
            assert!(matches!(frames[0].frame_type(), FrameType::Skeleton));
            assert!(matches!(frames[1].frame_type(), FrameType::Patch));
            assert!(matches!(frames[2].frame_type(), FrameType::Complete));
            assert_eq!(frames[1].priority(), Priority::CRITICAL, "{case}");
            let patches = patches_of(&frames[1]);
            assert_eq!(patches.len(), 1, "{case}: exactly one root patch");
            assert_eq!(
                patches[0].get("path").and_then(JsonData::as_str),
                Some("$"),
                "{case}"
            );
            assert_eq!(
                patches[0].get("operation").and_then(JsonData::as_str),
                Some("set"),
                "{case}"
            );
            assert_eq!(patches[0].get("value"), Some(&input), "{case}");
        }
    }
}

#[test]
fn test_generate_frames_non_object_root_keeps_skeleton_shape() {
    let cases = [
        (json!([1, 2]), json!([])),
        (json!(5), json!(null)),
        (json!("s"), json!(null)),
    ];
    for (input, expected) in cases {
        let frames = stream_frames(&JsonData::from(input), Priority::BACKGROUND);

        assert_eq!(payload_data(&frames[0]), JsonData::from(expected));
    }
}

#[test]
fn test_generate_frames_parser_and_stream_agree_on_non_object_root() {
    for (case, input) in non_object_roots() {
        let input = JsonData::from(input);

        let a = stream_frames(&input, Priority::BACKGROUND);
        let b = parser_frames(&input, Priority::BACKGROUND);

        assert_eq!(a.len(), b.len(), "{case}");
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!(payload_data(x), payload_data(y), "{case}");
            assert_eq!(x.priority(), y.priority(), "{case}");
        }
    }
}
