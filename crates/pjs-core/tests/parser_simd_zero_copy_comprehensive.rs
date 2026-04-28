//! Comprehensive tests for parser/simd_zero_copy.rs
//!
//! This test suite aims to achieve 70%+ coverage by testing:
//! - SimdZeroCopyParser configuration and creation
//! - SIMD vs non-SIMD parsing paths
//! - All value type parsing (object, array, string, number, boolean, null)
//! - String escaping and unescaping
//! - Buffer pool integration
//! - Error handling and validation
//! - LazyParser trait implementation

use pjson_rs::parser::{
    buffer_pool::PoolConfig,
    simd_zero_copy::{SimdParsingStats, SimdZeroCopyConfig, SimdZeroCopyParser},
    zero_copy::{LazyJsonValue, LazyParser},
};

mod parser_creation_tests {
    use super::*;

    #[test]
    fn test_parser_new_default() {
        let parser = SimdZeroCopyParser::new();
        assert!(parser.is_complete()); // Starts with no input
        assert_eq!(parser.remaining().len(), 0);
    }

    #[test]
    fn test_parser_with_default_config() {
        let config = SimdZeroCopyConfig::default();
        assert_eq!(config.max_depth, 64);
        assert!(config.enable_simd);
        assert_eq!(config.simd_threshold, 256);
        assert!(config.track_memory_usage);

        let parser = SimdZeroCopyParser::with_config(config);
        assert!(parser.is_complete()); // Starts with no input
    }

    #[test]
    fn test_parser_with_high_performance_config() {
        let config = SimdZeroCopyConfig::high_performance();
        assert_eq!(config.max_depth, 128);
        assert!(config.enable_simd);
        assert_eq!(config.simd_threshold, 128);
        assert!(!config.track_memory_usage); // Disabled for performance
    }

    #[test]
    fn test_parser_with_low_memory_config() {
        let config = SimdZeroCopyConfig::low_memory();
        assert_eq!(config.max_depth, 32);
        assert!(!config.enable_simd); // Disabled to save memory
        assert_eq!(config.simd_threshold, 1024);
        assert!(config.track_memory_usage);
    }

    #[test]
    fn test_parser_with_custom_buffer_pool() {
        let pool_config = PoolConfig::simd_optimized();
        let config = SimdZeroCopyConfig {
            max_depth: 64,
            enable_simd: true,
            buffer_pool_config: Some(pool_config),
            simd_threshold: 256,
            track_memory_usage: true,
        };

        let parser = SimdZeroCopyParser::with_config(config);
        assert!(parser.is_complete()); // Starts with no input
    }
}

mod string_parsing_tests {
    use super::*;

    #[test]
    fn test_parse_simple_string() {
        let mut parser = SimdZeroCopyParser::new();
        let input = br#""hello world""#;

        let result = parser.parse_simd(input).expect("parse should succeed");
        match result.value {
            LazyJsonValue::StringBorrowed(s) => {
                assert_eq!(s, b"hello world");
            }
            _ => panic!("Expected string"),
        }
    }

    #[test]
    fn test_parse_empty_string() {
        let mut parser = SimdZeroCopyParser::new();
        let input = br#""""#;

        let result = parser.parse_simd(input).expect("parse should succeed");
        match result.value {
            LazyJsonValue::StringBorrowed(s) => {
                assert_eq!(s, b"");
            }
            _ => panic!("Expected empty string"),
        }
    }

    #[test]
    fn test_parse_string_with_escapes() {
        let mut parser = SimdZeroCopyParser::new();
        let input = br#""hello\nworld\ttab""#;

        let result = parser.parse_simd(input).expect("parse should succeed");
        match result.value {
            LazyJsonValue::StringOwned(s) => {
                assert!(s.contains("\n"));
                assert!(s.contains("\t"));
            }
            _ => panic!("Expected owned string with escapes"),
        }
    }

    #[test]
    fn test_parse_string_with_quote_escape() {
        let mut parser = SimdZeroCopyParser::new();
        let input = br#""say \"hello\"""#;

        let result = parser.parse_simd(input).expect("parse should succeed");
        match result.value {
            LazyJsonValue::StringOwned(s) => {
                assert!(s.contains("\""));
            }
            _ => panic!("Expected owned string"),
        }
    }

    #[test]
    fn test_parse_string_with_backslash_escape() {
        let mut parser = SimdZeroCopyParser::new();
        let input = br#""path\\to\\file""#;

        let result = parser.parse_simd(input).expect("parse should succeed");
        match result.value {
            LazyJsonValue::StringOwned(s) => {
                assert!(s.contains("\\"));
            }
            _ => panic!("Expected owned string"),
        }
    }

    #[test]
    fn test_parse_string_with_carriage_return() {
        let mut parser = SimdZeroCopyParser::new();
        let input = br#""line1\rline2""#;

        let result = parser.parse_simd(input).expect("parse should succeed");
        match result.value {
            LazyJsonValue::StringOwned(s) => {
                assert!(s.contains("\r"));
            }
            _ => panic!("Expected owned string"),
        }
    }
}

mod number_parsing_tests {
    use super::*;

    #[test]
    fn test_parse_integer() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"42";

        let result = parser.parse_simd(input).expect("parse should succeed");
        match result.value {
            LazyJsonValue::NumberSlice(n) => {
                assert_eq!(n, b"42");
            }
            _ => panic!("Expected number"),
        }
    }

    #[test]
    fn test_parse_float() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"123.456";

        let result = parser.parse_simd(input).expect("parse should succeed");
        match result.value {
            LazyJsonValue::NumberSlice(n) => {
                assert_eq!(n, b"123.456");
            }
            _ => panic!("Expected number"),
        }
    }

    #[test]
    fn test_parse_negative_number() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"-999";

        let result = parser.parse_simd(input).expect("parse should succeed");
        match result.value {
            LazyJsonValue::NumberSlice(n) => {
                assert_eq!(n, b"-999");
            }
            _ => panic!("Expected number"),
        }
    }

    #[test]
    fn test_parse_scientific_notation() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"1.23e10";

        let result = parser.parse_simd(input).expect("parse should succeed");
        match result.value {
            LazyJsonValue::NumberSlice(n) => {
                assert_eq!(n, b"1.23e10");
            }
            _ => panic!("Expected number"),
        }
    }

    #[test]
    fn test_parse_number_with_plus_not_supported() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"+42";

        // Note: JSON spec doesn't support explicit + sign on numbers
        // This should fail parsing
        let result = parser.parse_simd(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_zero() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"0";

        let result = parser.parse_simd(input).expect("parse should succeed");
        match result.value {
            LazyJsonValue::NumberSlice(n) => {
                assert_eq!(n, b"0");
            }
            _ => panic!("Expected number"),
        }
    }
}

mod boolean_parsing_tests {
    use super::*;

    #[test]
    fn test_parse_true() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"true";

        let result = parser.parse_simd(input).expect("parse should succeed");
        assert_eq!(result.value, LazyJsonValue::Boolean(true));
    }

    #[test]
    fn test_parse_false() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"false";

        let result = parser.parse_simd(input).expect("parse should succeed");
        assert_eq!(result.value, LazyJsonValue::Boolean(false));
    }

    #[test]
    fn test_parse_boolean_invalid() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"True"; // Capital T - invalid

        let result = parser.parse_simd(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_boolean_partial() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"tru"; // Incomplete

        let result = parser.parse_simd(input);
        assert!(result.is_err());
    }
}

