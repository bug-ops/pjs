//! Comprehensive tests for pjs-wasm/src/streaming.rs and security.rs
//!
//! This test suite aims to achieve 60%+ coverage by testing:
//! - SecurityConfig configuration and validation
//! - JsonData conversions and operations
//! - Error handling paths
//! - Edge cases and boundary conditions
//!
//! Note: Core PriorityStream functionality is tested through WASM-bindgen-test in the source file.
//! These tests focus on supporting infrastructure that can be tested in standard Rust.

use pjs_domain::value_objects::{JsonData, Priority};
use pjs_wasm::security::{SecurityConfig, validate_input_size};
use std::collections::HashMap;

// === SecurityConfig Tests ===

#[test]
fn test_security_config_new() {
    let config = SecurityConfig::new();
    assert_eq!(config.max_json_size(), 10 * 1024 * 1024); // 10 MB default
    assert_eq!(config.max_depth(), 64); // Default depth
}

#[test]
fn test_security_config_default() {
    let config = SecurityConfig::default();
    assert_eq!(config.max_json_size(), 10 * 1024 * 1024);
    assert_eq!(config.max_depth(), 64);
}

#[test]
fn test_security_config_set_max_json_size() {
    let config = SecurityConfig::new().set_max_json_size(5 * 1024 * 1024);
    assert_eq!(config.max_json_size(), 5 * 1024 * 1024);
}

#[test]
fn test_security_config_set_max_depth() {
    let config = SecurityConfig::new().set_max_depth(64);
    assert_eq!(config.max_depth(), 64);
}

#[test]
fn test_security_config_set_minimum_depth() {
    let config = SecurityConfig::new().set_max_depth(1);
    assert_eq!(config.max_depth(), 1);
}

#[test]
fn test_security_config_set_large_depth() {
    let config = SecurityConfig::new().set_max_depth(1000);
    assert_eq!(config.max_depth(), 1000);
}

#[test]
fn test_security_config_set_large_json_size() {
    let config = SecurityConfig::new().set_max_json_size(100 * 1024 * 1024); // 100 MB
    assert_eq!(config.max_json_size(), 100 * 1024 * 1024);
}

#[test]
fn test_security_config_set_small_json_size() {
    let config = SecurityConfig::new().set_max_json_size(1024); // 1 KB
    assert_eq!(config.max_json_size(), 1024);
}

#[test]
fn test_security_config_multiple_updates() {
    let config = SecurityConfig::new()
        .set_max_json_size(1024)
        .set_max_depth(10);

    assert_eq!(config.max_json_size(), 1024);
    assert_eq!(config.max_depth(), 10);

    let config2 = config.set_max_json_size(2048).set_max_depth(20);

    assert_eq!(config2.max_json_size(), 2048);
    assert_eq!(config2.max_depth(), 20);
}

// === Input Validation Tests ===

#[test]
fn test_validate_input_size_within_limit() {
    let config = SecurityConfig::new();
    let input = "{}";

    let result = validate_input_size(input, &config);
    assert!(result.is_ok());
}

#[test]
fn test_validate_input_size_empty_string() {
    let config = SecurityConfig::new();
    let input = "";

    let result = validate_input_size(input, &config);
    assert!(result.is_ok());
}

#[test]
fn test_validate_input_size_at_limit() {
    let config = SecurityConfig::new().set_max_json_size(100);

    let input = "x".repeat(100);

    let result = validate_input_size(&input, &config);
    assert!(result.is_ok());
}

#[test]
fn test_validate_input_size_exceeds_limit() {
    let config = SecurityConfig::new().set_max_json_size(100);

    let input = "x".repeat(101);

    let result = validate_input_size(&input, &config);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("exceeds"));
}

#[test]
fn test_validate_input_size_way_over_limit() {
    let config = SecurityConfig::new().set_max_json_size(1024);

    let input = "x".repeat(10000);

    let result = validate_input_size(&input, &config);
    assert!(result.is_err());
}

#[test]
fn test_validate_input_size_unicode() {
    let config = SecurityConfig::new();
    let input = "こんにちは世界"; // Japanese characters

    let result = validate_input_size(input, &config);
    assert!(result.is_ok());
}

#[test]
fn test_validate_input_size_with_json_structure() {
    let config = SecurityConfig::new();
    let input = r#"{"key": "value", "number": 42, "array": [1, 2, 3]}"#;

    let result = validate_input_size(input, &config);
    assert!(result.is_ok());
}

// === JsonData Helper Tests ===

#[test]
fn test_json_data_null() {
    let data = JsonData::Null;
    assert!(matches!(data, JsonData::Null));
}

