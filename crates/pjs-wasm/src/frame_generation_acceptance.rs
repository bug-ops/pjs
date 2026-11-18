//! Acceptance tests for #656 and #698: values under keys with `.`, `[`, `]` or
//! the empty key survive WASM frame generation through quoted path segments.
//!
//! Frame generation is crate-private, so this file is compiled as a test-only
//! module of the crate (see `lib.rs`) instead of living under `tests/`.

use crate::frame_generation::build_skeleton;
use crate::security::DEFAULT_MAX_DEPTH;
use crate::{PjsParser, PriorityStream};
use pjson_rs_domain::entities::Frame;
use pjson_rs_domain::entities::frame::FrameType;
use pjson_rs_domain::value_objects::{JsonData, JsonPath, PathSegment, Priority, StreamId};
use serde_json::json;

/// Object-root documents with at least one key that `JsonPath` cannot encode.
fn unencodable_documents() -> Vec<(&'static str, serde_json::Value)> {
    vec![
        ("dotted_key", json!({"a.b": 1, "ok": 2})),
        ("empty_key", json!({"": 1, "ok": 2})),
        ("bracket_keys", json!({"k]": [1, 2], "[k": "s", "ok": 2})),
        ("nested_unencodable_key", json!({"x": {"a[0]": "v"}})),
        (
            "deeply_nested",
            json!({"x": {"y": {"p.q": true, "r": 5}}, "id": 7}),
        ),
        (
            "object_under_unencodable_key",
            json!({"a.b": {"c": [1, 2], "d.e": null}, "ok": "s"}),
        ),
    ]
}

fn data(value: serde_json::Value) -> JsonData {
    JsonData::from(value)
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

fn patch_frames(frames: &[Frame]) -> impl Iterator<Item = &Frame> {
    frames
        .iter()
        .filter(|f| matches!(f.frame_type(), FrameType::Patch))
}

fn patches_of(frame: &Frame) -> &[JsonData] {
    frame
        .payload()
        .get("patches")
        .and_then(JsonData::as_array)
        .expect("patch frame payload carries a patches array")
}

/// Applies the skeleton and then every patch frame in order, as a client does.
fn reconstruct(frames: &[Frame]) -> JsonData {
    let skeleton = frames
        .iter()
        .find(|f| matches!(f.frame_type(), FrameType::Skeleton))
        .expect("a skeleton frame is present")
        .payload();
    let mut state = serde_json::to_value(skeleton).expect("JsonData serializes");
    for frame in patch_frames(frames) {
        for patch in patches_of(frame) {
            let path: JsonPath = patch
                .get("path")
                .and_then(JsonData::as_str)
                .expect("path")
                .parse()
                .expect("emitted path parses");
            assert_eq!(
                patch.get("operation").and_then(JsonData::as_str),
                Some("set"),
                "only set patches are expected"
            );
            let value = serde_json::to_value(patch.get("value").expect("value"))
                .expect("JsonData serializes");
            let mut node = &mut state;
            for segment in path.segments() {
                node = match segment {
                    PathSegment::Key(key) => node.get_mut(key.as_str()),
                    PathSegment::Index(index) => node.get_mut(*index),
                }
                .unwrap_or_else(|| panic!("patch path {path} must be applicable"));
            }
            *node = value;
        }
    }
    JsonData::from(state)
}

#[test]
fn test_build_skeleton_has_no_verbatim_subtrees() {
    let input = data(json!({"a.b": {"c": [1, 2], "d.e": null}, "": "s", "k]": [1]}));

    let skeleton = build_skeleton(&input, 0, DEFAULT_MAX_DEPTH);

    assert_eq!(
        skeleton,
        data(json!({"a.b": {"c": [], "d.e": null}, "": null, "k]": []}))
    );
}

#[test]
fn test_build_skeleton_keeps_placeholders_for_every_key() {
    let input = data(json!({"a.b": 1, "ok": 2, "name": "n", "flag": true, "xs": [1]}));

    let skeleton = build_skeleton(&input, 0, DEFAULT_MAX_DEPTH);

    assert_eq!(
        skeleton,
        data(json!({"a.b": 0, "ok": 0, "name": null, "flag": false, "xs": []}))
    );
}

#[test]
fn test_generate_frames_reconstructs_object_root_with_unencodable_keys() {
    for (case, input) in unencodable_documents() {
        let input = data(input);

        assert_eq!(
            reconstruct(&stream_frames(&input, Priority::BACKGROUND)),
            input,
            "{case}: PriorityStream"
        );
        assert_eq!(
            reconstruct(&parser_frames(&input, Priority::BACKGROUND)),
            input,
            "{case}: PjsParser"
        );
    }
}

#[test]
fn test_generate_frames_addresses_unencodable_keys_with_quoted_segments() {
    let input = data(json!({"a.b": 1, "x": {"a[0]": "v"}, "ok": 2}));

    let frames = stream_frames(&input, Priority::BACKGROUND);

    let paths: Vec<String> = patch_frames(&frames)
        .flat_map(|f| patches_of(f).iter())
        .map(|p| {
            p.get("path")
                .and_then(JsonData::as_str)
                .expect("path")
                .to_owned()
        })
        .collect();
    assert!(paths.contains(&r#"$["a.b"]"#.to_owned()), "{paths:?}");
    assert!(paths.contains(&r#"$.x["a[0]"]"#.to_owned()), "{paths:?}");
}

/// Characterization of today's output for a document without unencodable keys:
/// the fix must not change a single frame for such documents.
#[test]
fn test_generate_frames_unchanged_for_documents_without_unencodable_keys() {
    let input = data(json!({"id": 1, "name": "Alice", "meta": {"n": 2}, "logs": []}));

    let frames = stream_frames(&input, Priority::LOW);

    assert_eq!(
        frames[0].payload(),
        &data(json!({"id": 0, "name": null, "meta": {"n": 0}, "logs": []}))
    );
    let summary: Vec<(u64, u8, String, String, JsonData)> = patch_frames(&frames)
        .flat_map(|f| {
            patches_of(f).iter().map(move |p| {
                let text = |k: &str| p.get(k).and_then(JsonData::as_str).expect(k).to_owned();
                (
                    f.sequence(),
                    f.priority().value(),
                    text("path"),
                    text("operation"),
                    p.get("value").expect("value").clone(),
                )
            })
        })
        .collect();
    let expected = vec![
        (1, 100, "$.id", "set", data(json!(1))),
        (2, 80, "$.name", "set", data(json!("Alice"))),
        (3, 70, "$.meta", "set", data(json!({"n": 2}))),
        (4, 60, "$.meta.n", "set", data(json!(2))),
    ];
    assert_eq!(
        summary,
        expected
            .into_iter()
            .map(|(s, p, path, op, v)| (s, p, path.to_owned(), op.to_owned(), v))
            .collect::<Vec<_>>()
    );
    assert_eq!(frames.len(), 6, "skeleton, four patch frames, completion");
}
