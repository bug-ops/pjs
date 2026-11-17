//! Acceptance tests for #683 (spec 066 FR-011, revised): the offload gate
//! weighs object-root entries, container elements and integers.
//!
//! Child of `planner`, so it reaches the private weight table and `exceeds`.

use super::{PLAN_COST, SIZE_BOUND, Weights, exceeds, exceeds_plan_cost, exceeds_size};
use serde_json::{Value, json};

const GATE: usize = 32 * 1024;

/// The #669 weights: no entry, element or integer extra.
const PRE_683: Weights = Weights {
    float: 7,
    int_extra: 0,
    element: 0,
    root_entry: 0,
};

fn serialized_len(value: &Value) -> usize {
    serde_json::to_vec(value).expect("serializable").len()
}

fn key(i: usize) -> String {
    format!("k{i:04}")
}

fn object_of(n: usize, value: &Value) -> Value {
    Value::Object((0..n).map(|i| (key(i), value.clone())).collect())
}

fn array_of(n: usize, value: &Value) -> Value {
    Value::Array(vec![value.clone(); n])
}

/// The largest `n` for which an object of `i % 1000` integers stays within the gate.
fn small_ints_n() -> usize {
    let mut size = 2;
    let mut n = 0;
    loop {
        let unit = 7 + 1 + (n % 1000).to_string().len() + usize::from(n > 0);
        if size + unit > GATE {
            return n;
        }
        size += unit;
        n += 1;
    }
}

fn small_ints(n: usize) -> Value {
    Value::Object((0..n).map(|i| (key(i), json!(i % 1000))).collect())
}

/// The seven `websocket_controller` bench shapes, each sized just under the
/// pre-#683 gate cost of 32768 (spec 075 FR-008).
fn calibration_shapes() -> Vec<(&'static str, Value)> {
    vec![
        ("short_strings", object_of(2730, &json!("v"))),
        ("small_ints", small_ints(small_ints_n())),
        (
            "long_floats",
            object_of(2047, &json!(0.123_456_789_012_345)),
        ),
        ("short_floats", object_of(2047, &json!(0.5))),
        ("escape_strings", array_of(3640, &json!("\u{1}"))),
        ("nested_arrays", array_of(8191, &json!([0]))),
        ("small_objects", array_of(4095, &json!({"a": 0}))),
    ]
}

/// The mixed 64-byte-string object of the bench's `document(bytes)` group.
fn reference_document(bytes: usize) -> Value {
    let text = "x".repeat(64);
    let entries = (bytes / (64 + 16)).max(1);
    Value::Object(
        (0..entries)
            .map(|i| (format!("key{i:06}"), json!(text)))
            .collect(),
    )
}

fn float_free_documents() -> Vec<Value> {
    vec![
        json!(null),
        json!(true),
        json!(-12345),
        json!(u64::MAX),
        json!("quote\" backslash\\ tab\t newline\n"),
        Value::String("\u{1}".repeat(300)),
        json!([]),
        json!({}),
        json!([[0], [1, 2], {"a": 0}]),
        json!({"k\"ey\n": [1, "a\u{2}b", null], "\u{1}": {"\\": true}}),
        array_of(400, &json!("\u{1}")),
        object_of(200, &json!(["\u{1f}", 7])),
    ]
}

fn limits_around(actual: usize) -> Vec<usize> {
    vec![
        0,
        1,
        2,
        3,
        5,
        17,
        100,
        1000,
        actual / 2,
        actual.saturating_sub(2),
        actual.saturating_sub(1),
        actual,
        actual + 1,
    ]
}

#[test]
fn test_plan_cost_weights_are_all_positive_and_size_bound_has_none() {
    let plan = std::hint::black_box(PLAN_COST);
    let size = std::hint::black_box(SIZE_BOUND);

    assert_ne!(plan.element, 0);
    assert_ne!(plan.root_entry, 0);
    assert_ne!(plan.int_extra, 0);
    assert!(plan.float >= size.float);
    assert_eq!(size.element, 0);
    assert_eq!(size.root_entry, 0);
    assert_eq!(size.int_extra, 0);
}

#[test]
fn test_exceeds_plan_cost_scalar_string_and_empty_containers_cost_exactly_their_size() {
    let documents = [
        Value::String(String::new()),
        Value::String("x".repeat(100)),
        Value::String("x".repeat(GATE - 2)),
        Value::String("\u{1}".repeat(2000)),
        json!([]),
        json!({}),
    ];
    for document in documents {
        let size = serialized_len(&document);

        assert!(!exceeds_plan_cost(&document, size), "{document}");
        assert!(exceeds_plan_cost(&document, size - 1), "{document}");
    }
}

#[test]
fn test_exceeds_plan_cost_float_free_cost_is_at_least_the_serialized_size() {
    for document in float_free_documents() {
        let actual = serialized_len(&document);
        for limit in limits_around(actual) {
            if exceeds_size(&document, limit) {
                assert!(
                    exceeds_plan_cost(&document, limit),
                    "{document}: over {limit} by size but not by plan cost"
                );
            }
        }
    }
}