#[test]
fn test_json_data_bool_true() {
    let data = JsonData::Bool(true);
    match data {
        JsonData::Bool(v) => assert!(v),
        _ => panic!("Expected bool"),
    }
}

#[test]
fn test_json_data_bool_false() {
    let data = JsonData::Bool(false);
    match data {
        JsonData::Bool(v) => assert!(!v),
        _ => panic!("Expected bool"),
    }
}

#[test]
fn test_json_data_integer() {
    let data = JsonData::Integer(42);
    match data {
        JsonData::Integer(v) => assert_eq!(v, 42),
        _ => panic!("Expected integer"),
    }
}

#[test]
fn test_json_data_integer_negative() {
    let data = JsonData::Integer(-123);
    match data {
        JsonData::Integer(v) => assert_eq!(v, -123),
        _ => panic!("Expected integer"),
    }
}

#[test]
fn test_json_data_integer_zero() {
    let data = JsonData::Integer(0);
    match data {
        JsonData::Integer(v) => assert_eq!(v, 0),
        _ => panic!("Expected integer"),
    }
}

#[test]
fn test_json_data_float() {
    let data = JsonData::Float(3.5);
    match data {
        JsonData::Float(v) => assert!((v - 3.5).abs() < 0.001),
        _ => panic!("Expected float"),
    }
}

#[test]
fn test_json_data_float_negative() {
    let data = JsonData::Float(-2.5);
    match data {
        JsonData::Float(v) => assert!((v + 2.5).abs() < 0.001),
        _ => panic!("Expected float"),
    }
}

#[test]
fn test_json_data_float_zero() {
    let data = JsonData::Float(0.0);
    match data {
        JsonData::Float(v) => assert_eq!(v, 0.0),
        _ => panic!("Expected float"),
    }
}

#[test]
fn test_json_data_string() {
    let data = JsonData::String("hello".to_string());
    match data {
        JsonData::String(s) => assert_eq!(s, "hello"),
        _ => panic!("Expected string"),
    }
}

#[test]
fn test_json_data_string_empty() {
    let data = JsonData::String("".to_string());
    match data {
        JsonData::String(s) => assert_eq!(s, ""),
        _ => panic!("Expected string"),
    }
}

#[test]
fn test_json_data_string_unicode() {
    let data = JsonData::String("こんにちは".to_string());
    match data {
        JsonData::String(s) => assert_eq!(s, "こんにちは"),
        _ => panic!("Expected string"),
    }
}

#[test]
fn test_json_data_array_empty() {
    let data = JsonData::Array(vec![]);
    match data {
        JsonData::Array(arr) => assert!(arr.is_empty()),
        _ => panic!("Expected array"),
    }
}

#[test]
fn test_json_data_array_with_elements() {
    let data = JsonData::Array(vec![
        JsonData::Integer(1),
        JsonData::Integer(2),
        JsonData::Integer(3),
    ]);
    match data {
        JsonData::Array(arr) => assert_eq!(arr.len(), 3),
        _ => panic!("Expected array"),
    }
}

#[test]
fn test_json_data_array_mixed_types() {
    let data = JsonData::Array(vec![
        JsonData::Integer(1),
        JsonData::String("test".to_string()),
        JsonData::Bool(true),
        JsonData::Null,
    ]);
    match data {
        JsonData::Array(arr) => assert_eq!(arr.len(), 4),
        _ => panic!("Expected array"),
    }
}

#[test]
fn test_json_data_object_empty() {
    let data = JsonData::Object(HashMap::new());
    match data {
        JsonData::Object(map) => assert!(map.is_empty()),
        _ => panic!("Expected object"),
    }
}

#[test]
fn test_json_data_object_with_fields() {
    let mut map = HashMap::new();
    map.insert("key".to_string(), JsonData::String("value".to_string()));
    let data = JsonData::Object(map);

    match data {
        JsonData::Object(m) => {
            assert_eq!(m.len(), 1);
            assert!(m.contains_key("key"));
        }
        _ => panic!("Expected object"),
    }
}

#[test]
fn test_json_data_object_multiple_fields() {
    let mut map = HashMap::new();
    map.insert("string".to_string(), JsonData::String("value".to_string()));
    map.insert("number".to_string(), JsonData::Integer(42));
    map.insert("bool".to_string(), JsonData::Bool(true));
    let data = JsonData::Object(map);

    match data {
        JsonData::Object(m) => assert_eq!(m.len(), 3),
        _ => panic!("Expected object"),
    }
}

