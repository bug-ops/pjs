//! Acceptance tests for #719: `Set` patches whose last path segment is an
//! array index apply positionally, and every kind mismatch is reported with
//! the matching `PatchError`. Since #751 the op vocabulary is `WirePatch`, so
//! the former `Replace` and `Remove` rows no longer exist.

use pjson_rs::Error;
use pjson_rs::domain::value_objects::{JsonPath, PathSegment, Priority};
use pjson_rs::error::PatchError;
use pjson_rs::stream::JsonReconstructor;
use pjson_rs::stream::patch::WirePatch;
use pjson_rs::stream::priority::{JsonPatch, PriorityStreamFrame};
use serde_json::{Value, json};

fn initial() -> Value {
    json!({"a": {"k": 1}, "arr": [1, 2]})
}

fn path(segments: &[PathSegment]) -> JsonPath {
    JsonPath::from_segments(segments.iter().cloned())
}

fn key(k: &str) -> PathSegment {
    PathSegment::Key(k.to_owned())
}

fn set_nine(target: JsonPath) -> WirePatch {
    WirePatch::Set {
        path: target,
        value: json!(9),
    }
}

fn apply(wire: WirePatch) -> (Result<(), Error>, Value) {
    let mut reconstructor = JsonReconstructor::new();
    reconstructor.add_frame(PriorityStreamFrame::Skeleton {
        data: initial(),
        priority: Priority::CRITICAL,
        complete: false,
    });
    reconstructor.add_frame(PriorityStreamFrame::Patch {
        patches: vec![JsonPatch {
            patch: wire,
            priority: Priority::HIGH,
        }],
        priority: Priority::HIGH,
    });
    let result = reconstructor.process_all_frames().map(|_| ());
    (result, reconstructor.current_state().clone())
}

fn patch_error(result: Result<(), Error>) -> PatchError {
    match result {
        Err(Error::Patch(error)) => error,
        other => panic!("expected Error::Patch, got {other:?}"),
    }
}

#[test]
fn apply_patch_rejects_with_error_and_leaves_state_unchanged() {
    let idx = PathSegment::Index;
    let rows: Vec<(&str, Vec<PathSegment>, PatchError)> = vec![
        (
            "set on object with index",
            vec![key("a"), idx(0)],
            PatchError::ExpectedArray,
        ),
        (
            "set index out of range",
            vec![key("arr"), idx(2)],
            PatchError::IndexOutOfBounds { index: 2, len: 2 },
        ),
        (
            "set key on array",
            vec![key("arr"), key("x")],
            PatchError::ExpectedObject,
        ),
    ];
    for (name, segments, expected) in rows {
        let (result, state) = apply(set_nine(path(&segments)));
        assert_eq!(patch_error(result), expected, "{name}");
        assert_eq!(state, initial(), "{name}: state must be unchanged");
    }
}

#[test]
fn apply_patch_applies() {
    let idx = PathSegment::Index;
    let rows: Vec<(&str, Vec<PathSegment>, Value)> = vec![
        (
            "set index",
            vec![key("arr"), idx(1)],
            json!({"a": {"k": 1}, "arr": [1, 9]}),
        ),
        (
            "set key inserts",
            vec![key("a"), key("n")],
            json!({"a": {"k": 1, "n": 9}, "arr": [1, 2]}),
        ),
        ("set root replaces document", vec![], json!(9)),
    ];
    for (name, segments, expected) in rows {
        let (result, state) = apply(set_nine(path(&segments)));
        result.unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(state, expected, "{name}");
    }
}

#[test]
fn apply_patch_append_extends_array_and_rejects_non_array() {
    let (result, state) = apply(WirePatch::Append {
        path: path(&[key("arr")]),
        values: vec![json!(3)],
    });
    result.expect("append must apply");
    assert_eq!(state, json!({"a": {"k": 1}, "arr": [1, 2, 3]}));

    let (result, state) = apply(WirePatch::Append {
        path: path(&[key("a")]),
        values: vec![json!(3)],
    });
    assert_eq!(patch_error(result), PatchError::ExpectedArray);
    assert_eq!(state, initial());
}
