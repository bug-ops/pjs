//! Acceptance tests for #750/#755: `compute_priority` scores a borrowed
//! `ValueShape` instead of a `JsonData`, so callers holding other value types
//! (for example `serde_json::Value`) reuse the one heuristic.

use pjson_rs_domain::services::{PriorityHeuristicConfig, ValueShape, compute_priority};
use pjson_rs_domain::value_objects::{JsonData, JsonPath, Priority};
use serde_json::json;

fn path(text: &str) -> JsonPath {
    JsonPath::new(text).expect("valid path")
}

#[test]
fn value_shape_from_json_data_maps_strings_by_byte_length_and_containers_by_count() {
    assert_eq!(
        ValueShape::from(&JsonData::String("h\u{e9}".to_owned())),
        ValueShape::String { len: 3 }
    );
    assert_eq!(
        ValueShape::from(&JsonData::from(json!([1, 2, 3]))),
        ValueShape::Array { len: 3 }
    );
    assert_eq!(
        ValueShape::from(&JsonData::from(json!({"a": 1, "b": 2}))),
        ValueShape::Object { len: 2 }
    );
    for scalar in [
        JsonData::Null,
        JsonData::Bool(true),
        JsonData::Integer(7),
        JsonData::Float(1.5),
    ] {
        assert_eq!(ValueShape::from(&scalar), ValueShape::Scalar, "{scalar:?}");
    }
}

#[test]
fn compute_priority_scores_the_fallback_from_depth_and_shape() {
    let cfg = PriorityHeuristicConfig::default();
    let rows: [(&str, ValueShape, u8); 10] = [
        ("$.unknown", ValueShape::String { len: 3 }, 50 + 20 + 5),
        ("$.unknown", ValueShape::String { len: 50 }, 50 + 20),
        ("$.unknown", ValueShape::String { len: 1000 }, 50 + 20),
        ("$.unknown", ValueShape::String { len: 1001 }, 50 + 20 - 20),
        ("$.unknown", ValueShape::Array { len: 10 }, 50 + 20),
        ("$.unknown", ValueShape::Array { len: 11 }, 50 + 20 - 15),
        ("$.unknown", ValueShape::Array { len: 101 }, 50 + 20 - 40),
        ("$.unknown", ValueShape::Object { len: 11 }, 50 + 20 - 10),
        ("$.unknown", ValueShape::Scalar, 50 + 20),
        ("$.a.b.c.d.e.f.g", ValueShape::Scalar, 50 - 10),
    ];
    for (text, shape, expected) in rows {
        assert_eq!(
            compute_priority(&cfg, &path(text), shape).value(),
            expected,
            "{text} {shape:?}"
        );
    }
}

#[test]
fn compute_priority_matches_field_names_case_insensitively_whatever_the_shape() {
    let cfg = PriorityHeuristicConfig::default();
    let rows = [
        ("$.state", Priority::CRITICAL),
        ("$.error", Priority::CRITICAL),
        ("$.ID", Priority::CRITICAL),
        ("$.Name", Priority::HIGH),
        ("$.Description", Priority::HIGH),
        ("$.created_at", Priority::LOW),
        ("$.Analytics", Priority::BACKGROUND),
    ];
    for (text, expected) in rows {
        for shape in [ValueShape::Scalar, ValueShape::Array { len: 500 }] {
            assert_eq!(
                compute_priority(&cfg, &path(text), shape),
                expected,
                "{text} {shape:?}"
            );
        }
    }
}

#[test]
fn compute_priority_override_wins_and_is_case_sensitive() {
    let mut cfg = PriorityHeuristicConfig::default();
    cfg.add_override("id", Priority::LOW);
    assert_eq!(
        compute_priority(&cfg, &path("$.id"), ValueShape::Scalar),
        Priority::LOW
    );
    assert_eq!(
        compute_priority(&cfg, &path("$.ID"), ValueShape::Scalar),
        Priority::CRITICAL
    );
}