#[test]
fn test_json_data_nested_object() {
    let mut inner = HashMap::new();
    inner.insert(
        "inner_key".to_string(),
        JsonData::String("value".to_string()),
    );

    let mut outer = HashMap::new();
    outer.insert("outer".to_string(), JsonData::Object(inner));
    let data = JsonData::Object(outer);

    match data {
        JsonData::Object(m) => assert_eq!(m.len(), 1),
        _ => panic!("Expected object"),
    }
}

#[test]
fn test_json_data_nested_array() {
    let data = JsonData::Array(vec![JsonData::Array(vec![
        JsonData::Integer(1),
        JsonData::Integer(2),
    ])]);

    match data {
        JsonData::Array(arr) => assert_eq!(arr.len(), 1),
        _ => panic!("Expected array"),
    }
}

// === Priority Tests ===

#[test]
fn test_priority_low() {
    let priority = Priority::LOW;
    assert_eq!(priority.value(), 25);
}

#[test]
fn test_priority_medium() {
    let priority = Priority::MEDIUM;
    assert_eq!(priority.value(), 50);
}

#[test]
fn test_priority_high() {
    let priority = Priority::HIGH;
    assert_eq!(priority.value(), 80);
}

#[test]
fn test_priority_critical() {
    let priority = Priority::CRITICAL;
    assert_eq!(priority.value(), 100);
}

#[test]
fn test_priority_new_valid() {
    let priority = Priority::new(100).expect("Valid priority");
    assert_eq!(priority.value(), 100);
}

#[test]
fn test_priority_new_min() {
    let priority = Priority::new(1).expect("Valid priority");
    assert_eq!(priority.value(), 1);
}

#[test]
fn test_priority_new_max() {
    let priority = Priority::new(255).expect("Valid priority");
    assert_eq!(priority.value(), 255);
}

#[test]
fn test_priority_new_zero_invalid() {
    let result = Priority::new(0);
    assert!(result.is_err());
}

#[test]
fn test_priority_comparison() {
    assert!(Priority::CRITICAL > Priority::HIGH);
    assert!(Priority::HIGH > Priority::MEDIUM);
    assert!(Priority::MEDIUM > Priority::LOW);
}

#[test]
fn test_priority_equality() {
    assert_eq!(Priority::LOW, Priority::LOW);
    assert_eq!(Priority::CRITICAL, Priority::CRITICAL);
    assert_ne!(Priority::LOW, Priority::HIGH);
}

// === Edge Cases and Boundary Conditions ===

#[test]
fn test_security_config_zero_size_limit_ignored() {
    // Zero size is ignored, default is used instead
    let config = SecurityConfig::new().set_max_json_size(0);
    assert_eq!(config.max_json_size(), 10 * 1024 * 1024); // Default 10 MB

    let input = "x";
    let result = validate_input_size(input, &config);
    assert!(result.is_ok()); // Uses default limit, so small input passes
}

#[test]
fn test_security_config_max_usize() {
    let config = SecurityConfig::new().set_max_json_size(usize::MAX);

    let input = "x".repeat(1000);
    let result = validate_input_size(&input, &config);
    assert!(result.is_ok());
}

#[test]
fn test_validate_large_json() {
    let config = SecurityConfig::new();
    // Create a large but valid JSON structure
    let mut obj = HashMap::new();
    for i in 0..1000 {
        obj.insert(format!("field{}", i), JsonData::Integer(i));
    }
    let data = JsonData::Object(obj);

    // Convert to string for validation
    let json_str = serde_json::to_string(&data).unwrap_or_default();
    let result = validate_input_size(&json_str, &config);
    assert!(result.is_ok());
}

#[test]
fn test_json_data_deeply_nested() {
    let mut current = JsonData::String("value".to_string());
    for _ in 0..10 {
        let mut map = HashMap::new();
        map.insert("nested".to_string(), current);
        current = JsonData::Object(map);
    }

    // Should not panic
    match current {
        JsonData::Object(_) => {}
        _ => panic!("Expected object"),
    }
}

#[test]
fn test_json_data_large_array() {
    let elements: Vec<JsonData> = (0..1000).map(JsonData::Integer).collect();
    let data = JsonData::Array(elements);

    match data {
        JsonData::Array(arr) => assert_eq!(arr.len(), 1000),
        _ => panic!("Expected array"),
    }
}

#[test]
fn test_json_data_large_object() {
    let mut map = HashMap::new();
    for i in 0..1000 {
        map.insert(format!("key{}", i), JsonData::Integer(i));
    }
    let data = JsonData::Object(map);

    match data {
        JsonData::Object(m) => assert_eq!(m.len(), 1000),
        _ => panic!("Expected object"),
    }
}
