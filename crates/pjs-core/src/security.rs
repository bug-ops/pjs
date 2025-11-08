//! Security validation utilities

use crate::{
    config::SecurityConfig,
    error::{Error, Result},
};

pub mod compression_bomb;
pub mod rate_limit;

pub use compression_bomb::{
    CompressionBombConfig, CompressionBombDetector, CompressionBombError, CompressionBombProtector,
    CompressionStats,
};
pub use rate_limit::{
    RateLimitConfig, RateLimitError, RateLimitGuard, RateLimitStats, WebSocketRateLimiter,
};

/// Security validator with configuration-based limits
#[derive(Debug, Clone)]
pub struct SecurityValidator {
    config: SecurityConfig,
}

impl SecurityValidator {
    /// Create new validator with given security configuration
    pub fn new(config: SecurityConfig) -> Self {
        Self { config }
    }

    /// Validate input size against security limits
    pub fn validate_input_size(&self, size: usize) -> Result<()> {
        if size > self.config.json.max_input_size {
            return Err(Error::Other(format!(
                "Input size {} exceeds maximum allowed {} bytes",
                size, self.config.json.max_input_size
            )));
        }
        Ok(())
    }

    /// Validate JSON depth to prevent stack overflow
    pub fn validate_json_depth(&self, depth: usize) -> Result<()> {
        if depth > self.config.json.max_depth {
            return Err(Error::Other(format!(
                "JSON nesting depth {} exceeds maximum allowed {}",
                depth, self.config.json.max_depth
            )));
        }
        Ok(())
    }

    /// Validate array length
    pub fn validate_array_length(&self, length: usize) -> Result<()> {
        if length > self.config.json.max_array_length {
            return Err(Error::Other(format!(
                "Array length {} exceeds maximum allowed {}",
                length, self.config.json.max_array_length
            )));
        }
        Ok(())
    }

    /// Validate object key count
    pub fn validate_object_keys(&self, key_count: usize) -> Result<()> {
        if key_count > self.config.json.max_object_keys {
            return Err(Error::Other(format!(
                "Object key count {} exceeds maximum allowed {}",
                key_count, self.config.json.max_object_keys
            )));
        }
        Ok(())
    }

    /// Validate string length
    pub fn validate_string_length(&self, length: usize) -> Result<()> {
        if length > self.config.json.max_string_length {
            return Err(Error::Other(format!(
                "String length {} exceeds maximum allowed {}",
                length, self.config.json.max_string_length
            )));
        }
        Ok(())
    }

    /// Validate session ID format and length
    pub fn validate_session_id(&self, session_id: &str) -> Result<()> {
        let len = session_id.len();

        if len < self.config.sessions.min_session_id_length {
            return Err(Error::Other(format!(
                "Session ID too short: {} characters (minimum {})",
                len, self.config.sessions.min_session_id_length
            )));
        }

        if len > self.config.sessions.max_session_id_length {
            return Err(Error::Other(format!(
                "Session ID too long: {} characters (maximum {})",
                len, self.config.sessions.max_session_id_length
            )));
        }

        // Check for valid characters (alphanumeric + hyphens + underscores)
        if !session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(Error::Other(
                "Session ID contains invalid characters (only alphanumeric, hyphens and underscores allowed)".to_string()
            ));
        }

        Ok(())
    }

    /// Validate WebSocket frame size
    pub fn validate_websocket_frame_size(&self, size: usize) -> Result<()> {
        if size > self.config.network.max_websocket_frame_size {
            return Err(Error::Other(format!(
                "WebSocket frame size {} exceeds maximum allowed {}",
                size, self.config.network.max_websocket_frame_size
            )));
        }
        Ok(())
    }

    /// Validate buffer size for pooling
    pub fn validate_buffer_size(&self, size: usize) -> Result<()> {
        if size > self.config.buffers.max_buffer_size {
            return Err(Error::Other(format!(
                "Buffer size {} exceeds maximum allowed {}",
                size, self.config.buffers.max_buffer_size
            )));
        }
        Ok(())
    }
}

impl Default for SecurityValidator {
    fn default() -> Self {
        Self::new(SecurityConfig::default())
    }
}

/// JSON depth tracker for preventing stack overflow
pub struct DepthTracker {
    current_depth: usize,
    max_depth: usize,
}

impl DepthTracker {
    /// Create depth tracker from security config
    pub fn from_config(config: &SecurityConfig) -> Self {
        Self {
            current_depth: 0,
            max_depth: config.json.max_depth,
        }
    }

    /// Create a new depth tracker with custom limit
    pub fn with_max_depth(max_depth: usize) -> Self {
        Self {
            current_depth: 0,
            max_depth,
        }
    }

