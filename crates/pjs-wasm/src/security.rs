//! Security limits and validation for WASM module.
//!
//! This module provides configurable security limits to prevent DoS attacks
//! and resource exhaustion in browser environments.
//!
//! # Default Limits
//!
//! - Maximum JSON size: 10 MB
//! - Maximum nesting depth: 64 levels
//! - Maximum array elements per frame: 10,000
//! - Maximum object keys per level: 10,000
//!
//! # Example
//!
//! ```javascript
//! import { SecurityConfig } from 'pjs-wasm';
//!
//! const config = new SecurityConfig()
//!     .setMaxJsonSize(5 * 1024 * 1024)  // 5 MB
//!     .setMaxDepth(32);
//! ```

use wasm_bindgen::prelude::*;

/// Default maximum JSON input size (10 MB)
pub const DEFAULT_MAX_JSON_SIZE: usize = 10 * 1024 * 1024;

/// Default maximum nesting depth (64 levels)
pub const DEFAULT_MAX_DEPTH: usize = 64;

/// Default maximum array elements per frame
pub const DEFAULT_MAX_ARRAY_ELEMENTS: usize = 10_000;

/// Default maximum object keys per level
pub const DEFAULT_MAX_OBJECT_KEYS: usize = 10_000;

/// Security configuration for PJS WASM operations.
///
/// Use this to customize security limits for your application.
/// All limits have sensible defaults suitable for most use cases.
#[wasm_bindgen]
#[derive(Debug, Clone)]
pub struct SecurityConfig {
    /// Maximum allowed JSON input size in bytes
    max_json_size: usize,
    /// Maximum allowed nesting depth
    max_depth: usize,
    /// Maximum array elements allowed per frame
    max_array_elements: usize,
    /// Maximum object keys allowed per level
    max_object_keys: usize,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            max_json_size: DEFAULT_MAX_JSON_SIZE,
            max_depth: DEFAULT_MAX_DEPTH,
            max_array_elements: DEFAULT_MAX_ARRAY_ELEMENTS,
            max_object_keys: DEFAULT_MAX_OBJECT_KEYS,
        }
    }
}

#[wasm_bindgen]
impl SecurityConfig {
    /// Create a new SecurityConfig with default limits.
    ///
    /// Default limits:
    /// - Max JSON size: 10 MB
    /// - Max depth: 64 levels
    /// - Max array elements: 10,000
    /// - Max object keys: 10,000
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the maximum allowed JSON input size in bytes.
    ///
    /// # Arguments
    ///
    /// * `size` - Maximum size in bytes (must be > 0)
    ///
    /// # Example
    ///
    /// ```javascript
    /// config.setMaxJsonSize(5 * 1024 * 1024); // 5 MB
    /// ```
    #[wasm_bindgen(js_name = setMaxJsonSize)]
    pub fn set_max_json_size(mut self, size: usize) -> Self {
        if size > 0 {
            self.max_json_size = size;
        }
        self
    }

    /// Set the maximum allowed nesting depth.
    ///
    /// # Arguments
    ///
    /// * `depth` - Maximum depth (must be > 0, recommended: 32-128)
    ///
    /// # Example
    ///
    /// ```javascript
    /// config.setMaxDepth(32);
    /// ```
    #[wasm_bindgen(js_name = setMaxDepth)]
    pub fn set_max_depth(mut self, depth: usize) -> Self {
        if depth > 0 {
            self.max_depth = depth;
        }
        self
    }

    /// Set the maximum array elements allowed per frame.
    ///
    /// # Arguments
    ///
    /// * `elements` - Maximum elements (must be > 0)
    #[wasm_bindgen(js_name = setMaxArrayElements)]
    pub fn set_max_array_elements(mut self, elements: usize) -> Self {
        if elements > 0 {
            self.max_array_elements = elements;
        }
        self
    }

    /// Set the maximum object keys allowed per level.
    ///
    /// # Arguments
    ///
    /// * `keys` - Maximum keys (must be > 0)
    #[wasm_bindgen(js_name = setMaxObjectKeys)]
    pub fn set_max_object_keys(mut self, keys: usize) -> Self {
        if keys > 0 {
            self.max_object_keys = keys;
        }
        self
    }

    /// Get the maximum JSON size limit.
    #[wasm_bindgen(getter, js_name = maxJsonSize)]
    pub fn max_json_size(&self) -> usize {
        self.max_json_size
    }

    /// Get the maximum depth limit.
    #[wasm_bindgen(getter, js_name = maxDepth)]
    pub fn max_depth(&self) -> usize {
        self.max_depth
    }

    /// Get the maximum array elements limit.
    #[wasm_bindgen(getter, js_name = maxArrayElements)]
    pub fn max_array_elements(&self) -> usize {
        self.max_array_elements
    }

    /// Get the maximum object keys limit.
    #[wasm_bindgen(getter, js_name = maxObjectKeys)]
    pub fn max_object_keys(&self) -> usize {
        self.max_object_keys
    }
}

