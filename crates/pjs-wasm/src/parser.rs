//! JSON Parser with priority support for WebAssembly.
//!
//! This module provides the main parser interface for the WASM bindings.
//! It wraps the pure domain logic from `pjs-domain` and exposes it through
//! a JavaScript-friendly API using wasm-bindgen.

use crate::priority_assignment::{PriorityAssigner, group_by_priority, sort_priorities};
use crate::priority_config::PriorityConfigBuilder;
use pjs_domain::entities::Frame;
use pjs_domain::entities::frame::FramePatch;
use pjs_domain::value_objects::{JsonData, Priority, StreamId};
use std::collections::HashMap;
use wasm_bindgen::prelude::*;

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
    priority_assigner: PriorityAssigner,
}

#[wasm_bindgen]
impl PjsParser {
    /// Create a new parser instance with default configuration.
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
        Self {
            priority_assigner: PriorityAssigner::new(),
        }
    }

    /// Create a parser with custom priority configuration.
    ///
    /// This allows you to customize which fields get which priorities.
    ///
    /// # Arguments
    ///
    /// * `config_builder` - A PriorityConfigBuilder with custom rules
    ///
    /// # Returns
    ///
    /// A new `PjsParser` with custom configuration
    ///
    /// # Example
    ///
    /// ```javascript
    /// import { PjsParser, PriorityConfigBuilder } from 'pjs-wasm';
    ///
    /// const config = new PriorityConfigBuilder()
    ///   .addCriticalField('user_id')
    ///   .addHighField('display_name');
    ///
    /// const parser = PjsParser.withConfig(config);
    /// ```
    #[wasm_bindgen(js_name = withConfig)]
    pub fn with_config(config_builder: PriorityConfigBuilder) -> Self {
        let config = config_builder.build_internal();
        Self {
            priority_assigner: PriorityAssigner::with_config(config),
        }
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

    /// Generate priority-based frames from JSON data.
    ///
    /// This method analyzes the JSON structure, assigns priorities to fields,
    /// and generates multiple frames based on priority levels. The frames are
    /// ordered from highest to lowest priority.
    ///
    /// # Arguments
    ///
    /// * `json_str` - JSON string to convert to frames
    /// * `min_priority` - Minimum priority threshold (1-255)
    ///
    /// # Returns
    ///
    /// Array of frames as JsValue, ordered by priority (highest first)
    ///
    /// # Example
    ///
    /// ```javascript
    /// const parser = new PjsParser();
    /// const frames = parser.generateFrames('{"id": 1, "name": "Alice", "bio": "..."}', 50);
    /// // Returns: [skeleton, critical_patch, high_patch, ..., complete]
    /// console.log(frames);
    /// ```
    #[wasm_bindgen(js_name = generateFrames)]
    pub fn generate_frames(&self, json_str: &str, min_priority: u8) -> Result<JsValue, JsValue> {
        // Parse JSON
        let value: serde_json::Value = serde_json::from_str(json_str)
            .map_err(|e| JsValue::from_str(&format!("Parse error: {}", e)))?;

        let json_data: JsonData = value.into();

        // Create stream ID
        let stream_id = StreamId::new();

        // Validate minimum priority threshold
        let min_priority_threshold = Priority::new(min_priority)
            .map_err(|e| JsValue::from_str(&format!("Invalid priority: {:?}", e)))?;

        // Generate frames using domain logic and priority assignment
        let frames = self
            .generate_frames_internal(&json_data, stream_id, min_priority_threshold)
            .map_err(|e| JsValue::from_str(&format!("Frame generation error: {:?}", e)))?;

        // Convert frames to JsValue
        serde_wasm_bindgen::to_value(&frames)
            .map_err(|e| JsValue::from_str(&format!("Serialization error: {}", e)))
    }

    /// Internal frame generation logic (not exposed to JS)
    fn generate_frames_internal(
        &self,
        data: &JsonData,
        stream_id: StreamId,
        min_priority: Priority,
    ) -> Result<Vec<Frame>, String> {
        let mut frames = Vec::new();
        let mut sequence = 0u64;

        // 1. Generate skeleton frame (always first, critical priority)
        let skeleton = Self::create_skeleton(data);
        frames.push(Frame::skeleton(stream_id, sequence, skeleton));
        sequence += 1;

        // 2. Extract all fields with priorities
        let prioritized_fields = self.priority_assigner.extract_prioritized_fields(data);

        // 3. Group fields by priority level
        let grouped = group_by_priority(prioritized_fields);

        // 4. Get sorted priorities (descending order)
        let mut priorities: Vec<Priority> = grouped.keys().copied().collect();
        priorities = sort_priorities(priorities);

        // 5. Generate patch frames for each priority level (above threshold)
        for priority in priorities {
            if priority < min_priority {
                continue; // Skip priorities below threshold
            }

            if let Some(fields) = grouped.get(&priority) {
                // Create patches for this priority level
                let patches: Result<Vec<FramePatch>, String> = fields
                    .iter()
                    .map(|field| Ok(FramePatch::set(field.path.clone(), field.value.clone())))
                    .collect();

                let patches = patches?;

                if !patches.is_empty() {
                    // Create patch frame
                    let frame = Frame::patch(stream_id, sequence, priority, patches)
                        .map_err(|e| format!("Failed to create patch frame: {:?}", e))?;

                    frames.push(frame);
                    sequence += 1;
                }
            }
        }

        // 6. Add completion frame (always last, critical priority)
        frames.push(Frame::complete(stream_id, sequence, None));

        Ok(frames)
    }

    /// Create skeleton structure from data (internal helper)
    ///
    /// Generates a skeleton with the same structure but null/empty values.
    fn create_skeleton(data: &JsonData) -> JsonData {
        match data {
            JsonData::Object(map) => {
                let skeleton_map: HashMap<String, JsonData> = map
                    .iter()
                    .map(|(k, v)| {
                        let skeleton_value = match v {
                            JsonData::Object(_) => Self::create_skeleton(v),
                            JsonData::Array(_) => JsonData::Array(vec![]),
                            JsonData::String(_) => JsonData::Null,
                            JsonData::Integer(_) => JsonData::Integer(0),
                            JsonData::Float(_) => JsonData::Float(0.0),
                            JsonData::Bool(_) => JsonData::Bool(false),
                            JsonData::Null => JsonData::Null,
                        };
                        (k.clone(), skeleton_value)
                    })
                    .collect();
                JsonData::Object(skeleton_map)
            }
            JsonData::Array(_) => JsonData::Array(vec![]),
            _ => JsonData::Null,
        }
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
    use pjs_domain::entities::frame::FrameType;

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
    fn test_parser_with_config() {
        let config = PriorityConfigBuilder::new().add_critical_field("custom_id".to_string());
        let _parser = PjsParser::with_config(config);
        // Parser created successfully with custom config
    }

    #[test]
    fn test_version() {
        let version = PjsParser::version();
        assert!(!version.is_empty());
        assert_eq!(version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn test_create_skeleton_simple_object() {
        let mut obj = HashMap::new();
        obj.insert("name".to_string(), JsonData::String("Alice".to_string()));
        obj.insert("age".to_string(), JsonData::Integer(30));
        let data = JsonData::Object(obj);

        let skeleton = PjsParser::create_skeleton(&data);

        if let JsonData::Object(map) = skeleton {
            assert_eq!(map.get("name"), Some(&JsonData::Null));
            assert_eq!(map.get("age"), Some(&JsonData::Integer(0)));
        } else {
            panic!("Expected object skeleton");
        }
    }

    #[test]
    fn test_create_skeleton_nested_object() {
        let mut inner = HashMap::new();
        inner.insert("city".to_string(), JsonData::String("NYC".to_string()));
        let mut outer = HashMap::new();
        outer.insert("address".to_string(), JsonData::Object(inner));
        let data = JsonData::Object(outer);

        let skeleton = PjsParser::create_skeleton(&data);

        if let JsonData::Object(map) = skeleton {
            if let Some(JsonData::Object(inner_map)) = map.get("address") {
                assert_eq!(inner_map.get("city"), Some(&JsonData::Null));
            } else {
                panic!("Expected nested object skeleton");
            }
        } else {
            panic!("Expected object skeleton");
        }
    }

    #[test]
    fn test_create_skeleton_array() {
        let data = JsonData::Array(vec![JsonData::Integer(1), JsonData::Integer(2)]);

        let skeleton = PjsParser::create_skeleton(&data);
        assert_eq!(skeleton, JsonData::Array(vec![]));
    }

    #[test]
    fn test_generate_frames_internal_simple() {
        let parser = PjsParser::new();
        let stream_id = StreamId::new();

        let mut obj = HashMap::new();
        obj.insert("id".to_string(), JsonData::Integer(1));
        obj.insert("name".to_string(), JsonData::String("Alice".to_string()));
        let data = JsonData::Object(obj);

        let frames = parser
            .generate_frames_internal(&data, stream_id, Priority::LOW)
            .expect("Frame generation failed");

        // Should have: skeleton + patch frames + complete
        assert!(frames.len() >= 3, "Expected at least 3 frames");

        // First frame should be skeleton
        assert_eq!(frames[0].frame_type(), &FrameType::Skeleton);

        // Last frame should be complete
        assert_eq!(frames.last().unwrap().frame_type(), &FrameType::Complete);

        // Middle frames should be patches
        for frame in frames.iter().skip(1).take(frames.len() - 2) {
            assert_eq!(frame.frame_type(), &FrameType::Patch);
        }
    }

    #[test]
    fn test_generate_frames_internal_priority_ordering() {
        let parser = PjsParser::new();
        let stream_id = StreamId::new();

        let mut obj = HashMap::new();
        obj.insert("id".to_string(), JsonData::Integer(1)); // Critical
        obj.insert("name".to_string(), JsonData::String("Alice".to_string())); // High
        obj.insert("bio".to_string(), JsonData::String("Developer".to_string())); // Medium
        let data = JsonData::Object(obj);

        let frames = parser
            .generate_frames_internal(&data, stream_id, Priority::LOW)
            .expect("Frame generation failed");

        // Verify frames are ordered by priority (descending)
        let mut prev_priority = Priority::CRITICAL;
        for frame in frames.iter().skip(1).take(frames.len() - 2) {
            let current_priority = frame.priority();
            assert!(
                current_priority <= prev_priority,
                "Frames should be ordered by descending priority"
            );
            prev_priority = current_priority;
        }
    }

    #[test]
    fn test_generate_frames_internal_min_priority_filter() {
        let parser = PjsParser::new();
        let stream_id = StreamId::new();

        let mut obj = HashMap::new();
        obj.insert("id".to_string(), JsonData::Integer(1)); // Critical (100)
        obj.insert("name".to_string(), JsonData::String("Alice".to_string())); // High (80)
        obj.insert("logs".to_string(), JsonData::Array(vec![])); // Background (10)
        let data = JsonData::Object(obj);

        // Set minimum priority to MEDIUM (50), should exclude background fields
        let frames = parser
            .generate_frames_internal(&data, stream_id, Priority::MEDIUM)
            .expect("Frame generation failed");

        // Verify no frames with priority below MEDIUM
        for frame in frames.iter().skip(1).take(frames.len() - 2) {
            assert!(
                frame.priority() >= Priority::MEDIUM,
                "Frames below MEDIUM priority should be filtered out"
            );
        }
    }

    #[test]
    fn test_generate_frames_validates_sequence() {
        let parser = PjsParser::new();
        let stream_id = StreamId::new();

        let mut obj = HashMap::new();
        obj.insert("id".to_string(), JsonData::Integer(1));
        let data = JsonData::Object(obj);

        let frames = parser
            .generate_frames_internal(&data, stream_id, Priority::LOW)
            .expect("Frame generation failed");

        // Verify sequence numbers are incrementing
        for (i, frame) in frames.iter().enumerate() {
            assert_eq!(
                frame.sequence(),
                i as u64,
                "Sequence numbers should increment from 0"
            );
        }
    }

    // Note: Additional parsing tests require WASM environment and should be run with
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

    #[wasm_bindgen_test]
    fn test_generate_frames_simple() {
        let parser = PjsParser::new();
        let result = parser.generate_frames(r#"{"id": 1, "name": "Alice"}"#, 10);
        assert!(result.is_ok());
    }

    #[wasm_bindgen_test]
    fn test_generate_frames_with_priority_threshold() {
        let parser = PjsParser::new();
        let result = parser.generate_frames(
            r#"{"id": 1, "name": "Alice", "bio": "Developer"}"#,
            50, // MEDIUM threshold
        );
        assert!(result.is_ok());
    }

    #[wasm_bindgen_test]
    fn test_generate_frames_invalid_json() {
        let parser = PjsParser::new();
        let result = parser.generate_frames(r#"{"invalid"#, 10);
        assert!(result.is_err());
    }

    #[wasm_bindgen_test]
    fn test_generate_frames_invalid_priority() {
        let parser = PjsParser::new();
        let result = parser.generate_frames(r#"{"id": 1}"#, 0); // Priority cannot be 0
        assert!(result.is_err());
    }

    #[wasm_bindgen_test]
    fn test_parser_with_custom_config() {
        let config = PriorityConfigBuilder::new()
            .add_critical_field("user_id".to_string())
            .add_high_field("display_name".to_string());

        let parser = PjsParser::with_config(config);
        let result = parser.generate_frames(r#"{"user_id": 123, "display_name": "Alice"}"#, 10);
        assert!(result.is_ok());
    }

    #[wasm_bindgen_test]
    fn test_version() {
        let version = PjsParser::version();
        assert!(!version.is_empty());
    }

    #[wasm_bindgen_test]
    fn test_priority_constants() {
        use crate::priority_constants::PriorityConstants;

        assert_eq!(PriorityConstants::CRITICAL(), 100);
        assert_eq!(PriorityConstants::HIGH(), 80);
        assert_eq!(PriorityConstants::MEDIUM(), 50);
        assert_eq!(PriorityConstants::LOW(), 25);
        assert_eq!(PriorityConstants::BACKGROUND(), 10);
    }

    #[wasm_bindgen_test]
    fn test_complex_nested_structure() {
        let parser = PjsParser::new();
        let json = r#"{
            "id": 1,
            "name": "Alice",
            "address": {
                "street": "Main St",
                "city": "NYC"
            },
            "posts": [
                {"title": "First Post", "likes": 10},
                {"title": "Second Post", "likes": 5}
            ],
            "analytics": {
                "views": 1000,
                "clicks": 50
            }
        }"#;

        let result = parser.generate_frames(json, 25); // LOW threshold
        assert!(result.is_ok());
    }
}
