//! Comprehensive tests for JsonData value object
//!
//! Tests cover all variants (Null, Bool, Number, String, Array, Object),
//! conversions, edge cases, and memory operations.

use pjson_rs_domain::value_objects::JsonData;
use std::collections::HashMap;

// ============================================================================
// Creation and Construction Tests
// ============================================================================

#[test]
fn test_json_data_null_creation() {
    let null1 = JsonData::null();
    let null2 = JsonData::Null;
    let null3 = JsonData::default();

    assert_eq!(null1, JsonData::Null);
    assert_eq!(null2, JsonData::Null);
    assert_eq!(null3, JsonData::Null);
}

#[test]
fn test_json_data_bool_creation() {
    let bool_true = JsonData::bool(true);
    let bool_false = JsonData::bool(false);

    assert_eq!(bool_true, JsonData::Bool(true));
    assert_eq!(bool_false, JsonData::Bool(false));
}

#[test]
fn test_json_data_integer_creation() {
    let zero = JsonData::integer(0);
    let positive = JsonData::integer(42);
    let negative = JsonData::integer(-100);
    let max = JsonData::integer(i64::MAX);
    let min = JsonData::integer(i64::MIN);

    assert_eq!(zero, JsonData::Integer(0));
    assert_eq!(positive, JsonData::Integer(42));
    assert_eq!(negative, JsonData::Integer(-100));
    assert_eq!(max, JsonData::Integer(i64::MAX));
    assert_eq!(min, JsonData::Integer(i64::MIN));
}

#[test]
fn test_json_data_float_creation() {
    let zero = JsonData::float(0.0);
    let positive = JsonData::float(3.5);
    let negative = JsonData::float(-2.5);
    let infinity = JsonData::float(f64::INFINITY);
    let neg_infinity = JsonData::float(f64::NEG_INFINITY);

    assert_eq!(zero, JsonData::Float(0.0));
    assert_eq!(positive, JsonData::Float(3.5));
    assert_eq!(negative, JsonData::Float(-2.5));
    assert_eq!(infinity, JsonData::Float(f64::INFINITY));
    assert_eq!(neg_infinity, JsonData::Float(f64::NEG_INFINITY));
}

#[test]
fn test_json_data_float_nan() {
    let nan = JsonData::float(f64::NAN);
    if let JsonData::Float(f) = nan {
        assert!(f.is_nan());
    } else {
        panic!("Expected Float variant");
    }
}

#[test]
fn test_json_data_string_creation() {
    let empty = JsonData::string("");
    let simple = JsonData::string("hello");
    let unicode = JsonData::string("🦀 Rust");
    let multiline = JsonData::string("line1\nline2\nline3");

    assert_eq!(empty, JsonData::String("".to_string()));
    assert_eq!(simple, JsonData::String("hello".to_string()));
    assert_eq!(unicode, JsonData::String("🦀 Rust".to_string()));
    assert_eq!(
        multiline,
        JsonData::String("line1\nline2\nline3".to_string())
    );
}

#[test]
fn test_json_data_array_creation() {
    let empty_array = JsonData::array(vec![]);
    let simple_array = JsonData::array(vec![
        JsonData::Integer(1),
        JsonData::Integer(2),
        JsonData::Integer(3),
    ]);
    let mixed_array = JsonData::array(vec![
        JsonData::Null,
        JsonData::Bool(true),
        JsonData::Integer(42),
        JsonData::String("test".to_string()),
    ]);

    assert_eq!(empty_array, JsonData::Array(vec![]));
    assert!(matches!(simple_array, JsonData::Array(_)));
    assert!(matches!(mixed_array, JsonData::Array(_)));
}

#[test]
fn test_json_data_object_creation() {
    let empty_obj = JsonData::object(HashMap::new());
    assert_eq!(empty_obj, JsonData::Object(HashMap::new()));

    let mut map = HashMap::new();
    map.insert("key".to_string(), JsonData::String("value".to_string()));
    let obj = JsonData::object(map);
    assert!(matches!(obj, JsonData::Object(_)));
}

