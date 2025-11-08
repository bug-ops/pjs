//! JSON Parser with priority support for WebAssembly.
//!
//! This module provides the main parser interface for the WASM bindings.
//! It wraps the pure domain logic from `pjs-domain` and exposes it through
//! a JavaScript-friendly API using wasm-bindgen.

use wasm_bindgen::prelude::*;
use pjs_domain::value_objects::JsonData;

/// PJS Parser for WebAssembly.
///
/// This struct provides a JavaScript-compatible interface for parsing JSON
/// with priority support. It's designed to be instantiated from JavaScript
/// and used to parse JSON strings.
///
/// # Example
///
/// ```javascript
/// import { PjsParser } from 'pjs-wasm';
///
/// const parser = new PjsParser();
/// const result = parser.parse('{"name": "Alice", "age": 30}');
/// console.log(result);
/// ```
#[wasm_bindgen]
pub struct PjsParser {
    // Future: Add configuration options or state here if needed
}

#[wasm_bindgen]
impl PjsParser {
    /// Create a new parser instance.
    ///
    /// # Returns
    ///
    /// A new `PjsParser` ready to parse JSON strings.
    ///
    /// # Example
    ///
    /// ```javascript
    /// const parser = new PjsParser();
    /// ```
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {}
    }

    /// Parse a JSON string and return the result.
    ///
    /// This method parses a JSON string using `serde_json` (WASM-compatible)
    /// and converts it to the domain's `JsonData` type, then serializes it
    /// back to a JsValue for JavaScript consumption.
    ///
    /// # Arguments
    ///
    /// * `json_str` - The JSON string to parse
    ///
    /// # Returns
    ///
    /// * `Ok(JsValue)` - The parsed JSON as a JavaScript value
    /// * `Err(JsValue)` - An error message if parsing fails
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The input is not valid JSON
    /// - The conversion to JsValue fails
    ///
    /// # Example
    ///
    /// ```javascript
    /// const parser = new PjsParser();
    /// try {
    ///     const result = parser.parse('{"key": "value"}');
    ///     console.log(result);
    /// } catch (error) {
    ///     console.error('Parse error:', error);
    /// }
    /// ```
    #[wasm_bindgen]
    pub fn parse(&self, json_str: &str) -> Result<JsValue, JsValue> {
        // Parse with serde_json (WASM-compatible, unlike sonic-rs)
        let value: serde_json::Value = serde_json::from_str(json_str)
            .map_err(|e| JsValue::from_str(&format!("Parse error: {}", e)))?;

        // Convert to domain JsonData
        let json_data: JsonData = value.into();

        // Convert to JsValue for return to JavaScript
        serde_wasm_bindgen::to_value(&json_data)
            .map_err(|e| JsValue::from_str(&format!("Conversion error: {}", e)))
    }

    /// Get the parser version.
    ///
    /// # Returns
    ///
    /// The version string of the pjs-wasm crate.
    ///
    /// # Example
    ///
    /// ```javascript
    /// console.log(`Parser version: ${PjsParser.version()}`);
    /// ```
    #[wasm_bindgen]
    pub fn version() -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }
}

impl Default for PjsParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parser_creation() {
        let _parser = PjsParser::new();
        // Parser created successfully
    }

    #[test]
    fn test_parser_default() {
        let _parser = PjsParser::default();
        // Parser created successfully using default
    }

    #[test]
    fn test_version() {
        let version = PjsParser::version();
        assert!(!version.is_empty());
        assert_eq!(version, env!("CARGO_PKG_VERSION"));
    }

    // Note: Parsing tests require WASM environment and should be run with
    // wasm-bindgen-test in a browser or Node.js environment.
    // See wasm-bindgen-test documentation for details.
}

#[cfg(all(test, target_arch = "wasm32"))]
mod wasm_tests {
    use super::*;
    use wasm_bindgen_test::*;

    #[wasm_bindgen_test]
    fn test_parse_simple_object() {
        let parser = PjsParser::new();
        let result = parser.parse(r#"{"name": "test"}"#);
        assert!(result.is_ok());
    }

    #[wasm_bindgen_test]
    fn test_parse_invalid_json() {
        let parser = PjsParser::new();
        let result = parser.parse(r#"{"invalid"#);
        assert!(result.is_err());
    }

    #[wasm_bindgen_test]
    fn test_parse_array() {
        let parser = PjsParser::new();
        let result = parser.parse(r#"[1, 2, 3]"#);
        assert!(result.is_ok());
    }

    #[wasm_bindgen_test]
    fn test_parse_nested() {
        let parser = PjsParser::new();
        let result = parser.parse(r#"{"nested": {"value": 42}}"#);
        assert!(result.is_ok());
    }
}