    /// Enter a new nesting level (array/object)
    pub fn enter(&mut self) -> Result<()> {
        if self.current_depth >= self.max_depth {
            return Err(Error::Other(format!(
                "JSON nesting depth {} would exceed maximum allowed {}",
                self.current_depth + 1,
                self.max_depth
            )));
        }
        self.current_depth += 1;
        Ok(())
    }

    /// Exit a nesting level
    pub fn exit(&mut self) {
        if self.current_depth > 0 {
            self.current_depth -= 1;
        }
    }

    /// Get current depth
    pub fn current_depth(&self) -> usize {
        self.current_depth
    }
}

impl Default for DepthTracker {
    fn default() -> Self {
        Self::with_max_depth(64) // Default depth limit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_security_validator_default() {
        let validator = SecurityValidator::default();

        assert!(validator.validate_input_size(1024).is_ok());
        assert!(validator.validate_json_depth(10).is_ok());
        assert!(validator.validate_session_id("valid-session-123").is_ok());
    }

    #[test]
    fn test_security_validator_with_config() {
        let config = SecurityConfig::low_memory();
        let validator = SecurityValidator::new(config.clone());

        // Should use low memory limits
        assert!(
            validator
                .validate_input_size(config.json.max_input_size)
                .is_ok()
        );
        assert!(
            validator
                .validate_input_size(config.json.max_input_size + 1)
                .is_err()
        );

        // Test other validations
        assert!(validator.validate_json_depth(config.json.max_depth).is_ok());
        assert!(
            validator
                .validate_json_depth(config.json.max_depth + 1)
                .is_err()
        );
    }

    #[test]
    fn test_validate_session_id() {
        let validator = SecurityValidator::default();

        // Valid session IDs
        assert!(validator.validate_session_id("session-123").is_ok());
        assert!(validator.validate_session_id("abcd1234-5678-90ef").is_ok());
        assert!(validator.validate_session_id("test_session_id").is_ok());

        // Invalid session IDs
        assert!(validator.validate_session_id("ab").is_err()); // Too short
        assert!(validator.validate_session_id(&"a".repeat(200)).is_err()); // Too long
        assert!(validator.validate_session_id("session@123").is_err()); // Invalid chars
        assert!(validator.validate_session_id("session 123").is_err()); // Space
    }

    #[test]
    fn test_depth_tracker() {
        let mut tracker = DepthTracker::with_max_depth(64);

        assert_eq!(tracker.current_depth(), 0);

        assert!(tracker.enter().is_ok());
        assert_eq!(tracker.current_depth(), 1);

        assert!(tracker.enter().is_ok());
        assert_eq!(tracker.current_depth(), 2);

        tracker.exit();
        assert_eq!(tracker.current_depth(), 1);

        tracker.exit();
        assert_eq!(tracker.current_depth(), 0);
    }

    #[test]
    fn test_depth_tracker_limit() {
        let mut tracker = DepthTracker::with_max_depth(2);

        assert!(tracker.enter().is_ok());
        assert!(tracker.enter().is_ok());
        assert!(tracker.enter().is_err()); // Should exceed limit
    }

    #[test]
    fn test_depth_tracker_from_config() {
        let config = SecurityConfig::low_memory();
        let mut tracker = DepthTracker::from_config(&config);

        // Should respect config limits
        for _ in 0..config.json.max_depth {
            assert!(tracker.enter().is_ok());
        }
        assert!(tracker.enter().is_err()); // Should exceed limit
    }

    #[test]
    fn test_high_throughput_config() {
        let config = SecurityConfig::high_throughput();
        let _validator = SecurityValidator::new(config.clone());

        // High throughput should have higher limits than default
        let default_config = SecurityConfig::default();
        assert!(config.json.max_input_size >= default_config.json.max_input_size);
        assert!(config.buffers.max_total_memory >= default_config.buffers.max_total_memory);
    }