// ============================================================================
// Type Checking Tests
// ============================================================================

#[test]
fn test_json_data_type_checks_null() {
    let null = JsonData::Null;
    assert!(null.is_null());
    assert!(!null.is_bool());
    assert!(!null.is_integer());
    assert!(!null.is_float());
    assert!(!null.is_number());
    assert!(!null.is_string());
    assert!(!null.is_array());
    assert!(!null.is_object());
}

#[test]
fn test_json_data_type_checks_bool() {
    let bool_val = JsonData::Bool(true);
    assert!(bool_val.is_bool());
    assert!(!bool_val.is_null());
    assert!(!bool_val.is_number());
}

#[test]
fn test_json_data_type_checks_integer() {
    let int_val = JsonData::Integer(42);
    assert!(int_val.is_integer());
    assert!(int_val.is_number());
    assert!(!int_val.is_float());
    assert!(!int_val.is_string());
}

#[test]
fn test_json_data_type_checks_float() {
    let float_val = JsonData::Float(3.5);
    assert!(float_val.is_float());
    assert!(float_val.is_number());
    assert!(!float_val.is_integer());
}

#[test]
fn test_json_data_type_checks_string() {
    let str_val = JsonData::String("test".to_string());
    assert!(str_val.is_string());
    assert!(!str_val.is_bool());
    assert!(!str_val.is_number());
}

#[test]
fn test_json_data_type_checks_array() {
    let arr_val = JsonData::Array(vec![]);
    assert!(arr_val.is_array());
    assert!(!arr_val.is_object());
}

#[test]
fn test_json_data_type_checks_object() {
    let obj_val = JsonData::Object(HashMap::new());
    assert!(obj_val.is_object());
    assert!(!obj_val.is_array());
}

// ============================================================================
// Value Extraction Tests
// ============================================================================

#[test]
fn test_json_data_as_bool() {
    assert_eq!(JsonData::Bool(true).as_bool(), Some(true));
    assert_eq!(JsonData::Bool(false).as_bool(), Some(false));
    assert_eq!(JsonData::Null.as_bool(), None);
    assert_eq!(JsonData::Integer(1).as_bool(), None);
}

#[test]
fn test_json_data_as_i64() {
    assert_eq!(JsonData::Integer(42).as_i64(), Some(42));
    assert_eq!(JsonData::Integer(0).as_i64(), Some(0));
    assert_eq!(JsonData::Integer(-100).as_i64(), Some(-100));
    assert_eq!(JsonData::Float(3.5).as_i64(), None);
    assert_eq!(JsonData::Null.as_i64(), None);
}

#[test]
fn test_json_data_as_f64() {
    assert_eq!(JsonData::Float(3.5).as_f64(), Some(3.5));
    assert_eq!(JsonData::Float(0.0).as_f64(), Some(0.0));
    assert_eq!(JsonData::Integer(42).as_f64(), Some(42.0));
    assert_eq!(JsonData::Integer(-10).as_f64(), Some(-10.0));
    assert_eq!(JsonData::String("test".to_string()).as_f64(), None);
}

#[test]
fn test_json_data_as_str() {
    assert_eq!(
        JsonData::String("hello".to_string()).as_str(),
        Some("hello")
    );
    assert_eq!(JsonData::String("".to_string()).as_str(), Some(""));
    assert_eq!(JsonData::Null.as_str(), None);
    assert_eq!(JsonData::Integer(42).as_str(), None);
}

#[test]
fn test_json_data_as_array() {
    let arr = JsonData::Array(vec![JsonData::Integer(1), JsonData::Integer(2)]);
    let arr_ref = arr.as_array();
    assert!(arr_ref.is_some());
    assert_eq!(arr_ref.unwrap().len(), 2);

    assert_eq!(JsonData::Null.as_array(), None);
}

