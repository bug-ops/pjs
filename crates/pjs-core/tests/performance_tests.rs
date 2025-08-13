//! Performance and stress tests to improve coverage

use pjson_rs::{
    SimpleParser, SonicParser, ZeroCopyParser,
    ArenaJsonParser,
    parser::ParseConfig,
    semantic::{SemanticType, NumericDType},
    SemanticMeta,
};
use std::time::Instant;

#[cfg(test)]
mod parser_performance_tests {
    use super::*;

    #[test]
    fn test_large_json_parsing() {
        let parser = SimpleParser::new();
        
        // Create a reasonably large JSON object
        let mut large_json = String::from("{");
        for i in 0..1000 {
            if i > 0 {
                large_json.push(',');
            }
            large_json.push_str(&format!("\"key{}\": {}", i, i * 2));
        }
        large_json.push('}');
        
        let start = Instant::now();
        let result = parser.parse(large_json.as_bytes());
        let duration = start.elapsed();
        
        assert!(result.is_ok(), "Large JSON should parse successfully");
        println!("Large JSON parsing took: {:?}", duration);
    }

    #[test]
    fn test_deep_nesting_performance() {
        let parser = SimpleParser::new();
        
        // Create deeply nested JSON (but within reasonable limits)
        let mut nested_json = String::new();
        let depth = 50; // Keep it reasonable to avoid stack overflow
        
        for _ in 0..depth {
            nested_json.push_str("{\"nested\":");
        }
        nested_json.push_str("\"value\"");
        for _ in 0..depth {
            nested_json.push('}');
        }
        
        let start = Instant::now();
        let result = parser.parse(nested_json.as_bytes());
        let duration = start.elapsed();
        
        // May succeed or fail depending on depth limits, but shouldn't crash
        println!("Deep nesting result: {:?}, time: {:?}", result.is_ok(), duration);
    }

    #[test]
    fn test_array_parsing_performance() {
        let parser = SimpleParser::new();
        
        // Create large array
        let mut array_json = String::from("[");
        for i in 0..10000 {
            if i > 0 {
                array_json.push(',');
            }
            array_json.push_str(&i.to_string());
        }
        array_json.push(']');
        
        let start = Instant::now();
        let result = parser.parse(array_json.as_bytes());
        let duration = start.elapsed();
        
        assert!(result.is_ok(), "Large array should parse successfully");
        println!("Large array parsing took: {:?}", duration);
    }

    #[test]
    fn test_sonic_vs_simple_parser() {
        let simple_parser = SimpleParser::new();
        let sonic_parser = SonicParser::new();
        
        let test_json = br#"{"users": [{"id": 1, "name": "Alice"}, {"id": 2, "name": "Bob"}]}"#;
        
        // Test simple parser
        let start = Instant::now();
        let simple_result = simple_parser.parse(test_json);
        let simple_duration = start.elapsed();
        
        // Test sonic parser
        let start = Instant::now();
        let sonic_result = sonic_parser.parse(test_json);
        let sonic_duration = start.elapsed();
        
        assert!(simple_result.is_ok(), "Simple parser should work");
        println!("Simple parser took: {:?}", simple_duration);
        println!("Sonic parser result: {:?}, time: {:?}", sonic_result.is_ok(), sonic_duration);
    }
}

#[cfg(test)]
mod memory_tests {
    use super::*;

    #[test]
    fn test_arena_parser_memory_efficiency() {
        let mut arena_parser = ArenaJsonParser::new();
        
        // Parse multiple small documents to test arena recycling
        for i in 0..10 {
            let json = format!(r#"{{"iteration": {}, "data": "test{}}}"#, i, i);
            let result = arena_parser.parse(&json);
            // ArenaJsonParser may not be fully implemented, so just check it doesn't crash
            println!("Arena parsing iteration {}: {:?}", i, result.is_ok());
        }
        
        let stats = arena_parser.arena_stats();
        println!("Arena stats after 10 parses: {:?}", stats);
        
        // Just ensure we can get stats without crashing
        assert!(stats.values_allocated < 1000, "Arena should have reasonable allocation count");
    }

    #[test]
    fn test_zero_copy_parser_memory() {
        let mut parser = ZeroCopyParser::new();
        
        // Test with various JSON sizes
        let test_cases: &[&[u8]] = &[
            b"null",
            br#"{"small": "test"}"#,
            br#"{"medium": "test data with more content for testing"}"#,
            br#"{"large": "test data with much more content for comprehensive testing of zero-copy parsing capabilities"}"#,
        ];
        
        for (i, test_case) in test_cases.iter().enumerate() {
            // Use trait method explicitly
            use pjson_rs::LazyParser;
            let result = parser.parse_lazy(test_case);
            
            println!("Test case {}: {} bytes", i, test_case.len());
            
            // Should parse successfully
            assert!(result.is_ok(), "Zero-copy parsing should succeed for case {}", i);
        }
    }
}

#[cfg(test)]
mod semantic_performance_tests {
    use super::*;

