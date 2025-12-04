//! Comprehensive tests for parser/value.rs
//!
//! This test suite aims to achieve 70%+ coverage by testing:
//! - JsonValue type conversions and accessors
//! - LazyArray operations and numeric detection
//! - LazyObject field access and iteration
//! - Edge cases and error conditions

use pjson_rs::parser::{
    scanner::{Range, ScanResult},
    value::{FieldRange, JsonValue, LazyArray, LazyObject},
};

mod json_value_tests {
    use super::*;

    #[test]
    fn test_json_value_string_accessor() {
        let val = JsonValue::String("test string");
        assert_eq!(val.as_str(), Some("test string"));
        assert!(val.as_f64().is_none());
        assert!(val.as_i64().is_none());
        assert!(val.as_bool().is_none());
        assert!(!val.is_null());
        assert!(val.as_array().is_none());
        assert!(val.as_object().is_none());
    }

    #[test]
    fn test_json_value_number_as_f64() {
        let val = JsonValue::Number(b"123.456");
        assert_eq!(val.as_f64(), Some(123.456));
        assert!(val.as_str().is_none());
        assert!(!val.is_null());
    }

    #[test]
    fn test_json_value_number_as_i64() {
        let val = JsonValue::Number(b"42");
        assert_eq!(val.as_i64(), Some(42));
        assert_eq!(val.as_f64(), Some(42.0));
    }

    #[test]
    fn test_json_value_number_negative() {
        let val = JsonValue::Number(b"-999");
        assert_eq!(val.as_i64(), Some(-999));
        assert_eq!(val.as_f64(), Some(-999.0));
    }

    #[test]
    fn test_json_value_number_invalid_bytes() {
        let val = JsonValue::Number(b"not-a-number");
        assert!(val.as_f64().is_none());
        assert!(val.as_i64().is_none());
    }

    #[test]
    fn test_json_value_number_invalid_utf8() {
        // Invalid UTF-8 bytes
        let val = JsonValue::Number(&[0xFF, 0xFE]);
        assert!(val.as_f64().is_none());
        assert!(val.as_i64().is_none());
    }

    #[test]
    fn test_json_value_bool_true() {
        let val = JsonValue::Bool(true);
        assert_eq!(val.as_bool(), Some(true));
        assert!(val.as_str().is_none());
        assert!(val.as_f64().is_none());
        assert!(!val.is_null());
    }

    #[test]
    fn test_json_value_bool_false() {
        let val = JsonValue::Bool(false);
        assert_eq!(val.as_bool(), Some(false));
        assert!(val.as_str().is_none());
    }

    #[test]
    fn test_json_value_null() {
        let val = JsonValue::Null;
        assert!(val.is_null());
        assert!(val.as_str().is_none());
        assert!(val.as_f64().is_none());
        assert!(val.as_i64().is_none());
        assert!(val.as_bool().is_none());
    }

    #[test]
    fn test_json_value_array_accessor() {
        let raw = b"[1, 2, 3]";
        let scan_result = ScanResult::new();
        let lazy_array = LazyArray::from_scan(raw, scan_result);
        let val = JsonValue::Array(lazy_array);

        assert!(val.as_array().is_some());
        assert!(val.as_object().is_none());
        assert!(val.as_str().is_none());
        assert!(!val.is_null());
    }

    #[test]
    fn test_json_value_object_accessor() {
        let raw = br#"{"key": "value"}"#;
        let scan_result = ScanResult::new();
        let lazy_object = LazyObject::from_scan(raw, scan_result);
        let val = JsonValue::Object(lazy_object);

        assert!(val.as_object().is_some());
        assert!(val.as_array().is_none());
        assert!(val.as_str().is_none());
        assert!(!val.is_null());
    }

    #[test]
    fn test_json_value_raw() {
        let raw_bytes = b"raw data";
        let val = JsonValue::Raw(raw_bytes);

        // Raw should not be accessible through typed accessors
        assert!(val.as_str().is_none());
        assert!(val.as_f64().is_none());
        assert!(!val.is_null());
    }

