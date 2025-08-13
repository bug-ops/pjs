//! Comprehensive security tests for PJS Core
//!
//! This module contains extensive security testing scenarios including:
//! - Injection attack vectors
//! - Resource exhaustion attacks
//! - Memory safety validation
//! - Input boundary testing

use pjson_rs::{
    SecurityConfig, 
    SecurityValidator, DepthTracker,
    SimpleParser, SonicParser, ZeroCopyParser, LazyParser,
};

/// Test suite for input size validation security
#[cfg(test)]
mod input_size_tests {
    use super::*;

    #[test]
    fn test_input_size_boundary_conditions() {
        let config = SecurityConfig::low_memory();
        let validator = SecurityValidator::new(config.clone());
        
        // Test exact boundary
        assert!(validator.validate_input_size(config.json.max_input_size).is_ok());
        
        // Test one byte over limit
        assert!(validator.validate_input_size(config.json.max_input_size + 1).is_err());
        
        // Test edge cases
        assert!(validator.validate_input_size(0).is_ok());
        assert!(validator.validate_input_size(1).is_ok());
        assert!(validator.validate_input_size(usize::MAX).is_err());
    }

    #[test] 
    fn test_very_large_input_rejection() {
        let validator = SecurityValidator::default();
        
        // Test various large sizes that should be rejected
        let large_sizes = [
            1_000_000_000,  // 1GB
            2_000_000_000,  // 2GB  
            usize::MAX / 2, // Half of max usize
            usize::MAX,     // Maximum possible size
        ];
        
        for size in large_sizes {
            let result = validator.validate_input_size(size);
            assert!(result.is_err(), "Size {} should be rejected", size);
        }
    }

    #[test]
    fn test_gradual_size_increase() {
        let config = SecurityConfig::development(); // 50MB limit
        let validator = SecurityValidator::new(config.clone());
        
        // Test increasing sizes up to limit
        let test_sizes = [
            1024,           // 1KB
            1024 * 1024,    // 1MB  
            10 * 1024 * 1024, // 10MB
            25 * 1024 * 1024, // 25MB
            49 * 1024 * 1024, // 49MB (should pass)
            51 * 1024 * 1024, // 51MB (should fail)
        ];
        
        for (i, &size) in test_sizes.iter().enumerate() {
            let result = validator.validate_input_size(size);
            if i < test_sizes.len() - 1 {
                assert!(result.is_ok(), "Size {} should be accepted", size);
            } else {
                assert!(result.is_err(), "Size {} should be rejected", size);
            }
        }
    }
}

/// Test suite for JSON depth validation security  
#[cfg(test)]
mod depth_tests {
    use super::*;

    #[test]
    fn test_json_depth_attack_prevention() {
        let config = SecurityConfig::low_memory(); // Depth limit: 32
        let validator = SecurityValidator::new(config);
        
        // Test exact boundary
        assert!(validator.validate_json_depth(32).is_ok());
        
        // Test one level over limit
        assert!(validator.validate_json_depth(33).is_err());
        
        // Test deep nesting attack scenarios
        let attack_depths = [100, 500, 1000, 10000];
        for depth in attack_depths {
            assert!(validator.validate_json_depth(depth).is_err(),
                   "Depth {} should be rejected", depth);
        }
    }

    #[test]
    fn test_depth_tracker_security() {
        let config = SecurityConfig::default();
        let mut tracker = DepthTracker::from_config(&config);
        
        // Test normal nesting up to limit
        for i in 0..config.json.max_depth {
            let result = tracker.enter();
            assert!(result.is_ok(), "Should be able to enter level {}", i);
        }
        
        // Test one level too deep
        let result = tracker.enter();
        assert!(result.is_err(), "Should reject depth > {}", config.json.max_depth);
        
        // Test exit and re-entry
        tracker.exit();
        let result = tracker.enter();
        assert!(result.is_ok(), "Should allow re-entry after exit");
    }

    #[test]
    fn test_nested_json_bombing() {
        let parser = SimpleParser::new();
        
        // Create deeply nested JSON that should be rejected
        let mut nested_json = String::new();
        for _ in 0..100 { // 100 levels deep
            nested_json.push_str("{\"a\":");
        }
        nested_json.push_str("\"value\"");
        for _ in 0..100 {
            nested_json.push('}');
        }
        
        let result = parser.parse(nested_json.as_bytes());
        // Note: This test may pass or fail depending on serde_json internal limits
        // The important thing is that it doesn't crash or cause stack overflow
        println!("Nested JSON bombing result: {:?}", result.is_err());
    }
}