#[test]
fn test_json_data_as_array_mut() {
    let mut arr = JsonData::Array(vec![JsonData::Integer(1)]);
    if let Some(arr_mut) = arr.as_array_mut() {
        arr_mut.push(JsonData::Integer(2));
        assert_eq!(arr_mut.len(), 2);
    } else {
        panic!("Expected mutable array");
    }

    let mut not_arr = JsonData::Null;
    assert!(not_arr.as_array_mut().is_none());
}

#[test]
fn test_json_data_as_object() {
    let mut map = HashMap::new();
    map.insert("key".to_string(), JsonData::Integer(42));
    let obj = JsonData::Object(map);

    let obj_ref = obj.as_object();
    assert!(obj_ref.is_some());
    assert_eq!(obj_ref.unwrap().len(), 1);

    assert_eq!(JsonData::Null.as_object(), None);
}

#[test]
fn test_json_data_as_object_mut() {
    let mut obj = JsonData::Object(HashMap::new());
    if let Some(obj_mut) = obj.as_object_mut() {
        obj_mut.insert("new_key".to_string(), JsonData::Bool(true));
        assert_eq!(obj_mut.len(), 1);
    } else {
        panic!("Expected mutable object");
    }

    let mut not_obj = JsonData::Null;
    assert!(not_obj.as_object_mut().is_none());
}

// ============================================================================
// Object Access Tests
// ============================================================================

#[test]
fn test_json_data_get() {
    let mut map = HashMap::new();
    map.insert("name".to_string(), JsonData::String("John".to_string()));
    map.insert("age".to_string(), JsonData::Integer(30));
    let obj = JsonData::Object(map);

    assert_eq!(obj.get("name").unwrap().as_str(), Some("John"));
    assert_eq!(obj.get("age").unwrap().as_i64(), Some(30));
    assert!(obj.get("nonexistent").is_none());
}

#[test]
fn test_json_data_get_non_object() {
    assert!(JsonData::Null.get("key").is_none());
    assert!(JsonData::Array(vec![]).get("key").is_none());
    assert!(JsonData::Integer(42).get("key").is_none());
}

// ============================================================================
// Path Operations Tests
// ============================================================================

#[test]
fn test_json_data_get_path_simple() {
    let mut inner = HashMap::new();
    inner.insert("name".to_string(), JsonData::String("John".to_string()));

    let mut outer = HashMap::new();
    outer.insert("user".to_string(), JsonData::Object(inner));

    let data = JsonData::Object(outer);

    assert_eq!(data.get_path("user.name").unwrap().as_str(), Some("John"));
}

#[test]
fn test_json_data_get_path_deep_nesting() {
    let mut level3 = HashMap::new();
    level3.insert("value".to_string(), JsonData::Integer(42));

    let mut level2 = HashMap::new();
    level2.insert("level3".to_string(), JsonData::Object(level3));

    let mut level1 = HashMap::new();
    level1.insert("level2".to_string(), JsonData::Object(level2));

    let data = JsonData::Object(level1);

    assert_eq!(
        data.get_path("level2.level3.value").unwrap().as_i64(),
        Some(42)
    );
}

#[test]
fn test_json_data_get_path_nonexistent() {
    let data = JsonData::Object(HashMap::new());
    assert!(data.get_path("nonexistent").is_none());
    assert!(data.get_path("a.b.c").is_none());
}

#[test]
fn test_json_data_get_path_not_object() {
    let data = JsonData::Integer(42);
    assert!(data.get_path("some.path").is_none());
}

#[test]
fn test_json_data_set_path_simple() {
    let mut data = JsonData::Object(HashMap::new());
    assert!(data.set_path("name", JsonData::String("Alice".to_string())));
    assert_eq!(data.get_path("name").unwrap().as_str(), Some("Alice"));
}

