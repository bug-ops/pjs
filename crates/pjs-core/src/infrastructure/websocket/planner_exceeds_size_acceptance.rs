//! Acceptance tests for #658: `planner::exceeds_size`, the cheap early-exit
//! size probe that decides whether `create_session` plans off-thread.
//!
//! `exceeds_size` is crate-private, so this file is a test-only child of the
//! `websocket` module (wired in `mod.rs`). It needs
//! `pub(super) fn exceeds_size(value: &Value, limit: usize) -> bool` in
//! `planner.rs`.

use super::planner::exceeds_size;
use serde_json::{Value, json};

fn sample_documents() -> Vec<Value> {
    vec![
        json!(null),
        json!(true),
        json!(12345),
        json!(-1.5),
        json!("abcdef"),
        json!([]),
        json!({}),
        json!([1, 2, 3, "four", null, {"k": [true, false]}]),
        json!({"user": {"name": "Alice", "tags": ["a", "b", "c"]}, "n": 42, "blob": "x".repeat(300)}),
        Value::Array(
            (0..500)
                .map(|i| json!({"id": i, "label": format!("item-{i}")}))
                .collect(),
        ),
    ]
}

fn serialized_len(value: &Value) -> usize {
    serde_json::to_vec(value).expect("serializable").len()
}

#[test]
fn test_exceeds_size_is_a_lower_bound_of_the_serialized_size() {
    for document in sample_documents() {
        let actual = serialized_len(&document);
        for limit in [
            0,
            1,
            2,
            5,
            17,
            100,
            1000,
            actual.saturating_sub(1),
            actual,
            actual + 1,
        ] {
            if exceeds_size(&document, limit) {
                assert!(
                    actual > limit,
                    "reported over limit {limit} but {document} serializes to {actual} bytes"
                );
            }
        }
    }
}

#[test]
fn test_exceeds_size_is_monotonic_in_the_limit() {
    for document in sample_documents() {
        let mut was_over = true;
        for limit in (0..2000).step_by(7) {
            let over = exceeds_size(&document, limit);
            assert!(
                was_over || !over,
                "{document}: not over at a smaller limit but over at {limit}"
            );
            was_over = over;
        }
    }
}

#[test]
fn test_exceeds_size_string_boundary_counts_quotes() {
    let value = json!("abcd");

    assert!(exceeds_size(&value, 5));
    assert!(!exceeds_size(&value, 6));
}

#[test]
fn test_exceeds_size_empty_containers_count_their_brackets() {
    for value in [json!({}), json!([])] {
        assert!(exceeds_size(&value, 1), "{value}");
        assert!(!exceeds_size(&value, 2), "{value}");
    }
}

#[test]
fn test_exceeds_size_never_over_for_unbounded_limit() {
    for document in sample_documents() {
        assert!(!exceeds_size(&document, usize::MAX), "{document}");
    }
}

#[test]
fn test_exceeds_size_detects_large_documents_of_every_shape() {
    let big_string = Value::String("x".repeat(1024 * 1024));
    let many_numbers = Value::Array((0..100_000).map(Value::from).collect());
    let many_nulls = Value::Array(vec![Value::Null; 5000]);
    let many_keys = Value::Object((0..5000).map(|i| (format!("k{i}"), Value::Null)).collect());

    for value in [big_string, many_numbers, many_nulls, many_keys] {
        assert!(exceeds_size(&value, 1000));
    }
}

#[test]
fn test_exceeds_size_small_document_is_not_over_a_generous_limit() {
    let value = json!({"a": [1, 2, 3], "b": "text"});

    assert!(!exceeds_size(&value, 64 * 1024));
}

#[test]
fn test_exceeds_size_is_stack_safe_for_extreme_nesting() {
    let mut value = json!(1);
    for _ in 0..200_000 {
        value = Value::Array(vec![value]);
    }

    let over = exceeds_size(&value, 1000);

    // Dropping the value would recurse as deep as it was built.
    std::mem::forget(value);
    assert!(over, "200000 nested arrays are at least 400000 bytes");
}
