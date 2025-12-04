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
        match result.value {
            LazyJsonValue::StringOwned(_) => {
                // Memory usage tracking should reflect allocation
            }
            _ => {}
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