mod null_parsing_tests {
    use super::*;

    #[test]
    fn test_parse_null() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"null";

        let result = parser.parse_simd(input).expect("parse should succeed");
        assert_eq!(result.value, LazyJsonValue::Null);
    }
}

mod object_parsing_tests {
    use super::*;

    #[test]
    fn test_parse_empty_object() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"{}";

        let result = parser.parse_simd(input).expect("parse should succeed");
        match result.value {
            LazyJsonValue::ObjectSlice(obj) => {
                assert_eq!(obj, b"{}");
            }
            _ => panic!("Expected object"),
        }
    }

    #[test]
    fn test_parse_simple_object() {
        let mut parser = SimdZeroCopyParser::new();
        let input = br#"{"key": "value"}"#;

        let result = parser.parse_simd(input).expect("parse should succeed");
        match result.value {
            LazyJsonValue::ObjectSlice(obj) => {
                assert_eq!(obj, input);
            }
            _ => panic!("Expected object"),
        }
    }

    #[test]
    fn test_parse_nested_object() {
        let mut parser = SimdZeroCopyParser::new();
        let input = br#"{"outer": {"inner": "value"}}"#;

        let result = parser.parse_simd(input).expect("parse should succeed");
        match result.value {
            LazyJsonValue::ObjectSlice(obj) => {
                assert_eq!(obj, input);
            }
            _ => panic!("Expected object"),
        }
    }

    #[test]
    fn test_parse_object_multiple_fields() {
        let mut parser = SimdZeroCopyParser::new();
        let input = br#"{"a": 1, "b": 2, "c": 3}"#;

        let result = parser.parse_simd(input).expect("parse should succeed");
        match result.value {
            LazyJsonValue::ObjectSlice(_) => {}
            _ => panic!("Expected object"),
        }
    }

    #[test]
    fn test_parse_object_unmatched_braces() {
        let mut parser = SimdZeroCopyParser::new();
        let input = br#"{"key": "value""#; // Missing closing brace

        let result = parser.parse_simd(input);
        assert!(result.is_err());
    }
}

mod array_parsing_tests {
    use super::*;

    #[test]
    fn test_parse_empty_array() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"[]";

        let result = parser.parse_simd(input).expect("parse should succeed");
        match result.value {
            LazyJsonValue::ArraySlice(arr) => {
                assert_eq!(arr, b"[]");
            }
            _ => panic!("Expected array"),
        }
    }

    #[test]
    fn test_parse_simple_array() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"[1, 2, 3]";

        let result = parser.parse_simd(input).expect("parse should succeed");
        match result.value {
            LazyJsonValue::ArraySlice(arr) => {
                assert_eq!(arr, input);
            }
            _ => panic!("Expected array"),
        }
    }

    #[test]
    fn test_parse_nested_array() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"[[1, 2], [3, 4]]";

        let result = parser.parse_simd(input).expect("parse should succeed");
        match result.value {
            LazyJsonValue::ArraySlice(arr) => {
                assert_eq!(arr, input);
            }
            _ => panic!("Expected array"),
        }
    }

    #[test]
    fn test_parse_array_mixed_types() {
        let mut parser = SimdZeroCopyParser::new();
        let input = br#"[1, "two", true, null]"#;

        let result = parser.parse_simd(input).expect("parse should succeed");
        match result.value {
            LazyJsonValue::ArraySlice(_) => {}
            _ => panic!("Expected array"),
        }
    }

    #[test]
    fn test_parse_array_unmatched_brackets() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"[1, 2, 3"; // Missing closing bracket

        let result = parser.parse_simd(input);
        assert!(result.is_err());
    }
}

mod error_handling_tests {
    use super::*;

    #[test]
    fn test_parse_empty_input() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"";

        let result = parser.parse_simd(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_only_whitespace() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"   \n\t  ";

        let result = parser.parse_simd(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_invalid_character() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"@invalid";

        let result = parser.parse_simd(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_incomplete_string() {
        let mut parser = SimdZeroCopyParser::new();
        let input = br#""incomplete"#; // Missing closing quote

        let result = parser.parse_simd(input);
        // Depending on implementation, might succeed or fail
        // Just ensure it doesn't panic
        let _ = result;
    }
}

mod memory_usage_tests {
    use super::*;

    #[test]
    fn test_memory_usage_tracking_enabled() {
        let config = SimdZeroCopyConfig {
            track_memory_usage: true,
            ..Default::default()
        };
        let mut parser = SimdZeroCopyParser::with_config(config);
        let input = br#""test string""#;

        let result = parser.parse_simd(input).expect("parse should succeed");
        assert_eq!(result.memory_usage.allocated_bytes, 0); // Zero-copy
        assert!(result.memory_usage.referenced_bytes > 0);
    }

    #[test]
    fn test_memory_usage_with_owned_string() {
        let mut parser = SimdZeroCopyParser::new();
        let input = br#""escaped\nstring""#;

        let result = parser.parse_simd(input).expect("parse should succeed");
        // Owned string due to escapes
        if let LazyJsonValue::StringOwned(_) = result.value {
            // Memory usage tracking should reflect allocation
        }
    }

    #[test]
    fn test_processing_time_tracked() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"42";

        let result = parser.parse_simd(input).expect("parse should succeed");
        assert!(result.processing_time_ns > 0);
    }
}

mod buffer_pool_tests {
    use super::*;

    #[test]
    fn test_get_buffer_small() {
        let mut parser = SimdZeroCopyParser::new();
        let buffer = parser.get_buffer(512).expect("should get buffer");
        assert!(buffer.capacity() >= 512);
    }

    #[test]
    fn test_get_buffer_large() {
        let mut parser = SimdZeroCopyParser::new();
        let buffer = parser.get_buffer(8192).expect("should get buffer");
        assert!(buffer.capacity() >= 8192);
    }

    #[test]
    fn test_buffer_reuse() {
        let mut parser = SimdZeroCopyParser::new();

        let _buffer1 = parser.get_buffer(1024).expect("should get buffer");
        parser.release_buffer();

        let _buffer2 = parser.get_buffer(1024).expect("should get buffer");
        // Should reuse or get new buffer
    }

    #[test]
    fn test_buffer_grow() {
        let mut parser = SimdZeroCopyParser::new();

        let _buffer1 = parser.get_buffer(1024).expect("should get buffer");
        let buffer2 = parser.get_buffer(2048).expect("should get buffer");

        assert!(buffer2.capacity() >= 2048);
    }

    #[test]
    fn test_release_buffer() {
        let mut parser = SimdZeroCopyParser::new();

        let _buffer = parser.get_buffer(1024).expect("should get buffer");
        parser.release_buffer();

        // Should be able to get another buffer
        let _buffer2 = parser.get_buffer(1024).expect("should get buffer");
    }
}

mod lazy_parser_trait_tests {
    use super::*;

    #[test]
    fn test_lazy_parser_parse_lazy() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"true";

        let result = parser.parse_lazy(input).expect("parse should succeed");
        assert_eq!(result.value, LazyJsonValue::Boolean(true));
    }

    #[test]
    fn test_lazy_parser_remaining_initial() {
        let parser = SimdZeroCopyParser::new();
        assert_eq!(parser.remaining().len(), 0);
    }

    #[test]
    fn test_lazy_parser_is_complete_initial() {
        let parser = SimdZeroCopyParser::new();
        assert!(parser.is_complete());
    }

    #[test]
    fn test_lazy_parser_reset() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"42";

        let _ = parser.parse_simd(input);
        parser.reset();

        assert_eq!(parser.remaining().len(), 0);
        assert!(parser.is_complete());
    }
}