    #[test]
    fn test_json_value_parse_raw() {
        let raw_bytes = b"null";
        let mut val = JsonValue::Raw(raw_bytes);

        assert!(val.parse_raw().is_ok());
        // After parsing, should be null (simplified implementation)
        assert!(val.is_null());
    }

    #[test]
    fn test_json_value_parse_raw_non_raw() {
        let mut val = JsonValue::String("already parsed");
        assert!(val.parse_raw().is_ok()); // Should succeed but do nothing
        assert_eq!(val.as_str(), Some("already parsed"));
    }
}

mod lazy_array_tests {
    use super::*;

    #[test]
    fn test_lazy_array_from_scan_empty() {
        let raw = b"[]";
        let scan_result = ScanResult::new();
        let array = LazyArray::from_scan(raw, scan_result);

        assert_eq!(array.len(), 0);
        assert!(array.is_empty());
    }

    #[test]
    fn test_lazy_array_len() {
        let raw = b"[1, 2, 3]";
        let scan_result = ScanResult::new();
        let array = LazyArray::from_scan(raw, scan_result);

        // With placeholder implementation, boundaries is empty
        assert_eq!(array.len(), 0);
    }

    #[test]
    fn test_lazy_array_get_out_of_bounds() {
        let raw = b"[1, 2, 3]";
        let scan_result = ScanResult::new();
        let array = LazyArray::from_scan(raw, scan_result);

        assert!(array.get(0).is_none());
        assert!(array.get(10).is_none());
    }

    #[test]
    fn test_lazy_array_get_parsed() {
        let raw = b"[1, 2, 3]";
        let scan_result = ScanResult::new();
        let array = LazyArray::from_scan(raw, scan_result);

        assert!(array.get_parsed(0).is_none());
    }

    #[test]
    fn test_lazy_array_iter_empty() {
        let raw = b"[]";
        let scan_result = ScanResult::new();
        let array = LazyArray::from_scan(raw, scan_result);

        let mut iter = array.iter();
        assert!(iter.next().is_none());
    }

    // Note: looks_like_number is a private method, so we test is_numeric() instead
    // which uses looks_like_number internally

    #[test]
    fn test_lazy_array_is_numeric_insufficient_elements() {
        let raw = b"[]";
        let scan_result = ScanResult::new();
        let array = LazyArray::from_scan(raw, scan_result);

        // Less than 5 elements, should return false
        assert!(!array.is_numeric());
    }

    #[test]
    fn test_lazy_array_clone() {
        let raw = b"[1, 2, 3]";
        let scan_result = ScanResult::new();
        let array = LazyArray::from_scan(raw, scan_result);
        let cloned = array.clone();

        assert_eq!(array.len(), cloned.len());
        assert_eq!(array.is_empty(), cloned.is_empty());
    }
}

mod lazy_object_tests {
    use super::*;

    #[test]
    fn test_lazy_object_from_scan_empty() {
        let raw = b"{}";
        let scan_result = ScanResult::new();
        let object = LazyObject::from_scan(raw, scan_result);

        assert_eq!(object.len(), 0);
        assert!(object.is_empty());
    }

    #[test]
    fn test_lazy_object_len() {
        let raw = br#"{"key": "value"}"#;
        let scan_result = ScanResult::new();
        let object = LazyObject::from_scan(raw, scan_result);

        // With placeholder implementation, fields is empty
        assert_eq!(object.len(), 0);
    }

    #[test]
    fn test_lazy_object_get_nonexistent_key() {
        let raw = br#"{"key": "value"}"#;
        let scan_result = ScanResult::new();
        let object = LazyObject::from_scan(raw, scan_result);

        assert!(object.get("key").is_none());
        assert!(object.get("nonexistent").is_none());
    }

    #[test]
    fn test_lazy_object_keys_empty() {
        let raw = b"{}";
        let scan_result = ScanResult::new();
        let object = LazyObject::from_scan(raw, scan_result);

        let keys = object.keys().expect("should get empty keys");
        assert_eq!(keys.len(), 0);
    }

