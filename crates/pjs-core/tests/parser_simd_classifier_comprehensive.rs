//! Comprehensive tests for parser/simd/mod.rs
//!
//! This test suite aims to achieve 70%+ coverage by testing:
//! - SIMD value classification for all JSON types
//! - Numeric array detection and string length calculation
//! - Object key scanning with special field detection
//! - Vectorized operations for large datasets
//! - Numeric operations (sum, stats) on arrays
//! - Performance characteristics for different data sizes

use pjson_rs::parser::simd::{
    ArrayStats, KeyScanResult, SimdClassifier, SimdNumericOps, ValueClass,
};


// === Value Classification Tests ===

mod value_classification_tests {
    use super::*;

    #[test]
    fn test_classify_integer() {
        let json = "42";
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        let class = SimdClassifier::classify_value_type(&value);
        assert!(matches!(
            class,
            ValueClass::Integer | ValueClass::UnsignedInteger
        ));
    }

    #[test]
    fn test_classify_unsigned_integer() {
        let json = "12345";
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        let class = SimdClassifier::classify_value_type(&value);
        assert!(matches!(
            class,
            ValueClass::Integer | ValueClass::UnsignedInteger
        ));
    }

    #[test]
    fn test_classify_float() {
        let json = "3.14";
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        let class = SimdClassifier::classify_value_type(&value);
        assert_eq!(class, ValueClass::Float);
    }

    #[test]
    fn test_classify_string() {
        let json = r#""hello world""#;
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        let class = SimdClassifier::classify_value_type(&value);
        assert_eq!(class, ValueClass::String);
    }

    #[test]
    fn test_classify_array() {
        let json = "[1, 2, 3]";
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        let class = SimdClassifier::classify_value_type(&value);
        assert_eq!(class, ValueClass::Array);
    }

    #[test]
    fn test_classify_object() {
        let json = r#"{"key": "value"}"#;
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        let class = SimdClassifier::classify_value_type(&value);
        assert_eq!(class, ValueClass::Object);
    }

    #[test]
    fn test_classify_boolean_true() {
        let json = "true";
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        let class = SimdClassifier::classify_value_type(&value);
        assert_eq!(class, ValueClass::Boolean);
    }

    #[test]
    fn test_classify_boolean_false() {
        let json = "false";
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        let class = SimdClassifier::classify_value_type(&value);
        assert_eq!(class, ValueClass::Boolean);
    }

    #[test]
    fn test_classify_null() {
        let json = "null";
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        let class = SimdClassifier::classify_value_type(&value);
        assert_eq!(class, ValueClass::Null);
    }

    #[test]
    fn test_classify_negative_integer() {
        let json = "-999";
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        let class = SimdClassifier::classify_value_type(&value);
        assert_eq!(class, ValueClass::Integer);
    }

    #[test]
    fn test_classify_scientific_notation() {
        let json = "1.23e10";
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        let class = SimdClassifier::classify_value_type(&value);
        assert_eq!(class, ValueClass::Float);
    }

    #[test]
    fn test_classify_zero() {
        let json = "0";
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        let class = SimdClassifier::classify_value_type(&value);
        assert!(matches!(
            class,
            ValueClass::Integer | ValueClass::UnsignedInteger
        ));
    }

    #[test]
    fn test_classify_empty_string() {
        let json = r#""""#;
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        let class = SimdClassifier::classify_value_type(&value);
        assert_eq!(class, ValueClass::String);
    }

    #[test]
    fn test_classify_empty_array() {
        let json = "[]";
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        let class = SimdClassifier::classify_value_type(&value);
        assert_eq!(class, ValueClass::Array);
    }

    #[test]
    fn test_classify_empty_object() {
        let json = "{}";
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        let class = SimdClassifier::classify_value_type(&value);
        assert_eq!(class, ValueClass::Object);
    }
}

// === Numeric Array Detection Tests ===

mod numeric_array_tests {
    use super::*;
    use sonic_rs::JsonContainerTrait;

    #[test]
    fn test_is_numeric_array_small() {
        let json = "[1, 2]";
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        if let Some(arr) = value.as_array() {
            assert!(!SimdClassifier::is_numeric_array(arr)); // Less than 3 elements
        }
    }

