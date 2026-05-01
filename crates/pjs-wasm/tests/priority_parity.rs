//! Cross-engine priority parity tests for issue #242.
//!
//! These tests guard the invariant that the WebAssembly priority assigner
//! produces the same `Priority` as the HTTP transport (`Stream::extract_patches`)
//! for the same `(path, value)` input. Both sides delegate to
//! `pjson_rs_domain::services::compute_priority`; this test fixes the
//! delegation in place so a future regression — e.g. someone reintroducing a
//! bespoke field-name table in the WASM crate — fails CI loudly.

use pjs_wasm::priority_assignment::PriorityAssigner;
use pjson_rs_domain::services::{PriorityHeuristicConfig, compute_priority};
use pjson_rs_domain::value_objects::{JsonData, JsonPath, Priority};
use std::collections::HashMap;

/// Representative payload covering every documented divergence case from #242.
fn sample_inputs() -> Vec<(JsonPath, JsonData)> {
    vec![
        // Critical-tier fields, including `state` and `error` which used to
        // fall through in the WASM branch.
        (JsonPath::new("$.id").unwrap(), JsonData::Integer(7)),
        (
            JsonPath::new("$.state").unwrap(),
            JsonData::String("active".to_string()),
        ),
        (
            JsonPath::new("$.error").unwrap(),
            JsonData::String("oops".to_string()),
        ),
        // High-tier fields, including `description` and `message` which
        // previously fell through in WASM.
        (
            JsonPath::new("$.description").unwrap(),
            JsonData::String("hi".to_string()),
        ),
        (
            JsonPath::new("$.message").unwrap(),
            JsonData::String("hello".to_string()),
        ),
        // Medium-tier fields that the WASM engine did not recognise.
        (
            JsonPath::new("$.content").unwrap(),
            JsonData::String("x".to_string()),
        ),
        (
            JsonPath::new("$.body").unwrap(),
            JsonData::String("y".to_string()),
        ),
        // Low-tier fields beyond the WASM substring set.
        (
            JsonPath::new("$.created_at").unwrap(),
            JsonData::String("2026-05-01".to_string()),
        ),
        (JsonPath::new("$.version").unwrap(), JsonData::Integer(2)),
        // Background-tier fields shared by both engines.
        (
            JsonPath::new("$.analytics").unwrap(),
            JsonData::Array(vec![]),
        ),
        // Heuristic-fallback territory: unknown field with a small string.
        (
            JsonPath::new("$.nickname").unwrap(),
            JsonData::String("ada".to_string()),
        ),
        // Heuristic fallback with a large array — exercises the value-shape
        // penalty that WASM was getting wrong.
        (
            JsonPath::new("$.unknown").unwrap(),
            JsonData::Array((0..200).map(JsonData::Integer).collect()),
        ),
        // Heuristic fallback at depth > 5 — should lose 10 points on both sides.
        (
            JsonPath::new("$.a.b.c.d.e.f.g").unwrap(),
            JsonData::Bool(true),
        ),
        // Top-level object that previously triggered the WASM-only
        // `timestamp` branch.
        (
            JsonPath::new("$.event").unwrap(),
            JsonData::Object({
                let mut m = HashMap::new();
                m.insert("timestamp".to_string(), JsonData::Integer(1_700_000_000));
                m
            }),
        ),
    ]
}

#[test]
fn wasm_assigner_matches_domain_for_default_config() {
    let assigner = PriorityAssigner::new();
    let domain_cfg = PriorityHeuristicConfig::default();

    for (path, value) in sample_inputs() {
        let wasm_priority = assigner.calculate_field_priority(&path, &value);
        let domain_priority = compute_priority(&domain_cfg, &path, &value);
        assert_eq!(
            wasm_priority,
            domain_priority,
            "priority mismatch at {path}: WASM={wasm_priority:?}, domain={domain_priority:?}",
            path = path.as_str(),
        );
    }
}

#[test]
fn issue_242_worked_example_now_agrees() {
    // The issue's worked example: `{"description": "x", "state": "active",
    // "stats": [1,2,3]}`. Before the fix, `state` and `description` produced
    // different priorities on WASM vs HTTP. Each field is computed at depth 1.
    let assigner = PriorityAssigner::new();

    let cases: &[(&str, JsonData, Priority)] = &[
        (
            "$.description",
            JsonData::String("x".to_string()),
            Priority::HIGH,
        ),
        (
            "$.state",
            JsonData::String("active".to_string()),
            Priority::CRITICAL,
        ),
    ];

    for (path_str, value, expected) in cases {
        let path = JsonPath::new(*path_str).unwrap();
        assert_eq!(
            assigner.calculate_field_priority(&path, value),
            *expected,
            "field {path_str} should now agree with the domain layer",
        );
    }
}

#[test]
fn user_added_critical_field_takes_effect() {
    // Sanity check that the JS-friendly customisation API still flows through
    // to the domain heuristic.
    let mut assigner = PriorityAssigner::new();
    assigner
        .config_mut()
        .add_critical_field("session_id".to_string());

    let path = JsonPath::new("$.session_id").unwrap();
    let value = JsonData::String("abcd".to_string());
    assert_eq!(
        assigner.calculate_field_priority(&path, &value),
        Priority::CRITICAL,
    );
}