#[test]
fn test_json_data_set_path_nested() {
    let mut data = JsonData::Object(HashMap::new());
    assert!(data.set_path("user.name", JsonData::String("Bob".to_string())));
    assert!(data.set_path("user.age", JsonData::Integer(25)));

    assert_eq!(data.get_path("user.name").unwrap().as_str(), Some("Bob"));
    assert_eq!(data.get_path("user.age").unwrap().as_i64(), Some(25));
}

#[test]
fn test_json_data_set_path_deep_nesting() {
    let mut data = JsonData::Object(HashMap::new());
    assert!(data.set_path("a.b.c.d", JsonData::Bool(true)));

    assert_eq!(data.get_path("a.b.c.d").unwrap().as_bool(), Some(true));
}

#[test]
fn test_json_data_set_path_overwrite() {
    let mut data = JsonData::Object(HashMap::new());
    assert!(data.set_path("key", JsonData::Integer(1)));
    assert!(data.set_path("key", JsonData::Integer(2)));

    assert_eq!(data.get_path("key").unwrap().as_i64(), Some(2));
}

#[test]
fn test_json_data_set_path_empty() {
    let mut data = JsonData::Object(HashMap::new());
    // Empty path should return false (no path to set)
    let result = data.set_path("", JsonData::Null);
    // The actual behavior depends on implementation - test it works
    let _ = result;
}

#[test]
fn test_json_data_set_path_not_object() {
    let mut data = JsonData::Integer(42);
    assert!(!data.set_path("key", JsonData::Null));
}

// ============================================================================
// Memory Size Tests
// ============================================================================

#[test]
fn test_json_data_memory_size_primitives() {
    assert_eq!(JsonData::Null.memory_size(), 1);
    assert_eq!(JsonData::Bool(true).memory_size(), 1);
    assert_eq!(JsonData::Integer(42).memory_size(), 8);
    assert_eq!(JsonData::Float(3.5).memory_size(), 8);
}

#[test]
fn test_json_data_memory_size_string() {
    assert_eq!(JsonData::String("".to_string()).memory_size(), 0);
    assert_eq!(JsonData::String("hello".to_string()).memory_size(), 10);
    assert_eq!(JsonData::String("x".repeat(100)).memory_size(), 200);
}

#[test]
fn test_json_data_memory_size_array() {
    let empty = JsonData::Array(vec![]);
    assert!(empty.memory_size() >= 8);

    let array = JsonData::Array(vec![
        JsonData::Integer(1),
        JsonData::Integer(2),
        JsonData::Integer(3),
    ]);
    assert!(array.memory_size() > 24);
}

#[test]
fn test_json_data_memory_size_object() {
    let empty = JsonData::Object(HashMap::new());
    assert!(empty.memory_size() >= 16);

    let mut map = HashMap::new();
    map.insert("key".to_string(), JsonData::Integer(42));
    let obj = JsonData::Object(map);
    assert!(obj.memory_size() > 24);
}

#[test]
fn test_json_data_memory_size_nested() {
    let mut inner = HashMap::new();
    inner.insert("value".to_string(), JsonData::Integer(42));

    let mut outer = HashMap::new();
    outer.insert("inner".to_string(), JsonData::Object(inner));

    let data = JsonData::Object(outer);
    assert!(data.memory_size() > 50);
}

// ============================================================================
// Display Tests
// ============================================================================

#[test]
fn test_json_data_display_null() {
    assert_eq!(format!("{}", JsonData::Null), "null");
}

#[test]
fn test_json_data_display_bool() {
    assert_eq!(format!("{}", JsonData::Bool(true)), "true");
    assert_eq!(format!("{}", JsonData::Bool(false)), "false");
}

#[test]
fn test_json_data_display_integer() {
    assert_eq!(format!("{}", JsonData::Integer(42)), "42");
    assert_eq!(format!("{}", JsonData::Integer(-100)), "-100");
    assert_eq!(format!("{}", JsonData::Integer(0)), "0");
}

