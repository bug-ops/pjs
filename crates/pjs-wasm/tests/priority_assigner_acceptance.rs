//! Acceptance tests for #755: `PriorityAssigner` is fixed by the heuristic
//! config passed to `with_config` and scores exactly like `compute_priority`.

use pjs_wasm::priority_assignment::PriorityAssigner;
use pjson_rs_domain::services::{PriorityHeuristicConfig, ValueShape, compute_priority};
use pjson_rs_domain::value_objects::{JsonData, JsonPath, Priority};
use serde_json::json;

fn custom_config() -> PriorityHeuristicConfig {
    let mut config = PriorityHeuristicConfig::default();
    config.critical_fields.push("session_id".to_owned());
    config.high_fields.push("display_name".to_owned());
    config.low_fields.push("debug_info".to_owned());
    config.background_fields.push("telemetry".to_owned());
    config.large_array_threshold = 3;
    config.large_string_threshold = 4;
    config
}

fn inputs() -> Vec<(JsonPath, JsonData)> {
    let path = |text: &str| JsonPath::new(text).unwrap();
    vec![
        (path("$.session_id"), JsonData::Integer(1)),
        (path("$.Display_Name"), JsonData::String("ada".to_owned())),
        (path("$.debug_info"), JsonData::Bool(true)),
        (path("$.telemetry"), JsonData::Null),
        (path("$.id"), JsonData::Integer(7)),
        (path("$.unknown"), JsonData::from(json!([1, 2, 3, 4]))),
        (path("$.unknown"), JsonData::String("longer".to_owned())),
        (path("$.a.b.c.d.e.f.g"), JsonData::from(json!({"k": 1}))),
    ]
}

#[test]
fn calculate_field_priority_equals_compute_priority_over_the_projected_config() {
    let heuristic = custom_config();
    let assigner = PriorityAssigner::with_config(custom_config());

    for (path, value) in inputs() {
        assert_eq!(
            assigner.calculate_field_priority(&path, &value),
            compute_priority(&heuristic, &path, ValueShape::from(&value)),
            "{path}"
        );
    }
}

#[test]
fn extract_prioritized_fields_with_limit_prices_each_field_like_compute_priority() {
    let heuristic = custom_config();
    let assigner = PriorityAssigner::with_config(custom_config());
    let data = JsonData::from(json!({
        "session_id": 1,
        "nested": {"telemetry": [1, 2, 3, 4], "name": "x"}
    }));

    let fields = assigner.extract_prioritized_fields_with_limit(&data, 8);

    assert_eq!(fields.len(), 4, "{fields:?}");
    for field in &fields {
        assert_eq!(
            field.priority,
            compute_priority(&heuristic, &field.path, ValueShape::from(&field.value)),
            "{}",
            field.path
        );
    }
    let critical = fields
        .iter()
        .find(|f| f.path.to_string() == "$.session_id")
        .unwrap();
    assert_eq!(critical.priority, Priority::CRITICAL);
}
