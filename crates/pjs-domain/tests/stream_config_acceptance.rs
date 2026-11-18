//! Acceptance tests for #760 (track C): `StreamConfig` keeps only the field that
//! changes behavior, `priority_rules`.

use std::collections::HashMap;

use pjson_rs_domain::Stream;
use pjson_rs_domain::entities::stream::StreamConfig;
use pjson_rs_domain::value_objects::{JsonData, Priority, SessionId};
use serde_json::json;

#[test]
fn stream_config_is_built_from_priority_rules_alone() {
    // An exhaustive literal: compiles only while `priority_rules` is the single field.
    let config = StreamConfig {
        priority_rules: HashMap::from([("note".to_owned(), Priority::CRITICAL)]),
    };

    assert_eq!(config.priority_rules.len(), 1);
    assert!(StreamConfig::default().priority_rules.is_empty());
}

#[test]
fn stream_config_serializes_only_priority_rules() {
    let encoded = serde_json::to_value(StreamConfig::default()).unwrap();

    let keys: Vec<&String> = encoded.as_object().expect("object").keys().collect();
    assert_eq!(keys, ["priority_rules"]);
}

#[test]
fn priority_rules_override_the_heuristic_for_a_field() {
    let config = StreamConfig {
        priority_rules: HashMap::from([("note".to_owned(), Priority::CRITICAL)]),
    };
    let mut stream = Stream::new(
        SessionId::new(),
        JsonData::from(json!({"note": "x", "analytics": 1})),
        config,
    );
    stream.start_streaming().unwrap();

    let patches = stream
        .extract_prioritized_patches(Priority::BACKGROUND)
        .unwrap();

    let priority_of = |key: &str| {
        patches
            .iter()
            .find(|(patch, _)| patch.path().last_key() == Some(key))
            .map(|(_, priority)| *priority)
    };
    assert_eq!(priority_of("note"), Some(Priority::CRITICAL));
    assert_eq!(priority_of("analytics"), Some(Priority::BACKGROUND));
}