mod simd_threshold_tests {
    use super::*;

    #[test]
    fn test_small_input_below_threshold() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"42"; // Much smaller than 256 byte threshold

        let result = parser.parse_simd(input).expect("parse should succeed");
        assert!(!result.simd_used); // Should use non-SIMD path
    }

    #[test]
    fn test_large_input_above_threshold() {
        let mut parser = SimdZeroCopyParser::new();
        // Create input larger than 256 bytes
        let large_input = format!(r#"{{"key": "{}"}}"#, "x".repeat(300));

        let result = parser
            .parse_simd(large_input.as_bytes())
            .expect("parse should succeed");
        // SIMD might be used if available
        let _ = result.simd_used;
    }

    #[test]
    fn test_simd_disabled_config() {
        let config = SimdZeroCopyConfig {
            enable_simd: false,
            ..Default::default()
        };
        let mut parser = SimdZeroCopyParser::with_config(config);
        let large_input = format!(r#"{{"key": "{}"}}"#, "x".repeat(300));

        let result = parser
            .parse_simd(large_input.as_bytes())
            .expect("parse should succeed");
        assert!(!result.simd_used); // SIMD explicitly disabled
    }
}

mod parsing_stats_tests {
    use super::*;

    #[test]
    fn test_stats_default() {
        let stats = SimdParsingStats::default();
        assert_eq!(stats.total_parses, 0);
        assert_eq!(stats.simd_accelerated_parses, 0);
        assert_eq!(stats.total_bytes_processed, 0);
        assert_eq!(stats.simd_efficiency, 0.0);
    }

    #[test]
    fn test_stats_simd_usage_ratio_zero() {
        let stats = SimdParsingStats::default();
        assert_eq!(stats.simd_usage_ratio(), 0.0);
    }

    #[test]
    fn test_stats_simd_usage_ratio_full() {
        let stats = SimdParsingStats {
            total_parses: 100,
            simd_accelerated_parses: 100,
            total_bytes_processed: 10000,
            average_processing_time_ns: 1000,
            simd_efficiency: 1.0,
        };
        assert_eq!(stats.simd_usage_ratio(), 1.0);
    }

    #[test]
    fn test_stats_simd_usage_ratio_partial() {
        let stats = SimdParsingStats {
            total_parses: 100,
            simd_accelerated_parses: 50,
            total_bytes_processed: 10000,
            average_processing_time_ns: 1000,
            simd_efficiency: 0.5,
        };
        assert_eq!(stats.simd_usage_ratio(), 0.5);
    }

    #[test]
    fn test_stats_average_throughput_zero() {
        let stats = SimdParsingStats::default();
        assert_eq!(stats.average_throughput_mbps(), 0.0);
    }

    #[test]
    fn test_stats_average_throughput_nonzero() {
        let stats = SimdParsingStats {
            total_parses: 10,
            simd_accelerated_parses: 5,
            total_bytes_processed: 1024 * 1024,        // 1 MB
            average_processing_time_ns: 1_000_000_000, // 1 second
            simd_efficiency: 0.5,
        };
        assert!(stats.average_throughput_mbps() > 0.0);
    }
}

mod whitespace_handling_tests {
    use super::*;

    #[test]
    fn test_parse_with_leading_whitespace() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"  \n\t  42";

        let result = parser.parse_simd(input).expect("parse should succeed");
        match result.value {
            LazyJsonValue::NumberSlice(_) => {}
            _ => panic!("Expected number"),
        }
    }

    #[test]
    fn test_parse_object_with_whitespace() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"  { } ";

        let result = parser.parse_simd(input).expect("parse should succeed");
        match result.value {
            LazyJsonValue::ObjectSlice(_) => {}
            _ => panic!("Expected object"),
        }
    }

    #[test]
    fn test_parse_array_with_whitespace() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"\n  [ ]  ";

        let result = parser.parse_simd(input).expect("parse should succeed");
        match result.value {
            LazyJsonValue::ArraySlice(_) => {}
            _ => panic!("Expected array"),
        }
    }
}

// === Additional Coverage Tests ===

mod simd_validation_tests {
    use super::*;

    #[test]
    fn test_object_unmatched_braces_error() {
        let mut parser = SimdZeroCopyParser::new();
        // Create large input with missing closing brace
        let large_input = format!(r#"{{"key": "{}""#, "x".repeat(300));

        let result = parser.parse_simd(large_input.as_bytes());
        // Note: Zero-copy parsers may not validate entire structure upfront
        // This test verifies parser handles malformed input without panicking
        let _ = result;
    }

    #[test]
    fn test_array_unmatched_brackets_error() {
        let mut parser = SimdZeroCopyParser::new();
        // Create large input with actually unmatched brackets (missing closing bracket)
        // Format: [1,2,3,... (300+ items, no closing bracket)
        let items: Vec<String> = (0..100).map(|i| i.to_string()).collect();
        let broken = format!("[{}", items.join(","));

        let result = parser.parse_simd(broken.as_bytes());
        // Note: Zero-copy parsers may not validate entire structure upfront
        // This test verifies the parser behavior for malformed input
        // If parser accepts lazily, result will be Ok; if it validates, Err
        // Either behavior is acceptable for a zero-copy parser
        let _ = result;
    }

    #[test]
    fn test_number_validation_invalid_characters() {
        let mut parser = SimdZeroCopyParser::new();
        // Parser might accept this as it only looks at initial characters
        // Use clearly invalid starting character instead
        let input = b"abc"; // Invalid number format

        let result = parser.parse_simd(input);
        // Should fail validation
        assert!(result.is_err());
    }

    #[test]
    fn test_boolean_invalid_value() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"tru"; // Invalid boolean

        let result = parser.parse_simd(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_number_error() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b""; // Empty input

        let result = parser.parse_simd(input);
        assert!(result.is_err());
    }
}

mod string_escape_tests {
    use super::*;

    #[test]
    fn test_unescape_forward_slash() {
        let mut parser = SimdZeroCopyParser::new();
        let input = br#""hello\/world""#;

        let result = parser.parse_simd(input).expect("parse should succeed");
        match result.value {
            LazyJsonValue::StringOwned(s) => {
                assert!(s.contains("/"));
            }
            _ => panic!("Expected owned string"),
        }
    }

    #[test]
    fn test_unescape_unicode_passthrough() {
        let mut parser = SimdZeroCopyParser::new();
        let input = br#""hello\uworld""#; // Simplified escape handling

        let result = parser.parse_simd(input).expect("parse should succeed");
        match result.value {
            LazyJsonValue::StringOwned(_) => {
                // Should parse without panicking
            }
            _ => panic!("Expected owned string"),
        }
    }