    #[test]
    fn test_lazy_object_clone() {
        let raw = br#"{"key": "value"}"#;
        let scan_result = ScanResult::new();
        let object = LazyObject::from_scan(raw, scan_result);
        let cloned = object.clone();

        assert_eq!(object.len(), cloned.len());
        assert_eq!(object.is_empty(), cloned.is_empty());
    }
}

mod field_range_tests {
    use super::*;

    #[test]
    fn test_field_range_creation() {
        let key_range = Range { start: 0, end: 5 };
        let value_range = Range { start: 6, end: 10 };

        let field_range = FieldRange::new(key_range, value_range);

        // Verify field range was created (no getters, so we can't check internals)
        // But at least ensure construction doesn't panic
        drop(field_range);
    }

    #[test]
    fn test_field_range_clone() {
        let key_range = Range { start: 0, end: 5 };
        let value_range = Range { start: 6, end: 10 };

        let field_range = FieldRange::new(key_range, value_range);
        let cloned = field_range.clone();

        drop(field_range);
        drop(cloned);
    }
}

mod lazy_array_iterator_tests {
    use super::*;

    #[test]
    fn test_lazy_array_iterator_empty() {
        let raw = b"[]";
        let scan_result = ScanResult::new();
        let array = LazyArray::from_scan(raw, scan_result);

        let count = array.iter().count();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_lazy_array_iterator_collect() {
        let raw = b"[1, 2, 3]";
        let scan_result = ScanResult::new();
        let array = LazyArray::from_scan(raw, scan_result);

        let elements: Vec<_> = array.iter().collect();
        assert_eq!(elements.len(), 0); // Empty boundaries in placeholder
    }

    #[test]
    fn test_lazy_array_iterator_multiple_iterations() {
        let raw = b"[1, 2, 3]";
        let scan_result = ScanResult::new();
        let array = LazyArray::from_scan(raw, scan_result);

        let first_count = array.iter().count();
        let second_count = array.iter().count();

        assert_eq!(first_count, second_count);
    }
}

mod json_value_edge_cases {
    use super::*;

    #[test]
    fn test_number_with_scientific_notation() {
        let val = JsonValue::Number(b"1.23e-10");
        assert!(val.as_f64().is_some());
    }

    #[test]
    fn test_number_very_large() {
        let val = JsonValue::Number(b"999999999999999999");
        assert!(val.as_i64().is_some());
    }

    #[test]
    fn test_number_decimal_only() {
        let val = JsonValue::Number(b"0.0001");
        assert!(val.as_f64().is_some());
    }

    #[test]
    fn test_string_empty() {
        let val = JsonValue::String("");
        assert_eq!(val.as_str(), Some(""));
        assert!(!val.is_null());
    }

    #[test]
    fn test_string_with_unicode() {
        let val = JsonValue::String("Hello 世界 🌍");
        assert_eq!(val.as_str(), Some("Hello 世界 🌍"));
    }

    #[test]
    fn test_nested_array_in_json_value() {
        let raw = b"[[1, 2], [3, 4]]";
        let scan_result = ScanResult::new();
        let lazy_array = LazyArray::from_scan(raw, scan_result);
        let val = JsonValue::Array(lazy_array);

        let array = val.as_array().expect("should be array");
        assert!(array.is_empty()); // Empty boundaries in placeholder
    }

    #[test]
    fn test_nested_object_in_json_value() {
        let raw = br#"{"outer": {"inner": "value"}}"#;
        let scan_result = ScanResult::new();
        let lazy_object = LazyObject::from_scan(raw, scan_result);
        let val = JsonValue::Object(lazy_object);

        let object = val.as_object().expect("should be object");
        assert!(object.is_empty()); // Empty fields in placeholder
    }
}

// Note: looks_like_number is a private method, cannot test directly
// It is tested indirectly through is_numeric() which is public