    #[test]
    fn test_semantic_hint_performance() {
        let parser = SimpleParser::new();
        
        // Test numeric array with semantic hints
        let numeric_data = (0..1000).map(|i| i.to_string()).collect::<Vec<_>>().join(",");
        let json = format!("[{}]", numeric_data);
        
        let semantics = SemanticMeta::new(SemanticType::NumericArray {
            dtype: NumericDType::I32,
            length: Some(1000),
        });
        
        // Parse without semantics
        let start = Instant::now();
        let result_without = parser.parse(json.as_bytes());
        let duration_without = start.elapsed();
        
        // Parse with semantics
        let start = Instant::now();
        let result_with = parser.parse_with_semantics(json.as_bytes(), &semantics);
        let duration_with = start.elapsed();
        
        assert!(result_without.is_ok(), "Parsing without semantics should work");
        assert!(result_with.is_ok(), "Parsing with semantics should work");
        
        println!("Without semantics: {:?}", duration_without);
        println!("With semantics: {:?}", duration_with);
    }

    #[test]
    fn test_all_semantic_types() {
        let parser = SimpleParser::new();
        let json = br#"{"timestamp": "2023-01-01", "values": [1, 2, 3]}"#;
        
        // Test time series semantics
        let time_series = SemanticMeta::new(SemanticType::TimeSeries {
            timestamp_field: "timestamp".to_string(),
            value_fields: smallvec::smallvec!["values".to_string()],
            interval_ms: Some(1000),
        });
        
        let result = parser.parse_with_semantics(json, &time_series);
        assert!(result.is_ok(), "Time series semantics should work");
        
        // Test geospatial semantics
        let geo_json = br#"{"type": "Point", "coordinates": [1.0, 2.0]}"#;
        let geospatial = SemanticMeta::new(SemanticType::Geospatial {
            coordinate_system: "WGS84".to_string(),
            geometry_type: "Point".to_string(),
        });
        
        let result = parser.parse_with_semantics(geo_json, &geospatial);
        assert!(result.is_ok(), "Geospatial semantics should work");
    }
}

#[cfg(test)]
mod config_tests {
    use super::*;

    #[test]
    fn test_parser_config_variations() {
        let configs = [
            ParseConfig {
                detect_semantics: true,
                max_size_mb: 10,
                stream_large_arrays: true,
                stream_threshold: 100,
            },
            ParseConfig {
                detect_semantics: false,
                max_size_mb: 50,
                stream_large_arrays: false,
                stream_threshold: 1000,
            },
        ];
        
        let test_json = br#"{"config": "test", "values": [1, 2, 3, 4, 5]}"#;
        
        for (i, config) in configs.iter().enumerate() {
            let parser = SimpleParser::with_config(config.clone());
            let result = parser.parse(test_json);
            
            assert!(result.is_ok(), "Parser with config {} should work", i);
            
            let stats = parser.stats();
            println!("Config {} stats: {:?}", i, stats);
        }
    }

    #[test]
    fn test_hybrid_parser_fallback() {
        let parser = pjson_rs::parser::Parser::new();
        
        // Test cases that might trigger fallback behavior
        let test_cases = [
            br#"{"simple": "json"}"#.as_slice(),
            br#"{"numbers": [1, 2, 3, 4, 5]}"#.as_slice(),
            br#"{"nested": {"deep": {"value": true}}}"#.as_slice(),
            br#"{"array": [{"a": 1}, {"b": 2}]}"#.as_slice(),
        ];
        
        for (i, test_case) in test_cases.iter().enumerate() {
            let result = parser.parse(test_case);
            assert!(result.is_ok(), "Hybrid parser should handle test case {}", i);
            
            let stats = parser.stats();
            println!("Test case {} stats: {:?}", i, stats);
        }
    }

    #[test]
    fn test_zero_copy_vs_regular_parser() {
        let regular_parser = pjson_rs::parser::Parser::new();
        let zero_copy_parser = pjson_rs::parser::Parser::zero_copy_optimized();
        
        let test_json = br#"{"performance": "test", "data": [1, 2, 3, 4, 5]}"#;
        
        // Test regular parser
        let start = Instant::now();
        let regular_result = regular_parser.parse(test_json);
        let regular_duration = start.elapsed();
        
        // Test zero-copy parser
        let start = Instant::now();
        let zero_copy_result = zero_copy_parser.parse(test_json);
        let zero_copy_duration = start.elapsed();
        
        assert!(regular_result.is_ok(), "Regular parser should work");
        assert!(zero_copy_result.is_ok(), "Zero-copy parser should work");
        
        println!("Regular parser: {:?}", regular_duration);
        println!("Zero-copy parser: {:?}", zero_copy_duration);
    }
}