/// Security validation errors
#[derive(Debug, Clone)]
pub enum SecurityError {
    /// Input exceeds maximum size limit
    InputTooLarge { size: usize, max: usize },
    /// Nesting depth exceeds maximum
    MaxDepthExceeded { depth: usize, max: usize },
    /// Array has too many elements
    ArrayTooLarge { count: usize, max: usize },
    /// Object has too many keys
    ObjectTooLarge { count: usize, max: usize },
}

impl std::fmt::Display for SecurityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SecurityError::InputTooLarge { size, max } => {
                write!(
                    f,
                    "Input too large: {} bytes exceeds limit of {} bytes",
                    size, max
                )
            }
            SecurityError::MaxDepthExceeded { depth, max } => {
                write!(
                    f,
                    "Nesting too deep: depth {} exceeds limit of {}",
                    depth, max
                )
            }
            SecurityError::ArrayTooLarge { count, max } => {
                write!(
                    f,
                    "Array too large: {} elements exceeds limit of {}",
                    count, max
                )
            }
            SecurityError::ObjectTooLarge { count, max } => {
                write!(
                    f,
                    "Object too large: {} keys exceeds limit of {}",
                    count, max
                )
            }
        }
    }
}

impl std::error::Error for SecurityError {}

/// Validate JSON input size
pub fn validate_input_size(input: &str, config: &SecurityConfig) -> Result<(), SecurityError> {
    let size = input.len();
    if size > config.max_json_size {
        return Err(SecurityError::InputTooLarge {
            size,
            max: config.max_json_size,
        });
    }
    Ok(())
}

/// Validate nesting depth during recursion
pub fn validate_depth(current_depth: usize, config: &SecurityConfig) -> Result<(), SecurityError> {
    if current_depth > config.max_depth {
        return Err(SecurityError::MaxDepthExceeded {
            depth: current_depth,
            max: config.max_depth,
        });
    }
    Ok(())
}

/// Validate array size
pub fn validate_array_size(count: usize, config: &SecurityConfig) -> Result<(), SecurityError> {
    if count > config.max_array_elements {
        return Err(SecurityError::ArrayTooLarge {
            count,
            max: config.max_array_elements,
        });
    }
    Ok(())
}

/// Validate object size
pub fn validate_object_size(count: usize, config: &SecurityConfig) -> Result<(), SecurityError> {
    if count > config.max_object_keys {
        return Err(SecurityError::ObjectTooLarge {
            count,
            max: config.max_object_keys,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = SecurityConfig::new();
        assert_eq!(config.max_json_size(), DEFAULT_MAX_JSON_SIZE);
        assert_eq!(config.max_depth(), DEFAULT_MAX_DEPTH);
        assert_eq!(config.max_array_elements(), DEFAULT_MAX_ARRAY_ELEMENTS);
        assert_eq!(config.max_object_keys(), DEFAULT_MAX_OBJECT_KEYS);
    }

    #[test]
    fn test_config_builder() {
        let config = SecurityConfig::new()
            .set_max_json_size(1024)
            .set_max_depth(32)
            .set_max_array_elements(100)
            .set_max_object_keys(50);

        assert_eq!(config.max_json_size(), 1024);
        assert_eq!(config.max_depth(), 32);
        assert_eq!(config.max_array_elements(), 100);
        assert_eq!(config.max_object_keys(), 50);
    }

    #[test]
    fn test_config_ignores_zero_values() {
        let config = SecurityConfig::new().set_max_json_size(0).set_max_depth(0);

        // Should keep defaults when 0 is passed
        assert_eq!(config.max_json_size(), DEFAULT_MAX_JSON_SIZE);
        assert_eq!(config.max_depth(), DEFAULT_MAX_DEPTH);
    }

    #[test]
    fn test_validate_input_size_ok() {
        let config = SecurityConfig::new().set_max_json_size(100);
        let input = "x".repeat(50);
        assert!(validate_input_size(&input, &config).is_ok());
    }

    #[test]
    fn test_validate_input_size_too_large() {
        let config = SecurityConfig::new().set_max_json_size(100);
        let input = "x".repeat(150);
        let result = validate_input_size(&input, &config);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SecurityError::InputTooLarge { .. }
        ));
    }

    #[test]
    fn test_validate_depth_ok() {
        let config = SecurityConfig::new().set_max_depth(64);
        assert!(validate_depth(32, &config).is_ok());
        assert!(validate_depth(64, &config).is_ok());
    }

    #[test]
    fn test_validate_depth_exceeded() {
        let config = SecurityConfig::new().set_max_depth(64);
        let result = validate_depth(65, &config);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SecurityError::MaxDepthExceeded { .. }
        ));
    }

    #[test]
    fn test_validate_array_size() {
        let config = SecurityConfig::new().set_max_array_elements(100);
        assert!(validate_array_size(50, &config).is_ok());
        assert!(validate_array_size(101, &config).is_err());
    }

    #[test]
    fn test_validate_object_size() {
        let config = SecurityConfig::new().set_max_object_keys(100);
        assert!(validate_object_size(50, &config).is_ok());
        assert!(validate_object_size(101, &config).is_err());
    }

    #[test]
    fn test_error_display() {
        let err = SecurityError::InputTooLarge {
            size: 1000,
            max: 500,
        };
        let msg = err.to_string();
        assert!(msg.contains("1000"));
        assert!(msg.contains("500"));
    }
}