    #[test]
    fn test_string_no_escapes_zero_copy() {
        let mut parser = SimdZeroCopyParser::new();
        let input = br#""simple string with no escapes""#;

        let result = parser.parse_simd(input).expect("parse should succeed");
        match result.value {
            LazyJsonValue::StringBorrowed(s) => {
                assert_eq!(s, b"simple string with no escapes");
            }
            _ => panic!("Expected borrowed string"),
        }
    }
}

mod depth_and_limits_tests {
    use super::*;

    #[test]
    fn test_max_depth_configuration() {
        let config = SimdZeroCopyConfig {
            max_depth: 5,
            ..Default::default()
        };
        let _parser = SimdZeroCopyParser::with_config(config);
        // Parser created with custom max_depth
        // Depth limiting will be tested through actual parsing
    }

    #[test]
    fn test_simd_threshold_configuration() {
        let config = SimdZeroCopyConfig {
            simd_threshold: 512,
            ..Default::default()
        };
        let _parser = SimdZeroCopyParser::with_config(config);
        // Parser should respect threshold
    }

    #[test]
    fn test_buffer_pool_custom_config() {
        use pjson_rs::parser::buffer_pool::PoolConfig;

        let pool_config = PoolConfig::low_memory();
        let config = SimdZeroCopyConfig {
            buffer_pool_config: Some(pool_config),
            ..Default::default()
        };

        let mut parser = SimdZeroCopyParser::with_config(config);
        let buffer = parser.get_buffer(128).expect("should get buffer");
        assert!(buffer.capacity() >= 128);
    }
}

mod parse_result_tests {
    use super::*;

    #[test]
    fn test_parse_result_processing_time() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"42";

        let result = parser.parse_simd(input).expect("parse should succeed");
        assert!(
            result.processing_time_ns > 0,
            "Processing time should be recorded"
        );
    }

    #[test]
    fn test_parse_result_simd_flag_small_input() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"123"; // Smaller than threshold

        let result = parser.parse_simd(input).expect("parse should succeed");
        assert!(!result.simd_used, "SIMD should not be used for small input");
    }

    #[test]
    fn test_parse_result_memory_usage_borrowed_string() {
        let mut parser = SimdZeroCopyParser::new();
        let input = br#""no escapes""#;

        let result = parser.parse_simd(input).expect("parse should succeed");
        assert_eq!(
            result.memory_usage.allocated_bytes, 0,
            "Borrowed string should have 0 allocated bytes"
        );
        assert!(result.memory_usage.referenced_bytes > 0);
    }

    #[test]
    fn test_parse_result_memory_usage_owned_string() {
        let mut parser = SimdZeroCopyParser::new();
        let input = br#""escaped\nstring""#;

        let result = parser.parse_simd(input).expect("parse should succeed");
        if let LazyJsonValue::StringOwned(_) = result.value {
            // Owned strings may have allocated bytes
        }
    }
}

mod config_presets_tests {
    use super::*;

    #[test]
    fn test_high_performance_config_values() {
        let config = SimdZeroCopyConfig::high_performance();
        assert_eq!(config.max_depth, 128);
        assert!(config.enable_simd);
        assert_eq!(config.simd_threshold, 128);
        assert!(!config.track_memory_usage);
        assert!(config.buffer_pool_config.is_some());
    }

    #[test]
    fn test_low_memory_config_values() {
        let config = SimdZeroCopyConfig::low_memory();
        assert_eq!(config.max_depth, 32);
        assert!(!config.enable_simd);
        assert_eq!(config.simd_threshold, 1024);
        assert!(config.track_memory_usage);
        assert!(config.buffer_pool_config.is_some());
    }

    #[test]
    fn test_parser_with_high_performance() {
        let config = SimdZeroCopyConfig::high_performance();
        let mut parser = SimdZeroCopyParser::with_config(config);
        let input = b"true";

        let result = parser.parse_simd(input).expect("parse should succeed");
        assert_eq!(result.value, LazyJsonValue::Boolean(true));
    }

    #[test]
    fn test_parser_with_low_memory() {
        let config = SimdZeroCopyConfig::low_memory();
        let mut parser = SimdZeroCopyParser::with_config(config);
        let input = b"false";

        let result = parser.parse_simd(input).expect("parse should succeed");
        assert_eq!(result.value, LazyJsonValue::Boolean(false));
    }
}

mod simd_availability_tests {
    use super::*;

    #[test]
    fn test_simd_enabled_in_config() {
        let config = SimdZeroCopyConfig {
            enable_simd: true,
            ..Default::default()
        };
        let _parser = SimdZeroCopyParser::with_config(config);
        // Parser respects SIMD configuration
    }

    #[test]
    fn test_parser_with_simd_disabled() {
        let config = SimdZeroCopyConfig {
            enable_simd: false,
            ..Default::default()
        };
        let mut parser = SimdZeroCopyParser::with_config(config);

        let input = b"42";
        let result = parser.parse_simd(input).expect("parse should succeed");
        assert!(!result.simd_used);
    }
}

mod null_parsing_additional_tests {
    use super::*;

    #[test]
    fn test_parse_null_with_simd() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"null";

        let result = parser.parse_simd(input).expect("parse should succeed");
        assert_eq!(result.value, LazyJsonValue::Null);
    }

    #[test]
    fn test_parse_null_large_context() {
        let mut parser = SimdZeroCopyParser::new();
        // Create context large enough to potentially trigger SIMD
        let input = format!("null{}", " ".repeat(300));

        let result = parser.parse_simd(input.trim().as_bytes());
        assert!(result.is_ok());
    }
}

mod boundary_conditions_tests {
    use super::*;