#[test]
fn test_exceeds_plan_cost_float_documents_over_the_size_bound_are_over_the_plan_cost() {
    let documents = [
        json!([0.1, -2.5e300, 1.0e-7]),
        array_of(500, &json!(0.123_456_789_012_345)),
        json!({"a": [1.5, {"b": 2.25}], "s": "\u{1}\"x"}),
        object_of(200, &json!(0.5)),
    ];
    for document in documents {
        let actual = serialized_len(&document);
        for limit in limits_around(actual) {
            if exceeds_size(&document, limit) {
                assert!(
                    exceeds_plan_cost(&document, limit),
                    "{document}: over {limit} by size bound but not by plan cost"
                );
            }
        }
    }
}

#[test]
fn test_exceeds_plan_cost_non_empty_containers_cost_more_than_their_size() {
    let documents = [
        json!(["a"]),
        json!({"k": "v"}),
        json!({"k": "v", "k2": ["a", "b\n", {"deep": "\u{1}"}]}),
        array_of(400, &json!("\u{1}")),
        object_of(200, &json!("v\"")),
        json!([[], {}]),
    ];
    for document in documents {
        let size = serialized_len(&document);

        assert!(
            exceeds_plan_cost(&document, size),
            "{document}: cost must be strictly above its size {size}"
        );
    }
}

#[test]
fn test_exceeds_plan_cost_calibration_shapes_are_inline_under_pre_683_weights() {
    for (name, shape) in calibration_shapes() {
        assert!(
            !exceeds(&shape, GATE, PRE_683),
            "{name}: bench shape must sit within the pre-#683 gate"
        );
        if !name.contains("floats") {
            assert!(serialized_len(&shape) <= GATE, "{name}");
        }
    }
}

#[test]
fn test_exceeds_plan_cost_calibration_shapes_are_over_the_gate_now() {
    for (name, shape) in calibration_shapes() {
        assert!(
            exceeds_plan_cost(&shape, GATE),
            "{name}: entry-dense shape must exceed the gate under PLAN_COST"
        );
    }
}

#[test]
fn test_exceeds_plan_cost_sixteen_kib_reference_stays_inline() {
    let reference = reference_document(16 * 1024);

    assert!(!exceeds_plan_cost(&reference, GATE));
}

#[test]
fn test_exceeds_plan_cost_thirty_two_kib_reference_plans_off_the_worker() {
    let reference = reference_document(GATE);

    assert!(exceeds_plan_cost(&reference, GATE));
}

#[test]
fn test_exceeds_size_is_unchanged_by_the_plan_cost_weights() {
    for (name, shape) in calibration_shapes() {
        if name.contains("floats") {
            continue;
        }
        let size = serialized_len(&shape);

        assert!(!exceeds_size(&shape, size), "{name}");
        assert!(exceeds_size(&shape, size - 1), "{name}");
    }
}

fn assert_flips_at(name: &str, document: &Value, total: usize) {
    assert!(
        exceeds_plan_cost(document, total - 1),
        "{name}: must exceed {}",
        total - 1
    );
    assert!(
        !exceeds_plan_cost(document, total),
        "{name}: must not exceed {total}"
    );
}

#[test]
fn test_exceeds_plan_cost_flips_at_the_weighted_total_for_a_root_object_of_strings() {
    let n = 100;
    let size = 12 * n + 1;
    let total = size + n * (PLAN_COST.root_entry + PLAN_COST.element);

    assert_eq!(serialized_len(&object_of(n, &json!("v"))), size);
    assert_flips_at("root object of strings", &object_of(n, &json!("v")), total);
}

#[test]
fn test_exceeds_plan_cost_flips_at_the_weighted_total_for_an_array_of_integers() {
    let n = 100;
    let total = 2 + (n - 1) + n * (1 + PLAN_COST.int_extra + PLAN_COST.element);

    assert_flips_at("array of ints", &array_of(n, &json!(7)), total);
}

#[test]
fn test_exceeds_plan_cost_flips_at_the_weighted_total_for_an_array_of_floats() {
    let n = 100;
    let total = 2 + (n - 1) + n * (PLAN_COST.float + PLAN_COST.element);

    assert_flips_at("array of floats", &array_of(n, &json!(0.5)), total);
}

#[test]
fn test_exceeds_plan_cost_flips_at_the_weighted_total_for_nested_arrays() {
    let n = 100;
    let total = (n + 1) + n * (3 + PLAN_COST.int_extra) + 2 * n * PLAN_COST.element;

    assert_flips_at("[[0]; n]", &array_of(n, &json!([0])), total);
}

#[test]
fn test_exceeds_plan_cost_flips_at_the_weighted_total_for_small_objects() {
    let n = 100;
    let total = (n + 1) + n * (7 + PLAN_COST.int_extra) + 2 * n * PLAN_COST.element;

    assert_flips_at("[{\"a\":0}; n]", &array_of(n, &json!({"a": 0})), total);
}

#[test]
fn test_exceeds_plan_cost_counts_the_root_entry_weight_once_for_a_nested_object() {
    let n = 100;
    let document = json!({"o": object_of(n, &json!("v"))});
    let size = 12 * n + 7;
    let total = size + PLAN_COST.root_entry + PLAN_COST.element * (1 + n);

    assert_eq!(serialized_len(&document), size);
    assert_flips_at("{\"o\": {n entries}}", &document, total);
}
