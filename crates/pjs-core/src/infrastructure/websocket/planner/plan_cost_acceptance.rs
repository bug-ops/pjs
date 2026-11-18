//! Acceptance tests for #669 (revised plan D4/D5): the exact lower-bound probe
//! `exceeds_size`, the offload gate `exceeds_plan_cost`, and the plan-size
//! soundness that the pre-plan budget reject (#671) relies on.
//!
//! Child of `planner`, so it sees `exceeds_plan_cost` and `exceeds_size`
//! without widening their visibility.

use super::{Framing, PlanOptions, StreamOptions, exceeds_plan_cost, exceeds_size, plan_stream};
use crate::domain::SessionId;
use crate::infrastructure::websocket::protocol::MaxFrameSize;
use serde_json::{Value, json};

const GATE: usize = 32 * 1024;

fn serialized_len(value: &Value) -> usize {
    serde_json::to_vec(value).expect("serializable").len()
}

fn string_document(serialized_len: usize) -> Value {
    Value::String("x".repeat(serialized_len - 2))
}

/// Documents without floats: `exceeds_size` is exact for these.
fn float_free_documents() -> Vec<Value> {
    vec![
        json!(null),
        json!(true),
        json!(-12345),
        json!(u64::MAX),
        json!("quote\" backslash\\ tab\t newline\n"),
        Value::String("\u{1}".repeat(300)),
        Value::String("\"".repeat(300)),
        json!([]),
        json!({}),
        json!({"k\"ey\n": [1, "a\u{2}b", null], "\u{1}": {"\\": true}}),
        Value::Array(vec![Value::String("\u{1}".into()); 400]),
        Value::Object(
            (0..200)
                .map(|i| (format!("k\"{i}\n"), json!(["\u{1f}", i])))
                .collect(),
        ),
    ]
}

fn documents_with_floats() -> Vec<Value> {
    let mut docs = float_free_documents();
    docs.push(json!([0.1, -2.5e300, 1.0e-7]));
    docs.push(Value::Array(vec![json!(0.123_456_789_012_345); 500]));
    docs.push(json!({"a": [1.5, {"b": 2.25}], "s": "\u{1}\"x"}));
    docs
}

fn limits_around(actual: usize) -> Vec<usize> {
    let mut limits = vec![0, 1, 2, 3, 5, 17, 100, 1000, actual / 2];
    limits.extend([
        actual.saturating_sub(2),
        actual.saturating_sub(1),
        actual,
        actual + 1,
    ]);
    limits
}

#[test]
fn test_exceeds_size_is_exact_for_float_free_documents() {
    for document in float_free_documents() {
        let actual = serialized_len(&document);
        for limit in limits_around(actual) {
            assert_eq!(
                exceeds_size(&document, limit),
                actual > limit,
                "{document} serializes to {actual}, limit {limit}"
            );
        }
    }
}

#[test]
fn test_exceeds_size_counts_string_escapes() {
    let control = Value::String("\u{1}".repeat(100));
    let quotes = Value::String("\"".repeat(100));

    assert!(exceeds_size(&control, 600));
    assert!(!exceeds_size(&control, 602));
    assert!(exceeds_size(&quotes, 200));
    assert!(!exceeds_size(&quotes, 202));
}

#[test]
fn test_exceeds_size_never_overstates_with_floats() {
    for document in documents_with_floats() {
        let actual = serialized_len(&document);
        for limit in limits_around(actual) {
            if exceeds_size(&document, limit) {
                assert!(
                    actual > limit,
                    "{document}: reported over {limit}, is {actual}"
                );
            }
        }
    }
}

#[test]
fn test_exceeds_plan_cost_gates_float_dense_document_at_the_threshold() {
    let floats = Value::Array(vec![json!(0.123_456_789_012_345); 8191]);

    assert!(exceeds_plan_cost(&floats, GATE));
}

#[test]
fn test_exceeds_plan_cost_gates_escape_dense_strings_at_the_threshold() {
    let escapes = Value::Array(vec![Value::String("\u{1}".into()); 8192]);

    assert!(exceeds_plan_cost(&escapes, GATE));
}

#[test]
fn test_exceeds_plan_cost_string_boundary_matches_the_inline_threshold() {
    assert!(!exceeds_plan_cost(&string_document(GATE), GATE));
    assert!(exceeds_plan_cost(&string_document(GATE + 1), GATE));
}

#[test]
fn test_exceeds_plan_cost_is_monotonic_in_the_budget() {
    for document in documents_with_floats() {
        let mut was_over = true;
        for budget in (0..4000).step_by(11) {
            let over = exceeds_plan_cost(&document, budget);
            assert!(
                was_over || !over,
                "{document}: not over at a smaller budget but over at {budget}"
            );
            was_over = over;
        }
    }
}

#[test]
fn test_exceeds_plan_cost_never_over_for_unbounded_budget() {
    for document in documents_with_floats() {
        assert!(!exceeds_plan_cost(&document, usize::MAX), "{document}");
    }
}

#[test]
fn test_exceeds_plan_cost_is_stack_safe_for_extreme_nesting() {
    let mut value = json!(1);
    for _ in 0..200_000 {
        value = Value::Array(vec![value]);
    }

    // Dropping the value would recurse as deep as it was built, also on unwind.
    let value = std::mem::ManuallyDrop::new(value);
    assert!(exceeds_plan_cost(&value, 1000));
}

/// Soundness of the pre-plan budget reject (#671 D5): a plan never holds fewer
/// bytes than the document serializes to, in every framing and under frame
/// limits that force splitting.
#[test]
fn test_plan_stream_total_bytes_is_at_least_the_serialized_document() {
    let documents = vec![
        json!(null),
        json!(5),
        json!([]),
        json!({}),
        json!({"a": {}, "b": [], "c": [[], {}]}),
        Value::String("\u{1}".repeat(5000)),
        Value::String("x".repeat(70_000)),
        Value::Array((0..800).map(|i| json!({"id": i, "tag": "t"})).collect()),
        Value::Array(vec![json!(0.123_456_789_012_345); 1500]),
        json!({"n": {"m": {"blob": "\"".repeat(3000), "xs": [1, 2, 3]}}, "k": "v"}),
        Value::Object((0..300).map(|i| (format!("key-{i}"), json!(i))).collect()),
    ];
    for document in documents {
        let actual = serialized_len(&document);
        for framing in [Framing::Merge, Framing::Patch] {
            for max_frame_size in [1024, 2048, 4096, 64 * 1024] {
                let options = PlanOptions::from(&StreamOptions {
                    max_frame_size: MaxFrameSize::try_from(max_frame_size).expect("valid size"),
                    framing,
                    ..StreamOptions::default()
                });

                if let Ok(plan) = plan_stream(SessionId::new(), document.clone(), &options) {
                    assert!(
                        plan.total_bytes >= actual,
                        "{framing:?}/{max_frame_size}: plan {} < document {actual}",
                        plan.total_bytes
                    );
                }
            }
        }
    }
}