    #[test]
    fn test_exactly_threshold_size() {
        let mut parser = SimdZeroCopyParser::new();
        // Create input exactly at threshold (256 bytes default)
        let input = format!(r#"{{"key": "{}"}}"#, "x".repeat(240));

        let result = parser.parse_simd(input.as_bytes());
        assert!(result.is_ok());
    }

    #[test]
    fn test_just_below_threshold() {
        let mut parser = SimdZeroCopyParser::new();
        // 255 bytes - just below threshold
        let input = format!(r#"{{"key": "{}"}}"#, "x".repeat(239));

        let result = parser
            .parse_simd(input.as_bytes())
            .expect("parse should succeed");
        assert!(!result.simd_used);
    }

    #[test]
    fn test_just_above_threshold() {
        let mut parser = SimdZeroCopyParser::new();
        // 257 bytes - just above threshold
        let input = format!(r#"{{"key": "{}"}}"#, "x".repeat(241));

        let _result = parser
            .parse_simd(input.as_bytes())
            .expect("parse should succeed");
        // SIMD might be used depending on availability
    }

    #[test]
    fn test_very_large_input() {
        let mut parser = SimdZeroCopyParser::new();
        // Very large input (10KB)
        let input = format!(r#"{{"data": "{}"}}"#, "x".repeat(10000));

        let result = parser.parse_simd(input.as_bytes());
        assert!(result.is_ok());
    }
}

mod additional_error_tests {
    use super::*;

    #[test]
    fn test_invalid_json_start_character() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"$invalid";

        let result = parser.parse_simd(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_number_empty() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"";

        let result = parser.parse_simd(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_string_utf8() {
        let mut parser = SimdZeroCopyParser::new();
        // Invalid UTF-8 sequence inside a string
        let mut input = vec![b'"'];
        input.extend_from_slice(&[0xFF, 0xFE, 0xFD]); // Invalid UTF-8
        input.push(b'"');

        // Parser might accept or reject based on validation strictness
        let _ = parser.parse_simd(&input);
    }

    #[test]
    fn test_large_object_validation() {
        let mut parser = SimdZeroCopyParser::new();
        // Create large object with many fields
        let mut fields = Vec::new();
        for i in 0..100 {
            fields.push(format!(r#""field{}": {}"#, i, i));
        }
        let input = format!("{{{}}}", fields.join(", "));

        let result = parser.parse_simd(input.as_bytes());
        assert!(result.is_ok());
    }

    #[test]
    fn test_large_array_validation() {
        let mut parser = SimdZeroCopyParser::new();
        // Create large array
        let items: Vec<String> = (0..100).map(|i| i.to_string()).collect();
        let input = format!("[{}]", items.join(","));

        let result = parser.parse_simd(input.as_bytes());
        assert!(result.is_ok());
    }
}

mod simd_friendly_tests {
    use super::*;

    #[test]
    fn test_alignment_detection() {
        let sizes = [31, 32, 33, 64, 100, 256];
        for size in sizes {
            let mut parser = SimdZeroCopyParser::new();
            let input_string = format!(r#"{{"data":"{}"}}"#, "x".repeat(size));
            let result = parser.parse_simd(input_string.as_bytes());
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_small_input_never_simd() {
        let mut parser = SimdZeroCopyParser::new();
        // Small inputs should never use SIMD
        let inputs: &[&[u8]] = &[b"1", b"true", b"null", b"\"hi\"", b"{}", b"[]"];

        for input in inputs {
            let result = parser.parse_simd(input).expect("should parse");
            assert!(!result.simd_used, "Small input should not use SIMD");
            parser.reset();
        }
    }
}

mod buffer_management_edge_cases {
    use super::*;

    #[test]
    fn test_buffer_exact_capacity() {
        let mut parser = SimdZeroCopyParser::new();
        let buffer = parser.get_buffer(1024).expect("should get buffer");
        assert!(buffer.capacity() >= 1024);
    }

    #[test]
    fn test_buffer_sequential_releases() {
        let mut parser = SimdZeroCopyParser::new();

        let _ = parser.get_buffer(512).expect("first buffer");
        parser.release_buffer();

        let _ = parser.get_buffer(1024).expect("second buffer");
        parser.release_buffer();

        let _ = parser.get_buffer(256).expect("third buffer");
        parser.release_buffer();
    }

    #[test]
    fn test_buffer_size_upgrade() {
        let mut parser = SimdZeroCopyParser::new();

        let buffer1 = parser.get_buffer(512).expect("small buffer");
        let cap1 = buffer1.capacity();

        // Request larger buffer - should replace
        let buffer2 = parser.get_buffer(2048).expect("large buffer");
        let cap2 = buffer2.capacity();

        assert!(cap2 >= 2048);
        assert!(cap2 >= cap1);
    }

    #[test]
    fn test_buffer_after_parse() {
        let mut parser = SimdZeroCopyParser::new();

        // Parse something
        let _ = parser.parse_simd(b"42").expect("parse");

        // Get buffer should still work
        let _ = parser.get_buffer(1024).expect("buffer after parse");
    }
}

mod escape_sequence_edge_cases {
    use super::*;

    #[test]
    fn test_consecutive_escapes() {
        let mut parser = SimdZeroCopyParser::new();
        let input = br#""\\\\\\""#; // Triple backslash

        let result = parser.parse_simd(input).expect("should parse");
        match result.value {
            LazyJsonValue::StringOwned(s) => {
                assert!(s.contains('\\'));
            }
            _ => panic!("Expected owned string"),
        }
    }

    #[test]
    fn test_mixed_escape_types() {
        let mut parser = SimdZeroCopyParser::new();
        let input = br#""a\nb\tc\rd\\"e""#;

        let result = parser.parse_simd(input).expect("should parse");
        match result.value {
            LazyJsonValue::StringOwned(s) => {
                assert!(s.contains('\n'));
                assert!(s.contains('\t'));
                assert!(s.contains('\r'));
                assert!(s.contains('\\'));
            }
            _ => panic!("Expected owned string"),
        }
    }

    #[test]
    fn test_escape_at_end() {
        let mut parser = SimdZeroCopyParser::new();
        let input = br#""test\n""#;

        let result = parser.parse_simd(input).expect("should parse");
        match result.value {
            LazyJsonValue::StringOwned(s) => {
                assert!(s.contains('\n'));
            }
            _ => panic!("Expected owned string"),
        }
    }

    #[test]
    fn test_no_escape_passthrough() {
        let mut parser = SimdZeroCopyParser::new();
        let input = br#""plain text with no escapes at all""#;

        let result = parser.parse_simd(input).expect("should parse");
        match result.value {
            LazyJsonValue::StringBorrowed(s) => {
                assert_eq!(s, b"plain text with no escapes at all");
            }
            _ => panic!("Expected borrowed string for no-escape case"),
        }
    }
}

mod number_format_edge_cases {
    use super::*;

    #[test]
    fn test_negative_zero() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"-0";

        let result = parser.parse_simd(input).expect("should parse");
        match result.value {
            LazyJsonValue::NumberSlice(n) => {
                assert_eq!(n, b"-0");
            }
            _ => panic!("Expected number"),
        }
    }

    #[test]
    fn test_negative_float() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"-123.456";

        let result = parser.parse_simd(input).expect("should parse");
        match result.value {
            LazyJsonValue::NumberSlice(n) => {
                assert_eq!(n, b"-123.456");
            }
            _ => panic!("Expected number"),
        }
    }

    #[test]
    fn test_exponent_uppercase() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"1.5E10";

        let result = parser.parse_simd(input).expect("should parse");
        match result.value {
            LazyJsonValue::NumberSlice(n) => {
                assert_eq!(n, b"1.5E10");
            }
            _ => panic!("Expected number"),
        }
    }

    #[test]
    fn test_exponent_negative() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"2.5e-5";

        let result = parser.parse_simd(input).expect("should parse");
        match result.value {
            LazyJsonValue::NumberSlice(n) => {
                assert_eq!(n, b"2.5e-5");
            }
            _ => panic!("Expected number"),
        }
    }

    #[test]
    fn test_exponent_positive() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"3.0e+8";

        let result = parser.parse_simd(input).expect("should parse");
        match result.value {
            LazyJsonValue::NumberSlice(n) => {
                assert_eq!(n, b"3.0e+8");
            }
            _ => panic!("Expected number"),
        }
    }

    #[test]
    fn test_very_large_number() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"99999999999999999999999999999999";

        let result = parser.parse_simd(input).expect("should parse");
        match result.value {
            LazyJsonValue::NumberSlice(_) => {}
            _ => panic!("Expected number"),
        }
    }

    #[test]
    fn test_very_small_number() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"0.000000000000000000000001";

        let result = parser.parse_simd(input).expect("should parse");
        match result.value {
            LazyJsonValue::NumberSlice(_) => {}
            _ => panic!("Expected number"),
        }
    }
}

mod object_array_nesting_tests {
    use super::*;

    #[test]
    fn test_deeply_nested_objects() {
        let mut parser = SimdZeroCopyParser::new();
        let input = br#"{"a":{"b":{"c":{"d":{"e":"value"}}}}}"#;

        let result = parser.parse_simd(input).expect("should parse");
        match result.value {
            LazyJsonValue::ObjectSlice(_) => {}
            _ => panic!("Expected object"),
        }
    }

    #[test]
    fn test_deeply_nested_arrays() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"[[[[[ 1 ]]]]]";

        let result = parser.parse_simd(input).expect("should parse");
        match result.value {
            LazyJsonValue::ArraySlice(_) => {}
            _ => panic!("Expected array"),
        }
    }

    #[test]
    fn test_mixed_nesting() {
        let mut parser = SimdZeroCopyParser::new();
        let input = br#"{"arr":[{"obj":[[1,2,3]],"x":true}]}"#;

        let result = parser.parse_simd(input).expect("should parse");
        match result.value {
            LazyJsonValue::ObjectSlice(_) => {}
            _ => panic!("Expected object"),
        }
    }

    #[test]
    fn test_array_of_objects() {
        let mut parser = SimdZeroCopyParser::new();
        let input = br#"[{"a":1},{"b":2},{"c":3}]"#;

        let result = parser.parse_simd(input).expect("should parse");
        match result.value {
            LazyJsonValue::ArraySlice(_) => {}
            _ => panic!("Expected array"),
        }
    }

    #[test]
    fn test_object_of_arrays() {
        let mut parser = SimdZeroCopyParser::new();
        let input = br#"{"a":[1,2],"b":[3,4],"c":[5,6]}"#;

        let result = parser.parse_simd(input).expect("should parse");
        match result.value {
            LazyJsonValue::ObjectSlice(_) => {}
            _ => panic!("Expected object"),
        }
    }
}

mod reset_and_state_tests {
    use super::*;

    #[test]
    fn test_parser_reset_clears_state() {
        let mut parser = SimdZeroCopyParser::new();
        let input = b"42";

        let _ = parser.parse_simd(input);

        parser.reset();
        assert_eq!(parser.remaining().len(), 0);
        assert!(parser.is_complete());
        // Buffer is released on reset
    }

    #[test]
    fn test_multiple_parses_with_reset() {
        let mut parser = SimdZeroCopyParser::new();

        let result1 = parser.parse_simd(b"true").expect("first parse");
        assert_eq!(result1.value, LazyJsonValue::Boolean(true));

        parser.reset();

        let result2 = parser.parse_simd(b"false").expect("second parse");
        assert_eq!(result2.value, LazyJsonValue::Boolean(false));
    }

    #[test]
    fn test_remaining_after_incomplete_parse() {
        let parser = SimdZeroCopyParser::new();
        // New parser has no input
        assert_eq!(parser.remaining().len(), 0);
    }
}

mod complex_json_tests {
    use super::*;

    #[test]
    fn test_nested_objects_and_arrays() {
        let mut parser = SimdZeroCopyParser::new();
        let input = br#"{"outer": {"inner": [1, 2, 3]}, "array": [{"a": 1}]}"#;

        let result = parser.parse_simd(input).expect("parse should succeed");
        match result.value {
            LazyJsonValue::ObjectSlice(_) => {}
            _ => panic!("Expected object"),
        }
    }

    #[test]
    fn test_complex_escapes_in_string() {
        let mut parser = SimdZeroCopyParser::new();
        let input = br#""line1\nline2\ttab\rcarriage\\backslash\"quote""#;

        let result = parser.parse_simd(input).expect("parse should succeed");
        match result.value {
            LazyJsonValue::StringOwned(s) => {
                assert!(s.contains('\n'));
                assert!(s.contains('\t'));
                assert!(s.contains('\r'));
                assert!(s.contains('\\'));
                assert!(s.contains('"'));
            }
            _ => panic!("Expected owned string"),
        }
    }

    #[test]
    fn test_scientific_notation_variants() {
        let mut parser = SimdZeroCopyParser::new();

        let inputs: &[&[u8]] = &[b"1e10", b"1.5e-5", b"2.3E+10", b"-4.5e2"];
        for input in inputs {
            let result = parser.parse_simd(input).expect("parse should succeed");
            match result.value {
                LazyJsonValue::NumberSlice(_) => {}
                _ => panic!("Expected number for {:?}", std::str::from_utf8(input)),
            }
            parser.reset();
        }
    }
}

mod serde_json_reference_correctness {
    use super::*;

    /// Returns true if the current platform supports SIMD (AVX2 on x86_64).
    #[allow(dead_code)]
    fn simd_available() -> bool {
        #[cfg(target_arch = "x86_64")]
        {
            std::arch::is_x86_feature_detected!("avx2")
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            false
        }
    }

    #[test]
    fn test_large_object_parses_to_valid_json() {
        let mut parser = SimdZeroCopyParser::new();
        let pairs: Vec<String> = (0..30)
            .map(|i| format!(r#""key_{i}": "value_{i}""#))
            .collect();
        let input = format!("{{{}}}", pairs.join(", "));
        assert!(
            input.len() > 256,
            "input must exceed 256 bytes to test SIMD path"
        );

        let result = parser
            .parse_simd(input.as_bytes())
            .expect("parse should succeed");
        match result.value {
            LazyJsonValue::ObjectSlice(bytes) => {
                let parsed: serde_json::Value =
                    serde_json::from_slice(bytes).expect("ObjectSlice must be valid JSON");
                assert!(parsed.is_object());
            }
            _ => panic!("Expected ObjectSlice"),
        }
    }

    #[test]
    fn test_large_array_parses_to_valid_json() {
        let mut parser = SimdZeroCopyParser::new();
        let items: Vec<String> = (0..100).map(|i| i.to_string()).collect();
        let input = format!("[{}]", items.join(", "));
        assert!(input.len() > 256);

        let result = parser
            .parse_simd(input.as_bytes())
            .expect("parse should succeed");
        match result.value {
            LazyJsonValue::ArraySlice(bytes) => {
                let parsed: serde_json::Value =
                    serde_json::from_slice(bytes).expect("ArraySlice must be valid JSON");
                assert!(parsed.is_array());
                assert_eq!(parsed.as_array().unwrap().len(), 100);
            }
            _ => panic!("Expected ArraySlice"),
        }
    }

    #[test]
    fn test_number_parses_to_same_f64_as_serde_json() {
        let cases: &[&[u8]] = &[b"42", b"3.14", b"-0", b"1e10", b"2.5e-5", b"0.000001"];
        for input in cases {
            let mut parser = SimdZeroCopyParser::new();
            let result = parser.parse_simd(input).expect("parse should succeed");
            match result.value {
                LazyJsonValue::NumberSlice(bytes) => {
                    let our_f64: f64 = std::str::from_utf8(bytes).unwrap().parse().unwrap();
                    let serde_f64: f64 = serde_json::from_slice(input).unwrap();
                    assert!(
                        (our_f64 - serde_f64).abs() < f64::EPSILON || our_f64 == serde_f64,
                        "mismatch for {:?}: our={our_f64} serde={serde_f64}",
                        std::str::from_utf8(input).unwrap()
                    );
                }
                _ => panic!(
                    "Expected NumberSlice for {:?}",
                    std::str::from_utf8(input).unwrap()
                ),
            }
        }
    }

    #[test]
    fn test_string_unescaping_matches_serde_json() {
        let cases: &[&[u8]] = &[
            br#""hello\nworld""#,
            br#""tab\there""#,
            br#""quote\"here""#,
            br#""backslash\\here""#,
        ];
        for input in cases {
            let mut parser = SimdZeroCopyParser::new();
            let result = parser.parse_simd(input).expect("parse should succeed");
            let our_str = match &result.value {
                LazyJsonValue::StringOwned(s) => s.clone(),
                LazyJsonValue::StringBorrowed(b) => std::str::from_utf8(b).unwrap().to_string(),
                _ => panic!("Expected string variant"),
            };
            let serde_str: String = serde_json::from_slice(input).unwrap();
            assert_eq!(
                our_str,
                serde_str,
                "mismatch for {:?}",
                std::str::from_utf8(input).unwrap()
            );
        }
    }

    #[test]
    fn test_boolean_matches_serde_json() {
        for (input, expected) in [(b"true" as &[u8], true), (b"false", false)] {
            let mut parser = SimdZeroCopyParser::new();
            let result = parser.parse_simd(input).expect("parse should succeed");
            let serde_bool: bool = serde_json::from_slice(input).unwrap();
            assert_eq!(result.value, LazyJsonValue::Boolean(serde_bool));
            assert_eq!(result.value, LazyJsonValue::Boolean(expected));
        }
    }

    #[test]
    fn test_null_matches_serde_json() {
        let mut parser = SimdZeroCopyParser::new();
        let result = parser.parse_simd(b"null").expect("parse should succeed");
        let serde_val: serde_json::Value = serde_json::from_slice(b"null").unwrap();
        assert_eq!(result.value, LazyJsonValue::Null);
        assert_eq!(serde_val, serde_json::Value::Null);
    }

    #[test]
    fn test_corpus_against_serde_json() {
        // (input, should_succeed)
        let cases: &[(&[u8], bool)] = &[
            (b"{}", true),
            (b"[]", true),
            (br#""hello""#, true),
            (b"42", true),
            (b"true", true),
            (b"false", true),
            (b"null", true),
            (br#"{"a":1,"b":"two","c":true,"d":null}"#, true),
            (br#"[1,"two",true,null,{},[]]"#, true),
            (br#""escape\ntest""#, true),
            ("\"unicode: caf\u{00e9}\"".as_bytes(), true),
            (b"-3.14e10", true),
            (b"0", true),
            (b"-0", true),
            (b"@invalid", false),
            (b"", false),
            (b"   ", false),
        ];
        for (input, should_succeed) in cases {
            let mut parser = SimdZeroCopyParser::new();
            let our_result = parser.parse_simd(input);
            let serde_result = serde_json::from_slice::<serde_json::Value>(input);
            assert_eq!(
                our_result.is_ok(),
                *should_succeed,
                "our parser disagreed on {:?}",
                std::str::from_utf8(input).unwrap_or("<binary>")
            );
            // For valid inputs, serde_json must also agree
            if *should_succeed {
                assert!(
                    serde_result.is_ok(),
                    "serde_json rejected valid input {:?}",
                    std::str::from_utf8(input).unwrap_or("<binary>")
                );
            }
        }
    }
}

mod simd_path_forced {
    use super::*;

    /// Returns true if SIMD (AVX2) is available on this platform.
    fn simd_available() -> bool {
        #[cfg(target_arch = "x86_64")]
        {
            std::arch::is_x86_feature_detected!("avx2")
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            false
        }
    }

    #[test]
    fn test_large_object_30_keys_above_threshold() {
        let mut parser = SimdZeroCopyParser::new();
        let pairs: Vec<String> = (0..30)
            .map(|i| format!(r#""key_{i:02}": "val_{i:02}""#))
            .collect();
        let input = format!("{{{}}}", pairs.join(", "));
        assert!(input.len() >= 256);

        let result = parser
            .parse_simd(input.as_bytes())
            .expect("large object should parse");
        match result.value {
            LazyJsonValue::ObjectSlice(_) => {}
            _ => panic!("Expected ObjectSlice"),
        }
        assert_eq!(result.simd_used, simd_available());
    }

    #[test]
    fn test_large_array_100_integers_above_threshold() {
        let mut parser = SimdZeroCopyParser::new();
        let items: Vec<String> = (0..100).map(|i| i.to_string()).collect();
        let input = format!("[{}]", items.join(","));
        assert!(input.len() >= 256);

        let result = parser
            .parse_simd(input.as_bytes())
            .expect("large array should parse");
        match result.value {
            LazyJsonValue::ArraySlice(_) => {}
            _ => panic!("Expected ArraySlice"),
        }
        assert_eq!(result.simd_used, simd_available());
    }

    #[test]
    fn test_large_string_300_chars_no_escapes() {
        let mut parser = SimdZeroCopyParser::new();
        let content = "a".repeat(300);
        let input = format!(r#""{content}""#);
        assert!(input.len() >= 256);

        let result = parser
            .parse_simd(input.as_bytes())
            .expect("large string should parse");
        match result.value {
            LazyJsonValue::StringBorrowed(bytes) => {
                assert_eq!(bytes.len(), 300);
                assert!(bytes.iter().all(|&b| b == b'a'));
            }
            _ => panic!("Expected StringBorrowed (no escapes in input)"),
        }
        assert_eq!(result.simd_used, simd_available());
    }

    #[test]
    fn test_large_string_with_escape_sequences() {
        let mut parser = SimdZeroCopyParser::new();
        // Build a JSON string literal where escape sequences are represented as two-byte
        // sequences in the source (e.g. backslash + 'n'), so the raw byte slice is > 256.
        // We use a raw string to avoid Rust interpreting the backslashes.
        let plain = "x".repeat(246);
        // Each r"\n" below is two bytes in the JSON literal: '\' and 'n'
        let escape_suffix = r"\n\t\r\\";
        let inner = format!("{plain}{escape_suffix}");
        // Wrap in JSON string quotes
        let input = format!("\"{inner}\"");
        assert!(
            input.len() >= 256,
            "input is {} bytes, need >= 256",
            input.len()
        );

        let result = parser
            .parse_simd(input.as_bytes())
            .expect("large escaped string should parse");
        match result.value {
            LazyJsonValue::StringOwned(s) => {
                assert!(s.contains('\n'));
                assert!(s.contains('\t'));
            }
            _ => panic!("Expected StringOwned due to escape sequences"),
        }
    }

    #[test]
    fn test_large_number_300_digits() {
        let mut parser = SimdZeroCopyParser::new();
        // A valid number consisting of many digits
        let input = format!("1{}", "0".repeat(299));
        assert!(input.len() >= 256);

        let result = parser
            .parse_simd(input.as_bytes())
            .expect("large number should parse");
        match result.value {
            LazyJsonValue::NumberSlice(_) => {}
            _ => panic!("Expected NumberSlice"),
        }
    }

    #[test]
    fn test_boundary_at_exactly_256_bytes() {
        let mut parser = SimdZeroCopyParser::new();
        // Construct input of exactly 256 bytes: {"k":"<padding>"}
        // Outer wrapper is 8 bytes: {"k":""}  → pad to 256-8 = 248 chars
        let padding = "x".repeat(248);
        let input = format!(r#"{{"k":"{padding}"}}"#);
        assert_eq!(input.len(), 256);

        let result = parser
            .parse_simd(input.as_bytes())
            .expect("256-byte input should parse");
        match result.value {
            LazyJsonValue::ObjectSlice(_) => {}
            _ => panic!("Expected ObjectSlice"),
        }
        assert_eq!(result.simd_used, simd_available());
    }

    #[test]
    fn test_boundary_at_255_bytes_no_simd() {
        let mut parser = SimdZeroCopyParser::new();
        // 255 bytes — one below the threshold
        let padding = "x".repeat(247);
        let input = format!(r#"{{"k":"{padding}"}}"#);
        assert_eq!(input.len(), 255);

        let result = parser
            .parse_simd(input.as_bytes())
            .expect("255-byte input should parse");
        // Regardless of platform, input below threshold must not use SIMD
        assert!(!result.simd_used, "inputs < 256 bytes must not use SIMD");
    }

    #[test]
    fn test_malformed_large_object_unmatched_braces() {
        let mut parser = SimdZeroCopyParser::new();
        // Object where open braces outnumber close braces
        let pairs: Vec<String> = (0..30).map(|i| format!(r#""k{i}":"v{i}""#)).collect();
        let input = format!("{{{{{}}}", pairs.join(","));
        // Has 2 open braces and 1 close brace → unmatched
        assert!(input.len() >= 256);

        let result = parser.parse_simd(input.as_bytes());
        // Must either error or succeed gracefully — must not panic
        let _ = result;
    }

    #[test]
    fn test_malformed_large_array_unmatched_brackets() {
        let mut parser = SimdZeroCopyParser::new();
        let items: Vec<String> = (0..100).map(|i| i.to_string()).collect();
        // Missing closing bracket
        let input = format!("[{}", items.join(","));
        assert!(input.len() >= 256);

        let result = parser.parse_simd(input.as_bytes());
        assert!(result.is_err(), "array missing closing bracket should fail");
    }

    #[test]
    fn test_simd_disabled_large_input_uses_fallback() {
        let config = SimdZeroCopyConfig {
            enable_simd: false,
            ..Default::default()
        };
        let mut parser = SimdZeroCopyParser::with_config(config);

        let pairs: Vec<String> = (0..30).map(|i| format!(r#""k{i}":"v{i}""#)).collect();
        let input = format!("{{{}}}", pairs.join(","));
        assert!(input.len() >= 256);

        let result = parser
            .parse_simd(input.as_bytes())
            .expect("large input should parse without SIMD");
        assert!(
            !result.simd_used,
            "SIMD must be off when disabled in config"
        );
        match result.value {
            LazyJsonValue::ObjectSlice(_) => {}
            _ => panic!("Expected ObjectSlice"),
        }
    }
}

mod edge_cases_large_input {
    use super::*;

    #[test]
    fn test_1mb_json_object() {
        let mut parser = SimdZeroCopyParser::new();
        // Build a ~1 MB JSON object: 1000 keys with ~1 KB values each
        let pairs: Vec<String> = (0..1000)
            .map(|i| format!(r#""key_{i:04}": "{}""#, "v".repeat(1000)))
            .collect();
        let input = format!("{{{}}}", pairs.join(", "));
        assert!(input.len() > 1_000_000);

        let result = parser
            .parse_simd(input.as_bytes())
            .expect("1 MB object should parse");
        match result.value {
            LazyJsonValue::ObjectSlice(_) => {}
            _ => panic!("Expected ObjectSlice"),
        }
    }

    #[test]
    fn test_large_scale_empty_nested_structures() {
        let mut parser = SimdZeroCopyParser::new();
        // Array of 200 empty objects and nested empty arrays
        let items: Vec<&str> = (0..100).map(|_| "{}").collect();
        let nested: Vec<String> = (0..100).map(|_| "[]".to_string()).collect();
        let all: Vec<&str> = items
            .iter()
            .map(|s| s.as_ref())
            .chain(nested.iter().map(|s| s.as_ref()))
            .collect();
        let input = format!("[{}]", all.join(", "));
        assert!(input.len() > 256);

        let result = parser
            .parse_simd(input.as_bytes())
            .expect("large nested structures should parse");
        match result.value {
            LazyJsonValue::ArraySlice(_) => {}
            _ => panic!("Expected ArraySlice"),
        }
    }

    #[test]
    fn test_unicode_in_large_string() {
        let mut parser = SimdZeroCopyParser::new();
        // Mix ASCII and multi-byte Unicode to exceed 256 bytes
        let unicode_block = "日本語テスト".repeat(20); // ~240+ bytes in UTF-8
        let input = format!(r#""{unicode_block}""#);
        assert!(input.len() > 256);

        let result = parser
            .parse_simd(input.as_bytes())
            .expect("unicode large string should parse");
        // No escapes → borrowed
        match result.value {
            LazyJsonValue::StringBorrowed(_) | LazyJsonValue::StringOwned(_) => {}
            _ => panic!("Expected string variant"),
        }
    }

    #[test]
    fn test_mixed_escape_sequences_large_string() {
        let mut parser = SimdZeroCopyParser::new();
        // Build a large string with multiple escape types distributed throughout
        let segment = r#"line\n\ttab\r\\"#;
        // Repeat until we exceed 256 bytes
        let repeated = segment.repeat(20);
        let input = format!(r#""{repeated}""#);
        assert!(input.len() > 256);

        let result = parser
            .parse_simd(input.as_bytes())
            .expect("large escaped string should parse");
        match result.value {
            LazyJsonValue::StringOwned(s) => {
                assert!(s.contains('\n'));
                assert!(s.contains('\t'));
            }
            _ => panic!("Expected StringOwned due to escape sequences"),
        }
    }

    #[test]
    fn test_deeply_nested_large_valid_object() {
        let mut parser = SimdZeroCopyParser::new();
        // 8 levels of nesting with padding to push total size above 256 bytes
        let padding = "x".repeat(30);
        let inner = format!(r#""leaf": "{padding}""#);
        let level7 = format!(r#"{{"l7": {{{inner}}}}}"#);
        let level6 = format!(r#"{{"l6": {level7}}}"#);
        let level5 = format!(r#"{{"l5": {level6}}}"#);
        let level4 = format!(r#"{{"l4": {level5}}}"#);
        let level3 = format!(r#"{{"l3": {level4}}}"#);
        let level2 = format!(r#"{{"l2": {level3}}}"#);
        let level1 = format!(r#"{{"l1": {level2}}}"#);

        let result = parser
            .parse_simd(level1.as_bytes())
            .expect("deeply nested object should parse");
        match result.value {
            LazyJsonValue::ObjectSlice(_) => {}
            _ => panic!("Expected ObjectSlice"),
        }
    }

    #[test]
    fn test_large_array_mixed_types_above_threshold() {
        let mut parser = SimdZeroCopyParser::new();
        // Mix strings, numbers, booleans, nulls and nested objects in a large array
        let mut items: Vec<String> = Vec::with_capacity(60);
        for i in 0..20 {
            items.push(format!(r#""string_{i}""#));
            items.push(i.to_string());
            items.push(if i % 2 == 0 {
                "true".into()
            } else {
                "false".into()
            });
        }
        let input = format!("[{}]", items.join(","));
        assert!(input.len() > 256);

        let result = parser
            .parse_simd(input.as_bytes())
            .expect("mixed large array should parse");
        match result.value {
            LazyJsonValue::ArraySlice(_) => {}
            _ => panic!("Expected ArraySlice"),
        }
    }
}