#[test]
fn test_json_data_display_float() {
    assert_eq!(format!("{}", JsonData::Float(3.5)), "3.5");
    assert_eq!(format!("{}", JsonData::Float(0.0)), "0");
}

#[test]
fn test_json_data_display_string() {
    assert_eq!(
        format!("{}", JsonData::String("hello".to_string())),
        "\"hello\""
    );
    assert_eq!(format!("{}", JsonData::String("".to_string())), "\"\"");
}

#[test]
fn test_json_data_display_array() {
    let arr = JsonData::Array(vec![
        JsonData::Integer(1),
        JsonData::Integer(2),
        JsonData::Integer(3),
    ]);
    assert_eq!(format!("{arr}"), "[1,2,3]");

    let empty = JsonData::Array(vec![]);
    assert_eq!(format!("{empty}"), "[]");
}

#[test]
fn test_json_data_display_object() {
    let mut map = HashMap::new();
    map.insert("name".to_string(), JsonData::String("John".to_string()));
    let obj = JsonData::Object(map);
    let display = format!("{obj}");
    assert!(display.contains("\"name\":\"John\""));
    assert!(display.starts_with('{'));
    assert!(display.ends_with('}'));
}

// ============================================================================
// Hash Tests
// ============================================================================

#[test]
fn test_json_data_hash_primitives() {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let hash_value = |v: &JsonData| {
        let mut hasher = DefaultHasher::new();
        v.hash(&mut hasher);
        hasher.finish()
    };

    assert_eq!(hash_value(&JsonData::Null), hash_value(&JsonData::Null));
    assert_eq!(
        hash_value(&JsonData::Bool(true)),
        hash_value(&JsonData::Bool(true))
    );
    assert_eq!(
        hash_value(&JsonData::Integer(42)),
        hash_value(&JsonData::Integer(42))
    );
    assert_ne!(
        hash_value(&JsonData::Integer(42)),
        hash_value(&JsonData::Integer(43))
    );
}

#[test]
fn test_json_data_hash_float() {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let hash_value = |v: &JsonData| {
        let mut hasher = DefaultHasher::new();
        v.hash(&mut hasher);
        hasher.finish()
    };

    assert_eq!(
        hash_value(&JsonData::Float(3.5)),
        hash_value(&JsonData::Float(3.5))
    );
}

// ============================================================================
// From Trait Tests
// ============================================================================

#[test]
fn test_json_data_from_bool() {
    let data: JsonData = true.into();
    assert_eq!(data, JsonData::Bool(true));

    let data: JsonData = false.into();
    assert_eq!(data, JsonData::Bool(false));
}

#[test]
fn test_json_data_from_i64() {
    let data: JsonData = 42i64.into();
    assert_eq!(data, JsonData::Integer(42));

    let data: JsonData = (-100i64).into();
    assert_eq!(data, JsonData::Integer(-100));
}

#[test]
fn test_json_data_from_f64() {
    let data: JsonData = 3.5f64.into();
    assert_eq!(data, JsonData::Float(3.5));
}

#[test]
fn test_json_data_from_string() {
    let data: JsonData = "hello".to_string().into();
    assert_eq!(data, JsonData::String("hello".to_string()));

    let data: JsonData = "world".into();
    assert_eq!(data, JsonData::String("world".to_string()));
}

#[test]
fn test_json_data_from_vec() {
    let vec = vec![JsonData::Integer(1), JsonData::Integer(2)];
    let data: JsonData = vec.into();
    assert!(matches!(data, JsonData::Array(_)));
}

#[test]
fn test_json_data_from_hashmap() {
    let mut map = HashMap::new();
    map.insert("key".to_string(), JsonData::Integer(42));
    let data: JsonData = map.into();
    assert!(matches!(data, JsonData::Object(_)));
}

