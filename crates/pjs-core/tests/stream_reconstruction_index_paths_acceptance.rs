//! Acceptance tests for #719: `Set`, `Replace` and `Remove` patches whose last
//! path segment is an array index apply positionally, and every kind mismatch
//! is reported with the matching `PatchError`.
//!
//! Remove asymmetry (deliberate): removing a missing object key is a no-op
//! (the document is already in the target state), while removing an
//! out-of-range array index is an error (the sender's view of the array has
//! diverged).

use pjson_rs::Error;
use pjson_rs::domain::value_objects::{JsonPath, PathSegment, Priority};
use pjson_rs::error::PatchError;
use pjson_rs::stream::JsonReconstructor;
use pjson_rs::stream::priority::{JsonPatch, PatchOperation, PriorityStreamFrame};
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

#[derive(Clone, Copy)]
enum Op {
    Set,
    Replace,
    Remove,
}

fn operation(op: Op) -> PatchOperation {
    match op {
        Op::Set => PatchOperation::Set { value: json!(9) },
        Op::Replace => PatchOperation::Replace { value: json!(9) },
        Op::Remove => PatchOperation::Remove,
    }
}

fn apply(op: PatchOperation, target: JsonPath) -> (Result<(), Error>, Value) {
    let mut reconstructor = JsonReconstructor::new();
    reconstructor.add_frame(PriorityStreamFrame::Skeleton {
        data: initial(),
        priority: Priority::CRITICAL,
        complete: false,
    });
    reconstructor.add_frame(PriorityStreamFrame::Patch {
        patches: vec![JsonPatch {
            path: target,
            operation: op,
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
    let rows: Vec<(&str, Op, Vec<PathSegment>, PatchError)> = vec![
        (
            "set on object with index",
            Op::Set,
            vec![key("a"), idx(0)],
            PatchError::ExpectedArray,
        ),
        (
            "replace on object with index",
            Op::Replace,
            vec![key("a"), idx(0)],
            PatchError::ExpectedArray,
        ),
        (
            "remove on object with index",
            Op::Remove,
            vec![key("a"), idx(0)],
            PatchError::ExpectedArray,
        ),
        (
            "set index out of range",
            Op::Set,
            vec![key("arr"), idx(2)],
            PatchError::IndexOutOfBounds { index: 2, len: 2 },
        ),
        (
            "remove index out of range",
            Op::Remove,
            vec![key("arr"), idx(2)],
            PatchError::IndexOutOfBounds { index: 2, len: 2 },
        ),
        (
            "set key on array",
            Op::Set,
            vec![key("arr"), key("x")],
            PatchError::ExpectedObject,
        ),
        (
            "remove key on array",
            Op::Remove,
            vec![key("arr"), key("x")],
            PatchError::ExpectedObject,
        ),
        ("remove root", Op::Remove, vec![], PatchError::NoTargetKey),
    ];
    for (name, op, segments, expected) in rows {
        let (result, state) = apply(operation(op), path(&segments));
        assert_eq!(patch_error(result), expected, "{name}");
        assert_eq!(state, initial(), "{name}: state must be unchanged");
    }
}

#[test]
fn apply_patch_applies() {
    let idx = PathSegment::Index;
    let rows: Vec<(&str, Op, Vec<PathSegment>, Value)> = vec![
        (
            "set index",
            Op::Set,
            vec![key("arr"), idx(1)],
            json!({"a": {"k": 1}, "arr": [1, 9]}),
        ),
        (
            "replace index",
            Op::Replace,
            vec![key("arr"), idx(1)],
            json!({"a": {"k": 1}, "arr": [1, 9]}),
        ),
        (
            "remove index shifts later elements",
            Op::Remove,
            vec![key("arr"), idx(0)],
            json!({"a": {"k": 1}, "arr": [2]}),
        ),
        (
            "set key inserts",
            Op::Set,
            vec![key("a"), key("n")],
            json!({"a": {"k": 1, "n": 9}, "arr": [1, 2]}),
        ),
        ("set root replaces document", Op::Set, vec![], json!(9)),
        // Q1 asymmetry: a missing key is already in the target state.
        (
            "remove missing key is a no-op",
            Op::Remove,
            vec![key("a"), key("missing")],
            initial(),
        ),
    ];
    for (name, op, segments, expected) in rows {
        let (result, state) = apply(operation(op), path(&segments));
        result.unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(state, expected, "{name}");
    }
}

#[test]
fn apply_patch_append_extends_array_and_rejects_non_array() {
    let (result, state) = apply(
        PatchOperation::Append {
            values: vec![json!(3)],
        },
        path(&[key("arr")]),
    );
    result.expect("append must apply");
    assert_eq!(state, json!({"a": {"k": 1}, "arr": [1, 2, 3]}));

    let (result, state) = apply(
        PatchOperation::Append {
            values: vec![json!(3)],
        },
        path(&[key("a")]),
    );
    assert_eq!(patch_error(result), PatchError::ExpectedArray);
    assert_eq!(state, initial());
}