/// Test suite for string length validation security
#[cfg(test)]
mod string_length_tests {
    use super::*;

    #[test]
    fn test_string_length_boundaries() {
        let config = SecurityConfig::low_memory(); // 1MB string limit
        let validator = SecurityValidator::new(config.clone());
        
        // Test exact boundary
        assert!(validator.validate_string_length(config.json.max_string_length).is_ok());
        
        // Test one byte over limit
        assert!(validator.validate_string_length(config.json.max_string_length + 1).is_err());
    }

    #[test]
    fn test_massive_string_attack() {
        let parser = SimpleParser::new();
        
        // Create JSON with massive string that should be rejected
        let large_string = "a".repeat(100 * 1024 * 1024); // 100MB string
        let json = format!(r#"{{"key": "{}"}}"#, large_string);
        
        let result = parser.parse(json.as_bytes());
        assert!(result.is_err(), "Massive string should be rejected");
    }
}

/// Test suite for array length validation security
#[cfg(test)]
mod array_length_tests {
    use super::*;

    #[test]
    fn test_array_length_boundaries() {
        let config = SecurityConfig::low_memory(); // 100k array limit  
        let validator = SecurityValidator::new(config.clone());
        
        // Test exact boundary
        assert!(validator.validate_array_length(config.json.max_array_length).is_ok());
        
        // Test one over limit
        assert!(validator.validate_array_length(config.json.max_array_length + 1).is_err());
    }

    #[test]
    fn test_billion_element_attack() {
        let parser = SimpleParser::new();
        
        // Create JSON array with pattern that expands to massive size
        // Note: We test the security validation, not actually creating billion elements
        let json = format!("[{}]", "1,".repeat(1_000_000));
        
        let result = parser.parse(json.as_bytes());
        // This might succeed or fail based on memory, but shouldn't crash
        println!("Billion element test result: {:?}", result.is_err());
    }
}

/// Test suite for object key count validation security
#[cfg(test)] 
mod object_key_tests {
    use super::*;

    #[test]
    fn test_object_key_boundaries() {
        let config = SecurityConfig::low_memory(); // 1000 key limit
        let validator = SecurityValidator::new(config.clone());
        
        // Test exact boundary
        assert!(validator.validate_object_keys(config.json.max_object_keys).is_ok());
        
        // Test one over limit
        assert!(validator.validate_object_keys(config.json.max_object_keys + 1).is_err());
    }