    #[test]
    fn test_is_numeric_array_valid() {
        let json = "[1, 2, 3, 4, 5]";
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        if let Some(arr) = value.as_array() {
            assert!(SimdClassifier::is_numeric_array(arr));
        }
    }

    #[test]
    fn test_is_numeric_array_mixed_types() {
        let json = r#"[1, "two", 3]"#;
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        if let Some(arr) = value.as_array() {
            assert!(!SimdClassifier::is_numeric_array(arr));
        }
    }

    #[test]
    fn test_is_numeric_array_with_null() {
        let json = "[1, 2, null, 4]";
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        if let Some(arr) = value.as_array() {
            assert!(!SimdClassifier::is_numeric_array(arr));
        }
    }

    #[test]
    fn test_is_numeric_array_floats() {
        let json = "[1.1, 2.2, 3.3, 4.4]";
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        if let Some(arr) = value.as_array() {
            assert!(SimdClassifier::is_numeric_array(arr));
        }
    }

    #[test]
    fn test_is_numeric_array_large() {
        let json = format!(
            "[{}]",
            (0..100)
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );
        let value: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
        if let Some(arr) = value.as_array() {
            assert!(SimdClassifier::is_numeric_array(arr));
        }
    }

    #[test]
    fn test_is_numeric_array_strings() {
        let json = r#"["a", "b", "c", "d"]"#;
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        if let Some(arr) = value.as_array() {
            assert!(!SimdClassifier::is_numeric_array(arr));
        }
    }

    #[test]
    fn test_is_numeric_array_exactly_three() {
        let json = "[1, 2, 3]";
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        if let Some(arr) = value.as_array() {
            assert!(SimdClassifier::is_numeric_array(arr));
        }
    }
}

// === String Length Calculation Tests ===

mod string_length_tests {
    use super::*;
    use sonic_rs::JsonContainerTrait;

    #[test]
    fn test_calculate_string_length_empty() {
        let json = "[]";
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        if let Some(arr) = value.as_array() {
            let total_len = SimdClassifier::calculate_total_string_length(arr);
            assert_eq!(total_len, 0);
        }
    }

    #[test]
    fn test_calculate_string_length_small() {
        let json = r#"["a", "bb", "ccc"]"#;
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        if let Some(arr) = value.as_array() {
            let total_len = SimdClassifier::calculate_total_string_length(arr);
            assert_eq!(total_len, 6); // 1 + 2 + 3
        }
    }