    #[test]
    fn test_validate_array_length() {
        let config = SecurityConfig::low_memory();
        let max_len = config.json.max_array_length;
        let validator = SecurityValidator::new(config);

        // Valid array length
        assert!(validator.validate_array_length(100).is_ok());

        // Invalid array length
        let result = validator.validate_array_length(max_len + 1);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_object_keys() {
        let validator = SecurityValidator::default();

        // Valid key count
        assert!(validator.validate_object_keys(10).is_ok());

        // Test with limit
        let config = SecurityConfig::low_memory();
        let max_keys = config.json.max_object_keys;
        let validator = SecurityValidator::new(config);
        let result = validator.validate_object_keys(max_keys + 1);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_string_length() {
        let validator = SecurityValidator::default();

        // Valid string length
        assert!(validator.validate_string_length(100).is_ok());

        // Test with limit
        let config = SecurityConfig::low_memory();
        let max_str_len = config.json.max_string_length;
        let validator = SecurityValidator::new(config);
        let result = validator.validate_string_length(max_str_len + 1);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_websocket_frame_size() {
        let validator = SecurityValidator::default();

        // Valid frame size
        assert!(validator.validate_websocket_frame_size(1024).is_ok());

        // Invalid frame size
        let config = SecurityConfig::low_memory();
        let max_frame = config.network.max_websocket_frame_size;
        let validator = SecurityValidator::new(config);
        let result = validator.validate_websocket_frame_size(max_frame + 1);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_buffer_size() {
        let validator = SecurityValidator::default();

        // Valid buffer size
        assert!(validator.validate_buffer_size(4096).is_ok());

        // Invalid buffer size
        let config = SecurityConfig::low_memory();
        let max_buf = config.buffers.max_buffer_size;
        let validator = SecurityValidator::new(config);
        let result = validator.validate_buffer_size(max_buf + 1);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_input_size_boundary() {
        let config = SecurityConfig::low_memory();
        let max_input = config.json.max_input_size;
        let validator = SecurityValidator::new(config);

        // At boundary
        assert!(validator.validate_input_size(max_input).is_ok());

        // Just over boundary
        let result = validator.validate_input_size(max_input + 1);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_json_depth_boundary() {
        let config = SecurityConfig::low_memory();
        let max_depth = config.json.max_depth;
        let validator = SecurityValidator::new(config);

        // At boundary
        assert!(validator.validate_json_depth(max_depth).is_ok());

        // Just over boundary
        let result = validator.validate_json_depth(max_depth + 1);
        assert!(result.is_err());
    }

    #[test]
    fn test_session_id_length_boundaries() {
        let validator = SecurityValidator::default();

        // Too short
        let result = validator.validate_session_id("a");
        assert!(result.is_err());

        // Too long (default max is 128)
        let long_id = "a".repeat(200);
        let result = validator.validate_session_id(&long_id);
        assert!(result.is_err());

        // Valid length with hyphens
        assert!(validator.validate_session_id("valid-session-id-123").is_ok());

        // Valid length with underscores
        assert!(validator.validate_session_id("valid_session_id_123").is_ok());
    }

    #[test]
    fn test_session_id_invalid_characters() {
        let validator = SecurityValidator::default();

        // Invalid: special characters
        let result = validator.validate_session_id("session@123");
        assert!(result.is_err());

        // Invalid: spaces
        let result = validator.validate_session_id("session 123");
        assert!(result.is_err());

        // Invalid: dots
        let result = validator.validate_session_id("session.123");
        assert!(result.is_err());

        // Valid: alphanumeric only
        assert!(validator.validate_session_id("session123").is_ok());
    }

    #[test]
    fn test_depth_tracker_boundary_cases() {
        let mut tracker = DepthTracker::with_max_depth(1);

        // Can enter once
        assert!(tracker.enter().is_ok());
        assert_eq!(tracker.current_depth(), 1);

        // Cannot enter again
        assert!(tracker.enter().is_err());
        assert_eq!(tracker.current_depth(), 1); // Depth not incremented on error

        // Can exit
        tracker.exit();
        assert_eq!(tracker.current_depth(), 0);

        // Can enter again after exit
        assert!(tracker.enter().is_ok());
    }

    #[test]
    fn test_depth_tracker_exit_at_zero() {
        let mut tracker = DepthTracker::with_max_depth(64);

        // Starting at 0
        assert_eq!(tracker.current_depth(), 0);

        // Exiting at 0 should not go negative
        tracker.exit();
        assert_eq!(tracker.current_depth(), 0);

        // Should still be able to enter
        assert!(tracker.enter().is_ok());
    }

    #[test]
    fn test_depth_tracker_multiple_cycles() {
        let mut tracker = DepthTracker::with_max_depth(3);

        // Cycle 1: enter, exit
        assert!(tracker.enter().is_ok());
        tracker.exit();
        assert_eq!(tracker.current_depth(), 0);

        // Cycle 2: multiple enters and exits
        assert!(tracker.enter().is_ok());
        assert!(tracker.enter().is_ok());
        tracker.exit();
        assert_eq!(tracker.current_depth(), 1);
        tracker.exit();
        assert_eq!(tracker.current_depth(), 0);

        // Cycle 3: stress to max
        for _ in 0..3 {
            assert!(tracker.enter().is_ok());
        }
        assert_eq!(tracker.current_depth(), 3);
    }
}
