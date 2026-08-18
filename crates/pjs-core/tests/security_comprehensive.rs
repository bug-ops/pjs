//! Comprehensive security tests for PJS Core
//!
//! This module contains extensive security testing scenarios including:
//! - Injection attack vectors
//! - Resource exhaustion attacks
//! - Memory safety validation
//! - Input boundary testing

use pjson_rs::{DepthTracker, LazyParser, SecurityConfig, SecurityValidator, ZeroCopyParser};

/// Test suite for input size validation security
#[cfg(test)]
mod input_size_tests {
    use super::*;

    #[test]
    fn test_input_size_boundary_conditions() {
        let config = SecurityConfig::low_memory();
        let validator = SecurityValidator::new(config.clone());

        // Test exact boundary
        assert!(
            validator
                .validate_input_size(config.json.max_input_size)
                .is_ok()
        );

        // Test one byte over limit
        assert!(
            validator
                .validate_input_size(config.json.max_input_size + 1)
                .is_err()
        );

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
            1024,             // 1KB
            1024 * 1024,      // 1MB
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
            assert!(
                validator.validate_json_depth(depth).is_err(),
                "Depth {} should be rejected",
                depth
            );
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
        assert!(
            result.is_err(),
            "Should reject depth > {}",
            config.json.max_depth
        );

        // Test exit and re-entry
        tracker.exit();
        let result = tracker.enter();
        assert!(result.is_ok(), "Should allow re-entry after exit");
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
        assert!(
            validator
                .validate_string_length(config.json.max_string_length)
                .is_ok()
        );

        // Test one byte over limit
        assert!(
            validator
                .validate_string_length(config.json.max_string_length + 1)
                .is_err()
        );
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
        assert!(
            validator
                .validate_array_length(config.json.max_array_length)
                .is_ok()
        );

        // Test one over limit
        assert!(
            validator
                .validate_array_length(config.json.max_array_length + 1)
                .is_err()
        );
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
        assert!(
            validator
                .validate_object_keys(config.json.max_object_keys)
                .is_ok()
        );

        // Test one over limit
        assert!(
            validator
                .validate_object_keys(config.json.max_object_keys + 1)
                .is_err()
        );
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
            "'; DROP TABLE sessions; --",    // SQL injection
            "<script>alert('xss')</script>", // XSS
            "../../etc/passwd",              // Path traversal
            "\x00\x01\x02",                  // Null bytes
            &"a".repeat(1000),               // Excessively long
            "",                              // Empty
            " ",                             // Whitespace only
            "session id with spaces",        // Invalid characters
            "session@123",                   // Invalid symbols
            "session#with$special%chars",    // Multiple invalid chars
        ];

        for id in malicious_ids {
            let result = validator.validate_session_id(id);
            assert!(
                result.is_err(),
                "Malicious session ID '{}' should be rejected",
                id
            );
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
            assert!(
                result.is_ok(),
                "Valid session ID '{}' should be accepted",
                id
            );
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
        assert!(
            validator
                .validate_buffer_size(config.buffers.max_buffer_size)
                .is_ok()
        );

        // Test one byte over limit
        assert!(
            validator
                .validate_buffer_size(config.buffers.max_buffer_size + 1)
                .is_err()
        );
    }

    #[test]
    fn test_massive_buffer_attack() {
        let validator = SecurityValidator::default();

        // Test various large buffer sizes
        let attack_sizes = [
            1024 * 1024 * 1024, // 1GB
            2_000_000_000,      // 2GB
            usize::MAX / 2,     // Half max
        ];

        for size in attack_sizes {
            assert!(
                validator.validate_buffer_size(size).is_err(),
                "Buffer size {} should be rejected",
                size
            );
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
        assert!(
            validator
                .validate_websocket_frame_size(config.network.max_websocket_frame_size)
                .is_ok()
        );

        // Test one byte over limit
        assert!(
            validator
                .validate_websocket_frame_size(config.network.max_websocket_frame_size + 1)
                .is_err()
        );
    }
}

/// Integration tests with actual parsers
#[cfg(test)]
mod integration_tests {
    use super::*;

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
