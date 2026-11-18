//! Spec 070 delta (2026-11-16): a non-object root ships whole in its root `set`
//! patch whatever `max_depth` is; object roots stay truncated at `max_depth`.

use crate::frame_generation::{generate_frames, payload_data};
use crate::priority_assignment::PriorityAssigner;
use pjson_rs_domain::entities::Frame;
use pjson_rs_domain::entities::frame::FrameType;
use pjson_rs_domain::value_objects::{JsonData, Priority, StreamId};
use serde_json::json;

fn frames(input: &JsonData, max_depth: usize) -> Vec<Frame> {
    generate_frames(
        &PriorityAssigner::new(),
        input,
        StreamId::new(),
        Priority::BACKGROUND,
        max_depth,
    )
    .expect("frame generation succeeds")
}

fn root_patch_value(frames: &[Frame]) -> JsonData {
    let patch_frame = frames
        .iter()
        .find(|f| matches!(f.frame_type(), FrameType::Patch))
        .expect("a patch frame is present");
    let payload = payload_data(patch_frame);
    let patches = payload
        .get("patches")
        .and_then(JsonData::as_array)
        .expect("patches array");
    patches[0].get("value").expect("value").clone()
}

#[test]
fn test_generate_frames_ships_a_deep_array_root_whole_beyond_max_depth() {
    let input = JsonData::from(json!([[[[[1, 2, 3]]]]]));

    for max_depth in [1, 2, 3] {
        assert_eq!(
            root_patch_value(&frames(&input, max_depth)),
            input,
            "max_depth {max_depth}"
        );
    }
}

#[test]
fn test_generate_frames_truncates_an_object_root_at_max_depth() {
    let input = JsonData::from(json!({"a": {"b": {"c": 1}}}));

    let frames = frames(&input, 2);

    assert_eq!(
        payload_data(&frames[0]),
        JsonData::from(json!({"a": {"b": null}}))
    );
}
