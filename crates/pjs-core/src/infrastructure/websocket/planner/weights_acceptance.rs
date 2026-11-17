//! Numeric pins of the gate weights (#669): per-number weights, the exact
//! flip point of `exceeds_plan_cost`, and the string scan boundary.
//!
//! Child of `planner`, so it reaches the private weight table.

use super::{PLAN_COST, SIZE_BOUND, Weights, exceeds_plan_cost, number_weight, string_size_within};
use serde_json::{Number, Value, json};

fn int(n: i64) -> Number {
    Number::from(n)
}

#[test]
fn test_number_weight_integers_weigh_their_digits_and_sign() {
    let zero_extra = Weights {
        float: 0,
        int_extra: 0,
        element: 0,
        root_entry: 0,
    };
    for (n, expected) in [
        (0, 1),
        (9, 1),
        (10, 2),
        (12_345, 5),
        (-1, 2),
        (-12_345, 6),
        (i64::MIN, 20),
    ] {
        assert_eq!(number_weight(&int(n), zero_extra), expected, "{n}");
    }
    assert_eq!(number_weight(&Number::from(u64::MAX), zero_extra), 20);
}

#[test]
fn test_number_weight_adds_the_integer_extra_to_signed_and_unsigned() {
    let weights = Weights {
        float: 0,
        int_extra: 3,
        element: 0,
        root_entry: 0,
    };

    assert_eq!(number_weight(&int(12_345), weights), 8);
    assert_eq!(number_weight(&int(-12_345), weights), 9);
    assert_eq!(number_weight(&Number::from(u64::MAX), weights), 23);
}

#[test]
fn test_number_weight_floats_weigh_the_table_float() {
    let float = Number::from_f64(0.5).expect("finite");

    assert_eq!(number_weight(&float, SIZE_BOUND), 3);
    assert_eq!(number_weight(&float, PLAN_COST), PLAN_COST.float);
}

#[test]
fn test_exceeds_plan_cost_flips_at_the_exact_weighted_size() {
    let floats = Value::Array(vec![json!(0.5); 100]);
    let ints = Value::Array(vec![json!(7); 100]);
    let float_total = 2 + 99 + 100 * PLAN_COST.float;
    let int_total = 2 + 99 + 100 * (1 + PLAN_COST.int_extra);

    assert!(exceeds_plan_cost(&floats, float_total - 1));
    assert!(!exceeds_plan_cost(&floats, float_total));
    assert!(exceeds_plan_cost(&ints, int_total - 1));
    assert!(!exceeds_plan_cost(&ints, int_total));
}

#[test]
fn test_string_size_within_scans_only_below_the_room_boundary() {
    let escaped = "a\"\u{1}b";
    let len = escaped.len();
    let exact = serde_json::to_vec(escaped).expect("serializable").len();

    assert_eq!(string_size_within(escaped, len + 2), None);
    assert_eq!(string_size_within(escaped, len + 1), None);
    assert_eq!(string_size_within(escaped, len + 3), Some(exact));
}
