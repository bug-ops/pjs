//! Priority configuration API for JavaScript
//!
//! This module provides a JavaScript-friendly API for configuring
//! priority assignment rules.

use crate::priority_assignment::PriorityConfig;
use wasm_bindgen::prelude::*;

/// Priority configuration builder for JavaScript.
///
/// This class allows JavaScript code to configure how priorities are
/// assigned to different JSON fields based on field names and patterns.
///
/// # Example
///
/// ```javascript
/// import { PriorityConfigBuilder } from 'pjs-wasm';
///
/// const config = new PriorityConfigBuilder()
///   .addCriticalField('user_id')
///   .addHighField('display_name')
///   .addLowPattern('debug')
///   .build();
/// ```
#[wasm_bindgen]
pub struct PriorityConfigBuilder {
    config: PriorityConfig,
}

#[wasm_bindgen]
impl PriorityConfigBuilder {
    /// Create a new configuration builder with default values.
    ///
    /// # Example
    ///
    /// ```javascript
    /// const builder = new PriorityConfigBuilder();
    /// ```
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            config: PriorityConfig::default(),
        }
    }

    /// Add a field name that should have critical priority.
    ///
    /// Fields with critical priority (100) are streamed first after the skeleton.
    ///
    /// # Arguments
    ///
    /// * `field` - The field name (e.g., "id", "uuid")
    ///
    /// # Returns
    ///
    /// Self for method chaining
    ///
    /// # Example
    ///
    /// ```javascript
    /// builder.addCriticalField('user_id').addCriticalField('session_id');
    /// ```
    #[wasm_bindgen(js_name = addCriticalField)]
    pub fn add_critical_field(mut self, field: String) -> Self {
        self.config.add_critical_field(field);
        self
    }

    /// Add a field name that should have high priority.
    ///
    /// Fields with high priority (80) are streamed after critical fields.
    ///
    /// # Arguments
    ///
    /// * `field` - The field name (e.g., "name", "title")
    ///
    /// # Returns
    ///
    /// Self for method chaining
    ///
    /// # Example
    ///
    /// ```javascript
    /// builder.addHighField('name').addHighField('title');
    /// ```
    #[wasm_bindgen(js_name = addHighField)]
    pub fn add_high_field(mut self, field: String) -> Self {
        self.config.add_high_field(field);
        self
    }

    /// Add a pattern that matches low priority fields.
    ///
    /// Fields containing these patterns get low priority (25).
    ///
    /// # Arguments
    ///
    /// * `pattern` - The pattern to match (e.g., "meta", "stats")
    ///
    /// # Returns
    ///
    /// Self for method chaining
    ///
    /// # Example
    ///
    /// ```javascript
    /// builder.addLowPattern('meta').addLowPattern('stats');
    /// ```
    #[wasm_bindgen(js_name = addLowPattern)]
    pub fn add_low_pattern(mut self, pattern: String) -> Self {
        self.config.add_low_pattern(pattern);
        self
    }

    /// Add a pattern that matches background priority fields.
    ///
    /// Fields containing these patterns get background priority (10).
    ///
    /// # Arguments
    ///
    /// * `pattern` - The pattern to match (e.g., "logs", "analytics")
    ///
    /// # Returns
    ///
    /// Self for method chaining
    ///
    /// # Example
    ///
    /// ```javascript
    /// builder.addBackgroundPattern('logs').addBackgroundPattern('analytics');
    /// ```
    #[wasm_bindgen(js_name = addBackgroundPattern)]
    pub fn add_background_pattern(mut self, pattern: String) -> Self {
        self.config.add_background_pattern(pattern);
        self
    }

    /// Set the threshold for considering an array "large".
    ///
    /// Arrays larger than this size get downgraded priority.
    ///
    /// # Arguments
    ///
    /// * `threshold` - The maximum array size (default: 100)
    ///
    /// # Returns
    ///
    /// Self for method chaining
    ///
    /// # Example
    ///
    /// ```javascript
    /// builder.setLargeArrayThreshold(200);
    /// ```
    #[wasm_bindgen(js_name = setLargeArrayThreshold)]
    pub fn set_large_array_threshold(mut self, threshold: usize) -> Self {
        self.config.large_array_threshold = threshold;
        self
    }

    /// Set the threshold for considering a string "large".
    ///
    /// Strings longer than this get downgraded priority.
    ///
    /// # Arguments
    ///
    /// * `threshold` - The maximum string length (default: 1000)
    ///
    /// # Returns
    ///
    /// Self for method chaining
    ///
    /// # Example
    ///
    /// ```javascript
    /// builder.setLargeStringThreshold(5000);
    /// ```
    #[wasm_bindgen(js_name = setLargeStringThreshold)]
    pub fn set_large_string_threshold(mut self, threshold: usize) -> Self {
        self.config.large_string_threshold = threshold;
        self
    }

    /// Build the final configuration (internal use).
    ///
    /// This method consumes the builder and returns the configuration.
    /// It's used internally by the parser and not exposed to JavaScript.
    pub(crate) fn build_internal(self) -> PriorityConfig {
        self.config
    }
}

impl Default for PriorityConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_creation() {
        let builder = PriorityConfigBuilder::new();
        let config = builder.build_internal();

        // Default config should have standard critical fields
        assert!(config.critical_fields.contains(&"id".to_string()));
    }

    #[test]
    fn test_builder_chaining() {
        let config = PriorityConfigBuilder::new()
            .add_critical_field("custom_id".to_string())
            .add_high_field("display_name".to_string())
            .add_low_pattern("debug".to_string())
            .add_background_pattern("trace".to_string())
            .set_large_array_threshold(200)
            .set_large_string_threshold(5000)
            .build_internal();

        assert!(config.critical_fields.contains(&"custom_id".to_string()));
        assert!(config.high_fields.contains(&"display_name".to_string()));
        assert!(config.low_patterns.contains(&"debug".to_string()));
        assert!(config.background_patterns.contains(&"trace".to_string()));
        assert_eq!(config.large_array_threshold, 200);
        assert_eq!(config.large_string_threshold, 5000);
    }

    #[test]
    fn test_default_builder() {
        let builder = PriorityConfigBuilder::default();
        let config = builder.build_internal();
        assert!(!config.critical_fields.is_empty());
    }
}