    #[test]
    fn test_calculate_string_length_large() {
        // Create array with more than 32 elements to trigger vectorized path
        let strings: Vec<String> = (0..50).map(|i| format!(r#""string{}""#, i)).collect();
        let json = format!("[{}]", strings.join(","));
        let value: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
        if let Some(arr) = value.as_array() {
            let total_len = SimdClassifier::calculate_total_string_length(arr);
            assert!(total_len > 0);
        }
    }

    #[test]
    fn test_calculate_string_length_mixed() {
        let json = r#"["hello", 123, "world"]"#;
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        if let Some(arr) = value.as_array() {
            let total_len = SimdClassifier::calculate_total_string_length(arr);
            assert_eq!(total_len, 10); // "hello" + "world"
        }
    }

    #[test]
    fn test_calculate_string_length_no_strings() {
        let json = "[1, 2, 3, 4, 5]";
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        if let Some(arr) = value.as_array() {
            let total_len = SimdClassifier::calculate_total_string_length(arr);
            assert_eq!(total_len, 0);
        }
    }

    #[test]
    fn test_calculate_string_length_exactly_32() {
        let strings: Vec<String> = (0..32).map(|i| format!(r#""s{}""#, i)).collect();
        let json = format!("[{}]", strings.join(","));
        let value: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
        if let Some(arr) = value.as_array() {
            let total_len = SimdClassifier::calculate_total_string_length(arr);
            assert!(total_len > 0);
        }
    }

    #[test]
    fn test_calculate_string_length_vectorized_path() {
        // Exactly 33 elements to trigger vectorized path (> 32)
        let strings: Vec<String> = (0..33).map(|i| format!(r#""str{}""#, i)).collect();
        let json = format!("[{}]", strings.join(","));
        let value: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
        if let Some(arr) = value.as_array() {
            let total_len = SimdClassifier::calculate_total_string_length(arr);
            assert!(total_len > 0);
        }
    }
}

// === Object Key Scanning Tests ===

mod key_scanning_tests {
    use super::*;
    use sonic_rs::JsonContainerTrait;

    #[test]
    fn test_scan_empty_object() {
        let json = "{}";
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        if let Some(obj) = value.as_object() {
            let result = SimdClassifier::scan_object_keys(obj);
            assert_eq!(result.key_count, 0);
            assert!(!result.has_timestamp);
            assert!(!result.has_coordinates);
            assert!(!result.has_type_field);
        }
    }

    #[test]
    fn test_scan_object_with_timestamp() {
        let json = r#"{"timestamp": 123456789}"#;
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        if let Some(obj) = value.as_object() {
            let result = SimdClassifier::scan_object_keys(obj);
            assert_eq!(result.key_count, 1);
            assert!(result.has_timestamp);
        }
    }

    #[test]
    fn test_scan_object_with_time() {
        let json = r#"{"time": 123456789}"#;
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        if let Some(obj) = value.as_object() {
            let result = SimdClassifier::scan_object_keys(obj);
            assert!(result.has_timestamp);
        }
    }

    #[test]
    fn test_scan_object_with_coordinates() {
        let json = r#"{"coordinates": [1.0, 2.0]}"#;
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        if let Some(obj) = value.as_object() {
            let result = SimdClassifier::scan_object_keys(obj);
            assert!(result.has_coordinates);
        }
    }

    #[test]
    fn test_scan_object_with_coord() {
        let json = r#"{"coord": [1.0, 2.0]}"#;
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        if let Some(obj) = value.as_object() {
            let result = SimdClassifier::scan_object_keys(obj);
            assert!(result.has_coordinates);
        }
    }

    #[test]
    fn test_scan_object_with_type() {
        let json = r#"{"type": "Point"}"#;
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        if let Some(obj) = value.as_object() {
            let result = SimdClassifier::scan_object_keys(obj);
            assert!(result.has_type_field);
        }
    }

    #[test]
    fn test_scan_object_with_all_special_keys() {
        let json = r#"{"timestamp": 123, "coordinates": [1, 2], "type": "test"}"#;
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        if let Some(obj) = value.as_object() {
            let result = SimdClassifier::scan_object_keys(obj);
            assert_eq!(result.key_count, 3);
            assert!(result.has_timestamp);
            assert!(result.has_coordinates);
            assert!(result.has_type_field);
        }
    }

    #[test]
    fn test_scan_object_with_regular_keys() {
        let json = r#"{"name": "test", "value": 42}"#;
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        if let Some(obj) = value.as_object() {
            let result = SimdClassifier::scan_object_keys(obj);
            assert_eq!(result.key_count, 2);
            assert!(!result.has_timestamp);
            assert!(!result.has_coordinates);
            assert!(!result.has_type_field);
        }
    }

    #[test]
    fn test_scan_small_object() {
        let json = r#"{"a": 1, "b": 2, "c": 3, "timestamp": 999}"#;
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        if let Some(obj) = value.as_object() {
            let result = SimdClassifier::scan_object_keys(obj);
            assert_eq!(result.key_count, 4);
            assert!(result.has_timestamp);
        }
    }

    #[test]
    fn test_scan_large_object() {
        // Create object with more than 16 keys to trigger vectorized path
        let mut fields = Vec::new();
        for i in 0..20 {
            fields.push(format!(r#""field{}": {}"#, i, i));
        }
        fields.push(r#""timestamp": 123"#.to_string());
        let json = format!("{{{}}}", fields.join(","));
        let value: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
        if let Some(obj) = value.as_object() {
            let result = SimdClassifier::scan_object_keys(obj);
            assert_eq!(result.key_count, 21);
            assert!(result.has_timestamp);
        }
    }

    #[test]
    fn test_scan_object_exactly_16_keys() {
        let mut fields = Vec::new();
        for i in 0..16 {
            fields.push(format!(r#""field{}": {}"#, i, i));
        }
        let json = format!("{{{}}}", fields.join(","));
        let value: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
        if let Some(obj) = value.as_object() {
            let result = SimdClassifier::scan_object_keys(obj);
            assert_eq!(result.key_count, 16);
        }
    }

    #[test]
    fn test_scan_object_17_keys_triggers_vectorized() {
        let mut fields = Vec::new();
        for i in 0..17 {
            fields.push(format!(r#""field{}": {}"#, i, i));
        }
        let json = format!("{{{}}}", fields.join(","));
        let value: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
        if let Some(obj) = value.as_object() {
            let result = SimdClassifier::scan_object_keys(obj);
            assert_eq!(result.key_count, 17);
        }
    }
}

// === Numeric Operations Tests ===

mod numeric_ops_tests {
    use super::*;
    use sonic_rs::JsonContainerTrait;

    #[test]
    fn test_fast_array_sum_empty() {
        let json = "[]";
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        if let Some(arr) = value.as_array() {
            let sum = SimdNumericOps::fast_array_sum(arr);
            assert_eq!(sum, None);
        }
    }

    #[test]
    fn test_fast_array_sum_small() {
        let json = "[1, 2, 3, 4, 5]";
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        if let Some(arr) = value.as_array() {
            let sum = SimdNumericOps::fast_array_sum(arr).unwrap();
            assert_eq!(sum, 15.0);
        }
    }

    #[test]
    fn test_fast_array_sum_floats() {
        let json = "[1.5, 2.5, 3.5]";
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        if let Some(arr) = value.as_array() {
            let sum = SimdNumericOps::fast_array_sum(arr).unwrap();
            assert!((sum - 7.5).abs() < 1e-10);
        }
    }

    #[test]
    fn test_fast_array_sum_large() {
        // More than 64 elements to trigger vectorized path
        let json = format!(
            "[{}]",
            (0..100)
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );
        let value: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
        if let Some(arr) = value.as_array() {
            let sum = SimdNumericOps::fast_array_sum(arr).unwrap();
            assert_eq!(sum, 4950.0); // Sum of 0..100
        }
    }

    #[test]
    fn test_fast_array_sum_non_numeric() {
        let json = r#"["a", "b", "c"]"#;
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        if let Some(arr) = value.as_array() {
            let sum = SimdNumericOps::fast_array_sum(arr);
            assert_eq!(sum, None);
        }
    }

    #[test]
    fn test_fast_array_sum_mixed() {
        let json = r#"[1, 2, "three"]"#;
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        if let Some(arr) = value.as_array() {
            let sum = SimdNumericOps::fast_array_sum(arr);
            assert_eq!(sum, None);
        }
    }

    #[test]
    fn test_fast_array_sum_exactly_64() {
        let json = format!(
            "[{}]",
            (0..64).map(|i| i.to_string()).collect::<Vec<_>>().join(",")
        );
        let value: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
        if let Some(arr) = value.as_array() {
            let sum = SimdNumericOps::fast_array_sum(arr).unwrap();
            assert_eq!(sum, 2016.0); // Sum of 0..64
        }
    }

    #[test]
    fn test_fast_array_sum_65_triggers_vectorized() {
        let json = format!(
            "[{}]",
            (0..65).map(|i| i.to_string()).collect::<Vec<_>>().join(",")
        );
        let value: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
        if let Some(arr) = value.as_array() {
            let sum = SimdNumericOps::fast_array_sum(arr).unwrap();
            assert_eq!(sum, 2080.0); // Sum of 0..65
        }
    }

    #[test]
    fn test_fast_array_sum_negative_numbers() {
        let json = "[-5, -3, 2, 4]";
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        if let Some(arr) = value.as_array() {
            let sum = SimdNumericOps::fast_array_sum(arr).unwrap();
            assert_eq!(sum, -2.0);
        }
    }
}

// === Array Statistics Tests ===

mod array_stats_tests {
    use super::*;
    use sonic_rs::JsonContainerTrait;

    #[test]
    fn test_array_stats_empty() {
        let json = "[]";
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        if let Some(arr) = value.as_array() {
            let stats = SimdNumericOps::array_stats(arr);
            assert!(stats.is_none());
        }
    }

    #[test]
    fn test_array_stats_single_element() {
        let json = "[42]";
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        if let Some(arr) = value.as_array() {
            let stats = SimdNumericOps::array_stats(arr).unwrap();
            assert_eq!(stats.count, 1);
            assert_eq!(stats.min, 42.0);
            assert_eq!(stats.max, 42.0);
            assert_eq!(stats.sum, 42.0);
            assert_eq!(stats.mean(), 42.0);
        }
    }

    #[test]
    fn test_array_stats_multiple() {
        let json = "[1.5, 2.0, 3.5, 4.0]";
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        if let Some(arr) = value.as_array() {
            let stats = SimdNumericOps::array_stats(arr).unwrap();
            assert_eq!(stats.count, 4);
            assert_eq!(stats.min, 1.5);
            assert_eq!(stats.max, 4.0);
            assert_eq!(stats.sum, 11.0);
            assert_eq!(stats.mean(), 2.75);
        }
    }

    #[test]
    fn test_array_stats_negative() {
        let json = "[-5, -2, 0, 3, 7]";
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        if let Some(arr) = value.as_array() {
            let stats = SimdNumericOps::array_stats(arr).unwrap();
            assert_eq!(stats.count, 5);
            assert_eq!(stats.min, -5.0);
            assert_eq!(stats.max, 7.0);
            assert_eq!(stats.sum, 3.0);
            assert_eq!(stats.mean(), 0.6);
        }
    }

    #[test]
    fn test_array_stats_non_numeric() {
        let json = r#"["a", "b", "c"]"#;
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        if let Some(arr) = value.as_array() {
            let stats = SimdNumericOps::array_stats(arr);
            assert!(stats.is_none());
        }
    }

    #[test]
    fn test_array_stats_mixed_types() {
        let json = r#"[1, "two", 3]"#;
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        if let Some(arr) = value.as_array() {
            let stats = SimdNumericOps::array_stats(arr);
            assert!(stats.is_none());
        }
    }

    #[test]
    fn test_array_stats_mean_zero_count() {
        let stats = ArrayStats {
            min: 0.0,
            max: 0.0,
            sum: 0.0,
            count: 0,
        };
        assert_eq!(stats.mean(), 0.0);
    }

    #[test]
    fn test_array_stats_large_array() {
        let json = format!(
            "[{}]",
            (1..=100)
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );
        let value: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
        if let Some(arr) = value.as_array() {
            let stats = SimdNumericOps::array_stats(arr).unwrap();
            assert_eq!(stats.count, 100);
            assert_eq!(stats.min, 1.0);
            assert_eq!(stats.max, 100.0);
            assert_eq!(stats.sum, 5050.0);
            assert_eq!(stats.mean(), 50.5);
        }
    }

    #[test]
    fn test_array_stats_all_same() {
        let json = "[5, 5, 5, 5, 5]";
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        if let Some(arr) = value.as_array() {
            let stats = SimdNumericOps::array_stats(arr).unwrap();
            assert_eq!(stats.count, 5);
            assert_eq!(stats.min, 5.0);
            assert_eq!(stats.max, 5.0);
            assert_eq!(stats.sum, 25.0);
            assert_eq!(stats.mean(), 5.0);
        }
    }
}

// === Value Class Tests ===

mod value_class_tests {
    use super::*;

    #[test]
    fn test_value_class_equality() {
        assert_eq!(ValueClass::Integer, ValueClass::Integer);
        assert_eq!(ValueClass::String, ValueClass::String);
        assert_ne!(ValueClass::Integer, ValueClass::Float);
    }

    #[test]
    fn test_value_class_clone() {
        let class1 = ValueClass::Object;
        let class2 = class1;
        assert_eq!(class1, class2);
    }

    #[test]
    fn test_value_class_debug() {
        let class = ValueClass::Array;
        let debug_str = format!("{:?}", class);
        assert!(debug_str.contains("Array"));
    }
}

// === Key Scan Result Tests ===

mod key_scan_result_tests {
    use super::*;

    #[test]
    fn test_key_scan_result_default() {
        let result = KeyScanResult::default();
        assert!(!result.has_timestamp);
        assert!(!result.has_coordinates);
        assert!(!result.has_type_field);
        assert_eq!(result.key_count, 0);
    }

    #[test]
    fn test_key_scan_result_debug() {
        let result = KeyScanResult {
            has_timestamp: true,
            has_coordinates: false,
            has_type_field: true,
            key_count: 5,
        };
        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("KeyScanResult"));
    }
}

// === Array Stats Structure Tests ===

mod array_stats_struct_tests {
    use super::*;

    #[test]
    fn test_array_stats_clone() {
        let stats1 = ArrayStats {
            min: 1.0,
            max: 10.0,
            sum: 55.0,
            count: 10,
        };
        let stats2 = stats1.clone();
        assert_eq!(stats1.min, stats2.min);
        assert_eq!(stats1.max, stats2.max);
        assert_eq!(stats1.sum, stats2.sum);
        assert_eq!(stats1.count, stats2.count);
    }

    #[test]
    fn test_array_stats_debug() {
        let stats = ArrayStats {
            min: 1.0,
            max: 10.0,
            sum: 55.0,
            count: 10,
        };
        let debug_str = format!("{:?}", stats);
        assert!(debug_str.contains("ArrayStats"));
    }
}

// === Edge Cases and Boundary Tests ===

mod edge_cases_tests {
    use super::*;
    use sonic_rs::JsonContainerTrait;

    #[test]
    fn test_numeric_array_boundary_2_elements() {
        let json = "[1, 2]";
        let value: sonic_rs::Value = sonic_rs::from_str(json).unwrap();
        if let Some(arr) = value.as_array() {
            assert!(!SimdClassifier::is_numeric_array(arr)); // Exactly 2, needs >= 3
        }
    }

    #[test]
    fn test_string_length_boundary_31_elements() {
        let strings: Vec<String> = (0..31).map(|i| format!(r#""s{}""#, i)).collect();
        let json = format!("[{}]", strings.join(","));
        let value: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
        if let Some(arr) = value.as_array() {
            let total_len = SimdClassifier::calculate_total_string_length(arr);
            assert!(total_len > 0);
        }
    }

    #[test]
    fn test_object_key_scan_boundary_15_keys() {
        let mut fields = Vec::new();
        for i in 0..15 {
            fields.push(format!(r#""field{}": {}"#, i, i));
        }
        let json = format!("{{{}}}", fields.join(","));
        let value: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
        if let Some(obj) = value.as_object() {
            let result = SimdClassifier::scan_object_keys(obj);
            assert_eq!(result.key_count, 15);
        }
    }

    #[test]
    fn test_sum_boundary_63_elements() {
        let json = format!(
            "[{}]",
            (0..63).map(|i| i.to_string()).collect::<Vec<_>>().join(",")
        );
        let value: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
        if let Some(arr) = value.as_array() {
            let sum = SimdNumericOps::fast_array_sum(arr).unwrap();
            assert_eq!(sum, 1953.0); // Sum of 0..63
        }
    }

    #[test]
    fn test_very_large_string_array() {
        let strings: Vec<String> = (0..200).map(|i| format!(r#""item{}""#, i)).collect();
        let json = format!("[{}]", strings.join(","));
        let value: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
        if let Some(arr) = value.as_array() {
            let total_len = SimdClassifier::calculate_total_string_length(arr);
            assert!(total_len > 800); // "item0" = 5, "item99" = 6, etc.
        }
    }

    #[test]
    fn test_very_large_numeric_sum() {
        let json = format!(
            "[{}]",
            (0..1000)
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );
        let value: sonic_rs::Value = sonic_rs::from_str(&json).unwrap();
        if let Some(arr) = value.as_array() {
            let sum = SimdNumericOps::fast_array_sum(arr).unwrap();
            assert_eq!(sum, 499500.0); // Sum of 0..1000
        }
    }
}