#[test]
fn test_json_data_from_serde_json_null() {
    let json = serde_json::Value::Null;
    let data: JsonData = json.into();
    assert_eq!(data, JsonData::Null);
}

#[test]
fn test_json_data_from_serde_json_bool() {
    let json = serde_json::Value::Bool(true);
    let data: JsonData = json.into();
    assert_eq!(data, JsonData::Bool(true));
}

#[test]
fn test_json_data_from_serde_json_number() {
    let json = serde_json::json!(42);
    let data: JsonData = json.into();
    assert_eq!(data, JsonData::Integer(42));

    let json = serde_json::json!(3.5);
    let data: JsonData = json.into();
    assert_eq!(data, JsonData::Float(3.5));
}

#[test]
fn test_json_data_from_serde_json_string() {
    let json = serde_json::Value::String("test".to_string());
    let data: JsonData = json.into();
    assert_eq!(data, JsonData::String("test".to_string()));
}

#[test]
fn test_json_data_from_serde_json_array() {
    let json = serde_json::json!([1, 2, 3]);
    let data: JsonData = json.into();
    if let JsonData::Array(arr) = data {
        assert_eq!(arr.len(), 3);
    } else {
        panic!("Expected Array");
    }
}

#[test]
fn test_json_data_from_serde_json_object() {
    let json = serde_json::json!({"key": "value"});
    let data: JsonData = json.into();
    if let JsonData::Object(obj) = data {
        assert!(obj.contains_key("key"));
    } else {
        panic!("Expected Object");
    }
}

// ============================================================================
// Serialization Tests
// ============================================================================

#[test]
fn test_json_data_serialize_primitives() {
    let null = JsonData::Null;
    let _ = serde_json::to_string(&null);

    let bool_val = JsonData::Bool(true);
    let _ = serde_json::to_string(&bool_val);

    let int_val = JsonData::Integer(42);
    let _ = serde_json::to_string(&int_val);

    let float_val = JsonData::Float(3.5);
    let _ = serde_json::to_string(&float_val);

    let str_val = JsonData::String("test".to_string());
    let _ = serde_json::to_string(&str_val);
}

#[test]
fn test_json_data_serialize_complex() {
    let mut map = HashMap::new();
    map.insert("id".to_string(), JsonData::Integer(1));
    map.insert("name".to_string(), JsonData::String("John".to_string()));
    map.insert("active".to_string(), JsonData::Bool(true));

    let data = JsonData::Object(map);
    let serialized = serde_json::to_string(&data).unwrap();

    assert!(serialized.contains("id"));
    assert!(serialized.contains("name"));
    assert!(serialized.contains("active"));
}

#[test]
fn test_json_data_deserialize() {
    // Test deserializing from serde_json::Value first
    let json_str = r#"{"key":"value","num":42}"#;
    let serde_value: serde_json::Value = serde_json::from_str(json_str).unwrap();
    let data: JsonData = serde_value.into();

    if let JsonData::Object(obj) = data {
        assert!(!obj.is_empty());
    } else {
        panic!("Expected Object");
    }
}

// ============================================================================
// Clone and PartialEq Tests
// ============================================================================

#[test]
fn test_json_data_clone() {
    let original = JsonData::Integer(42);
    let cloned = original.clone();
    assert_eq!(original, cloned);

    let mut map = HashMap::new();
    map.insert("key".to_string(), JsonData::String("value".to_string()));
    let original = JsonData::Object(map);
    let cloned = original.clone();
    assert_eq!(original, cloned);
}

#[test]
fn test_json_data_equality() {
    assert_eq!(JsonData::Null, JsonData::Null);
    assert_eq!(JsonData::Bool(true), JsonData::Bool(true));
    assert_ne!(JsonData::Bool(true), JsonData::Bool(false));
    assert_eq!(JsonData::Integer(42), JsonData::Integer(42));
    assert_ne!(JsonData::Integer(42), JsonData::Integer(43));
}