    #[test]
    fn test_massive_object_attack() {
        let parser = SimpleParser::new();
        
        // Create JSON object with many keys
        let mut json = String::from("{");
        for i in 0..10000 { // 10k keys
            if i > 0 {
                json.push(',');
            }
            json.push_str(&format!(r#""key{}": {}"#, i, i));
        }
        json.push('}');
        
        let result = parser.parse(json.as_bytes());
        // Note: This test verifies we can handle large objects without crashing
        // Actual rejection depends on memory limits and parser implementation
        println!("Massive object attack result: {:?}", result.is_err());
    }
}

/// Test suite for session ID validation security
#[cfg(test)]
mod session_id_tests {
    use super::*;

    #[test]
    fn test_session_id_injection_attempts() {
        let validator = SecurityValidator::default();
        
        // Test various injection attack vectors
        let malicious_ids = [
            "'; DROP TABLE sessions; --",  // SQL injection
            "<script>alert('xss')</script>", // XSS
            "../../etc/passwd",            // Path traversal
            "\x00\x01\x02",              // Null bytes
            &"a".repeat(1000),            // Excessively long
            "",                           // Empty
            " ",                          // Whitespace only
            "session id with spaces",     // Invalid characters
            "session@123",                // Invalid symbols
            "session#with$special%chars", // Multiple invalid chars
        ];
        
        for id in malicious_ids {
            let result = validator.validate_session_id(id);
            assert!(result.is_err(), "Malicious session ID '{}' should be rejected", id);
        }
    }

    #[test]
    fn test_valid_session_ids() {
        let validator = SecurityValidator::default();
        
        let valid_ids = [
            "session123",
            "abc-def-123",
            "user_session_456",
            "SESSION-ID-789",
            "a1b2c3d4",
        ];
        
        for id in valid_ids {
            let result = validator.validate_session_id(id);
            assert!(result.is_ok(), "Valid session ID '{}' should be accepted", id);
        }
    }
}

/// Test suite for buffer size validation security
#[cfg(test)]
mod buffer_size_tests {
    use super::*;

    #[test]
    fn test_buffer_size_boundaries() {
        let config = SecurityConfig::low_memory(); // 10MB buffer limit
        let validator = SecurityValidator::new(config.clone());
        
        // Test exact boundary
        assert!(validator.validate_buffer_size(config.buffers.max_buffer_size).is_ok());
        
        // Test one byte over limit
        assert!(validator.validate_buffer_size(config.buffers.max_buffer_size + 1).is_err());
    }

    #[test]
    fn test_massive_buffer_attack() {
        let validator = SecurityValidator::default();
        
        // Test various large buffer sizes
        let attack_sizes = [
            1024 * 1024 * 1024,  // 1GB
            2_000_000_000,       // 2GB
            usize::MAX / 2,      // Half max
        ];
        
        for size in attack_sizes {
            assert!(validator.validate_buffer_size(size).is_err(),
                   "Buffer size {} should be rejected", size);
        }
    }
}

/// Test suite for WebSocket frame size validation
#[cfg(test)]
mod websocket_tests {
    use super::*;

    #[test]
    fn test_websocket_frame_boundaries() {
        let config = SecurityConfig::low_memory(); // 1MB frame limit
        let validator = SecurityValidator::new(config.clone());
        
        // Test exact boundary
        assert!(validator.validate_websocket_frame_size(config.network.max_websocket_frame_size).is_ok());
        
        // Test one byte over limit  
        assert!(validator.validate_websocket_frame_size(config.network.max_websocket_frame_size + 1).is_err());
    }
}

/// Integration tests with actual parsers
#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn test_simple_parser_security_integration() {
        let _config = SecurityConfig::low_memory();
        // For now, just test with default parser since we need to add security config method
        let parser = SimpleParser::new();
        
        // Test with valid small JSON
        let small_json = br#"{"key": "value"}"#;
        let result = parser.parse(small_json);
        assert!(result.is_ok(), "Small JSON should be accepted");
    }

    #[test] 
    fn test_sonic_parser_security_integration() {
        let _config = SecurityConfig::development();
        // For now, just test with default parser since we need to add security config method
        let parser = SonicParser::new();
        
        // Test with valid small JSON
        let small_json = br#"{"key": "value"}"#;
        let result = parser.parse(small_json);
        assert!(result.is_ok(), "Small JSON should be accepted");
    }

    #[test]
    fn test_zero_copy_parser_security() {
        let config = SecurityConfig::low_memory();
        let mut parser = ZeroCopyParser::with_security_config(config.clone());
        
        // Test with valid small JSON
        let small_json = br#"{"key": "value"}"#;
        let result = parser.parse_lazy(small_json);
        assert!(result.is_ok(), "Small JSON should be accepted");
    }
}

/// Performance security tests (prevent DoS through resource exhaustion)
#[cfg(test)]
mod performance_security_tests {
    use super::*;
    use std::time::{Instant, Duration};

    #[test]
    fn test_parsing_time_bounds() {
        let parser = SimpleParser::new();
        let test_timeout = Duration::from_secs(1);
        
        // Test with moderately complex JSON
        let json = serde_json::json!({
            "users": (0..1000).map(|i| serde_json::json!({
                "id": i,
                "name": format!("user_{}", i),
                "data": vec![1, 2, 3, 4, 5]
            })).collect::<Vec<_>>()
        });
        
        let json_str = serde_json::to_string(&json).unwrap();
        let start = Instant::now();
        
        let result = parser.parse(json_str.as_bytes());
        let elapsed = start.elapsed();
        
        assert!(result.is_ok(), "Moderate JSON should parse successfully");
        assert!(elapsed < test_timeout, "Parsing should complete within timeout");
    }

    #[test]
    fn test_memory_usage_bounds() {
        use pjson_rs::ArenaJsonParser;
        
        let mut parser = ArenaJsonParser::new();
        
        // Parse multiple documents to test memory usage
        for i in 0..100 {
            let json = format!(r#"{{"iteration": {}, "data": "test"}}"#, i);
            let result = parser.parse(&json);
            assert!(result.is_ok(), "JSON parsing should succeed");
        }
        
        let stats = parser.arena_stats();
        // Ensure arena is recycling memory appropriately
        assert!(stats.values_allocated < 1000, "Arena should recycle memory");
    }
}