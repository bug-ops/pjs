//! Acceptance tests for #758 (track C): one depth-limited skeleton transform,
//! `pjson_rs_domain::services::skeleton`, shared by `Stream`; and a `Stream`
//! whose skeleton and patch walks are bounded at `MAX_DESERIALIZE_DEPTH` yet
//! still rebuild a document of any depth.

use std::collections::HashMap;

use pjson_rs_domain::Stream;
use pjson_rs_domain::entities::frame::FrameBody;
use pjson_rs_domain::entities::stream::StreamConfig;
use pjson_rs_domain::services::skeleton;
use pjson_rs_domain::value_objects::{
    JsonData, MAX_DESERIALIZE_DEPTH, PathSegment, Priority, SessionId,
};
use serde_json::{Value, json};

fn data(value: Value) -> JsonData {
    JsonData::from(value)
}

fn as_value(data: &JsonData) -> Value {
    serde_json::to_value(data).expect("JsonData serializes")
}

fn skeleton_value(value: Value, max_depth: usize) -> Value {
    as_value(&skeleton(&data(value), max_depth))
}

/// `levels` single-key objects ending in `leaf`, built without recursion in the test.
fn nested(levels: usize, leaf: JsonData) -> JsonData {
    (0..levels).fold(leaf, |inner, _| {
        JsonData::Object(HashMap::from([("k".to_owned(), inner)]))
    })
}

fn nesting_depth(value: &Value) -> usize {
    match value {
        Value::Object(map) => 1 + map.values().map(nesting_depth).max().unwrap_or(0),
        Value::Array(items) => 1 + items.iter().map(nesting_depth).max().unwrap_or(0),
        _ => 0,
    }
}

#[test]
fn skeleton_replaces_object_members_with_placeholders() {
    let result = skeleton_value(
        json!({"s": "text", "i": 7, "f": 1.5, "b": true, "n": null, "a": [1, 2], "o": {"s": "x", "f": 2.5, "a": [3]}}),
        64,
    );

    assert_eq!(
        result,
        json!({"s": null, "i": 0, "f": 0.0, "b": false, "n": null, "a": [], "o": {"s": null, "f": 0.0, "a": []}})
    );
    assert!(result["f"].is_f64(), "float placeholder must stay a float");
    assert!(result["i"].is_i64() || result["i"].is_u64());
}

#[test]
fn skeleton_of_a_root_scalar_is_null_and_of_a_root_array_is_empty() {
    for scalar in [json!("s"), json!(1), json!(1.5), json!(true), json!(null)] {
        assert_eq!(skeleton_value(scalar.clone(), 64), json!(null), "{scalar}");
    }
    assert_eq!(skeleton_value(json!([1, {"a": 2}]), 64), json!([]));
}

#[test]
fn skeleton_nulls_an_object_at_the_depth_limit() {
    let source = json!({"a": {"b": {"c": {"d": 1}}}});

    assert_eq!(
        skeleton_value(source.clone(), 3),
        json!({"a": {"b": {"c": null}}})
    );
    assert_eq!(skeleton_value(source.clone(), 1), json!({"a": null}));
    assert_eq!(skeleton_value(source, 0), json!(null));
}

#[test]
fn skeleton_keeps_the_whole_shape_when_the_limit_is_not_reached() {
    let source = json!({"a": {"b": {"c": {"d": 1}}}});

    assert_eq!(
        skeleton_value(source, 64),
        json!({"a": {"b": {"c": {"d": 0}}}})
    );
}

#[test]
fn skeleton_is_bounded_on_a_document_far_deeper_than_the_limit() {
    let deep = nested(2_000, JsonData::Integer(1));

    let result = as_value(&skeleton(&deep, MAX_DESERIALIZE_DEPTH));

    assert_eq!(nesting_depth(&result), MAX_DESERIALIZE_DEPTH);
}

fn set_at(root: &mut Value, segments: &[PathSegment], value: Value) {
    let Some((first, rest)) = segments.split_first() else {
        *root = value;
        return;
    };
    let PathSegment::Key(key) = first else {
        panic!("a Stream patch walks objects only, got {first:?}");
    };
    if !root.is_object() {
        *root = json!({});
    }
    let slot = root
        .as_object_mut()
        .expect("object")
        .entry(key.clone())
        .or_insert(Value::Null);
    set_at(slot, rest, value);
}

#[test]
fn stream_frames_of_a_document_deeper_than_the_limit_are_bounded_and_rebuild_it() {
    let source = nested(
        MAX_DESERIALIZE_DEPTH + 40,
        data(json!({"id": 1, "name": "leaf", "score": 2.5, "tags": ["a", "b"]})),
    );
    let mut stream = Stream::new(SessionId::new(), source.clone(), StreamConfig::default());
    stream.start_streaming().unwrap();

    let skeleton_frame = stream.create_skeleton_frame().unwrap();
    let patch_frames = stream
        .create_patch_frames(Priority::BACKGROUND, 1_000)
        .unwrap();

    let FrameBody::Skeleton(skeleton_data) = skeleton_frame.body() else {
        panic!("expected a skeleton frame");
    };
    let mut rebuilt = as_value(skeleton_data);
    assert!(
        nesting_depth(&rebuilt) <= MAX_DESERIALIZE_DEPTH,
        "skeleton nests {} levels",
        nesting_depth(&rebuilt)
    );
    for frame in &patch_frames {
        let FrameBody::Patch(patches) = frame.body() else {
            panic!("expected a patch frame");
        };
        for patch in patches {
            assert!(
                patch.path().depth() <= MAX_DESERIALIZE_DEPTH,
                "patch path has {} segments",
                patch.path().depth()
            );
            set_at(
                &mut rebuilt,
                patch.path().segments(),
                as_value(patch.value()),
            );
        }
    }
    assert_eq!(rebuilt, as_value(&source));
}

#[test]
fn stream_skeleton_frame_matches_the_shared_skeleton() {
    let source = data(json!({"id": 1, "f": 1.5, "o": {"s": "x"}, "a": [1]}));
    let mut stream = Stream::new(SessionId::new(), source.clone(), StreamConfig::default());
    stream.start_streaming().unwrap();

    let frame = stream.create_skeleton_frame().unwrap();

    assert!(matches!(
        frame.body(),
        FrameBody::Skeleton(found) if *found == skeleton(&source, MAX_DESERIALIZE_DEPTH)
    ));
}
