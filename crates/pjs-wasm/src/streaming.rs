//! Streaming API for progressive frame delivery in WebAssembly.
//!
//! This module provides a callback-based streaming interface that allows
//! JavaScript applications to receive frames progressively as they are
//! generated, enabling immediate UI updates for high-priority data.
//!
//! # Example
//!
//! ```javascript
//! import { PriorityStream } from 'pjs-wasm';
//!
//! const stream = new PriorityStream();
//!
//! stream.onFrame((frame) => {
//!     console.log('Received frame:', frame.type, 'priority:', frame.priority);
//!     updateUI(frame);
//! });
//!
//! stream.onComplete((stats) => {
//!     console.log('Stream complete:', stats);
//! });
//!
//! stream.onError((error) => {
//!     console.error('Stream error:', error);
//! });
//!
//! stream.start('{"id": 1, "name": "Alice", "bio": "..."}');
//! ```

use crate::priority_assignment::PriorityAssigner;
use crate::priority_config::PriorityConfigBuilder;
use crate::security::{SecurityConfig, validate_input_size};
use pjson_rs_domain::entities::Frame;
use pjson_rs_domain::entities::frame::FrameType;
use pjson_rs_domain::value_objects::{JsonData, Priority, StreamId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(typescript_custom_section)]
const TS_CALLBACK_TYPES: &str = r#"
export type FrameCallback = (frame: FrameData) => void;
export type StreamStatsCallback = (stats: StreamStats) => void;
export type ErrorCallback = (error: string) => void;
"#;

/// Statistics about a completed stream.
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct StreamStats {
    /// Total number of frames generated
    pub total_frames: u32,
    /// Number of patch frames
    pub patch_frames: u32,
    /// Total bytes processed
    pub bytes_processed: u32,
    /// Time taken in milliseconds
    pub duration_ms: f64,
}

/// Frame data exposed to JavaScript.
///
/// This struct wraps frame information in a JavaScript-friendly format.
/// The `type` field contains one of: `"skeleton"`, `"patch"`, `"complete"`, `"error"`.
/// The `payload` field contains the frame data as a JSON string.
#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct FrameData {
    /// Frame type: "skeleton", "patch", "complete", or "error"
    #[serde(rename = "type")]
    pub frame_type: String,
    /// Frame sequence number
    pub sequence: u64,
    /// Priority level (1-255)
    pub priority: u8,
    /// Frame payload as JSON string
    pub payload: String,
}

impl From<&Frame> for FrameData {
    fn from(frame: &Frame) -> Self {
        let frame_type = match frame.frame_type() {
            FrameType::Skeleton => "skeleton".to_string(),
            FrameType::Patch => "patch".to_string(),
            FrameType::Complete => "complete".to_string(),
            FrameType::Error => "error".to_string(),
        };

        let payload = serde_json::to_string(frame.payload()).unwrap_or_else(|_| "null".to_string());

        Self {
            frame_type,
            sequence: frame.sequence(),
            priority: frame.priority().value(),
            payload,
        }
    }
}

/// Priority-based streaming parser for WebAssembly.
///
/// This class provides a callback-based streaming interface that delivers
/// frames progressively based on their priority. High-priority frames are
/// delivered first, allowing immediate UI updates.
///
/// # Features
///
/// - Callback-based frame delivery (`onFrame`, `onComplete`, `onError`)
/// - Configurable priority thresholds
/// - Stream statistics tracking
/// - Custom priority configuration support
/// - **Security limits** to prevent DoS attacks
///
/// # Example
///
/// ```javascript
/// const stream = new PriorityStream();
///
/// stream.onFrame((frame) => {
///     if (frame.type === 'skeleton') {
///         renderSkeleton(JSON.parse(frame.payload));
///     } else if (frame.type === 'patch') {
///         applyPatch(JSON.parse(frame.payload));
///     }
/// });
///
/// stream.start(jsonString);
/// ```
#[wasm_bindgen]
pub struct PriorityStream {
    priority_assigner: PriorityAssigner,
    min_priority: u8,
    security_config: SecurityConfig,
    on_frame: Option<js_sys::Function>,
    on_complete: Option<js_sys::Function>,
    on_error: Option<js_sys::Function>,
}

#[wasm_bindgen]
impl PriorityStream {
    /// Create a new PriorityStream with default configuration.
    ///
    /// # Example
    ///
    /// ```javascript
    /// const stream = new PriorityStream();
    /// ```
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            priority_assigner: PriorityAssigner::new(),
            min_priority: 1,
            security_config: SecurityConfig::default(),
            on_frame: None,
            on_complete: None,
            on_error: None,
        }
    }

    /// Create a PriorityStream with custom priority configuration.
    ///
    /// # Arguments
    ///
    /// * `config_builder` - Custom priority configuration
    ///
    /// # Example
    ///
    /// ```javascript
    /// const config = new PriorityConfigBuilder()
    ///     .addCriticalField('user_id');
    /// const stream = PriorityStream.withConfig(config);
    /// ```
    #[wasm_bindgen(js_name = withConfig)]
    pub fn with_config(config_builder: PriorityConfigBuilder) -> Self {
        let config = config_builder.build_internal();
        Self {
            priority_assigner: PriorityAssigner::with_config(config),
            min_priority: 1,
            security_config: SecurityConfig::default(),
            on_frame: None,
            on_complete: None,
            on_error: None,
        }
    }

    /// Set custom security limits.
    ///
    /// # Arguments
    ///
    /// * `config` - Security configuration with custom limits
    ///
    /// # Example
    ///
    /// ```javascript
    /// const security = new SecurityConfig()
    ///     .setMaxJsonSize(5 * 1024 * 1024)  // 5 MB
    ///     .setMaxDepth(32);
    /// stream.setSecurityConfig(security);
    /// ```
    #[wasm_bindgen(js_name = setSecurityConfig)]
    pub fn set_security_config(&mut self, config: SecurityConfig) {
        self.security_config = config;
    }

    /// Set the minimum priority threshold.
    ///
    /// Frames with priority below this threshold will not be delivered.
    ///
    /// # Arguments
    ///
    /// * `priority` - Minimum priority (1-255)
    ///
    /// # Example
    ///
    /// ```javascript
    /// stream.setMinPriority(50); // Only deliver MEDIUM and above
    /// ```
    #[wasm_bindgen(js_name = setMinPriority)]
    pub fn set_min_priority(&mut self, priority: u8) -> Result<(), JsValue> {
        if priority == 0 {
            return Err(JsValue::from_str("Priority must be between 1 and 255"));
        }
        self.min_priority = priority;
        Ok(())
    }

    /// Register a callback for frame events.
    ///
    /// The callback receives a `FrameData` object for each generated frame.
    ///
    /// # Arguments
    ///
    /// * `callback` - `(frame: FrameData) => void`
    ///
    /// # Example
    ///
    /// ```javascript
    /// stream.onFrame((frame) => {
    ///     console.log(`Frame ${frame.sequence}: ${frame.type}`);
    /// });
    /// ```
    #[wasm_bindgen(js_name = onFrame)]
    pub fn on_frame(&mut self, callback: js_sys::Function) {
        self.on_frame = Some(callback);
    }

    /// Register a callback for stream completion.
    ///
    /// The callback receives a `StreamStats` object with statistics.
    ///
    /// # Arguments
    ///
    /// * `callback` - `(stats: StreamStats) => void`
    ///
    /// # Example
    ///
    /// ```javascript
    /// stream.onComplete((stats) => {
    ///     console.log(`Completed in ${stats.durationMs}ms`);
    /// });
    /// ```
    #[wasm_bindgen(js_name = onComplete)]
    pub fn on_complete(&mut self, callback: js_sys::Function) {
        self.on_complete = Some(callback);
    }

    /// Register a callback for errors.
    ///
    /// The callback receives an error message string.
    ///
    /// # Arguments
    ///
    /// * `callback` - `(error: string) => void`
    ///
    /// # Example
    ///
    /// ```javascript
    /// stream.onError((error) => {
    ///     console.error('Stream error:', error);
    /// });
    /// ```
    #[wasm_bindgen(js_name = onError)]
    pub fn on_error(&mut self, callback: js_sys::Function) {
        self.on_error = Some(callback);
    }

    /// Start streaming frames from JSON data.
    ///
    /// This method parses the JSON, generates frames ordered by priority,
    /// and delivers them via the registered callbacks.
    ///
    /// # Arguments
    ///
    /// * `json_str` - JSON string to parse and stream
    ///
    /// # Example
    ///
    /// ```javascript
    /// stream.start('{"id": 1, "name": "Alice", "bio": "..."}');
    /// ```
    #[wasm_bindgen]
    pub fn start(&self, json_str: &str) -> Result<(), JsValue> {
        let start_time = js_sys::Date::now();
        let bytes_processed = json_str.len() as u32;

        // Security: Validate input size
        if let Err(e) = validate_input_size(json_str, &self.security_config) {
            let error_msg = e.to_string();
            self.emit_error(&error_msg);
            return Err(JsValue::from_str(&error_msg));
        }

        // Parse JSON
        let value: serde_json::Value = match serde_json::from_str(json_str) {
            Ok(v) => v,
            Err(e) => {
                self.emit_error(&format!("Parse error: {}", e));
                return Err(JsValue::from_str(&format!("Parse error: {}", e)));
            }
        };

        let json_data: JsonData = value.into();

        // Generate stream ID
        let stream_id = StreamId::from_string("00000000-0000-0000-0000-000000000001")
            .map_err(|e| JsValue::from_str(&format!("StreamId error: {:?}", e)))?;

        // Generate frames
        let min_priority = Priority::new(self.min_priority)
            .map_err(|e| JsValue::from_str(&format!("Invalid priority: {:?}", e)))?;

        let frames = match self.generate_frames_internal(&json_data, stream_id, min_priority) {
            Ok(f) => f,
            Err(e) => {
                self.emit_error(&e);
                return Err(JsValue::from_str(&e));
            }
        };

        // Deliver frames via callbacks
        let mut patch_count = 0u32;
        for frame in &frames {
            if matches!(frame.frame_type(), FrameType::Patch) {
                patch_count += 1;
            }
            self.emit_frame(frame);
        }

        // Calculate duration
        let duration_ms = js_sys::Date::now() - start_time;

        // Emit completion
        let stats = StreamStats {
            total_frames: frames.len() as u32,
            patch_frames: patch_count,
            bytes_processed,
            duration_ms,
        };
        self.emit_complete(stats);

        Ok(())
    }

    /// Internal frame generation (reused from parser)
    fn generate_frames_internal(
        &self,
        data: &JsonData,
        stream_id: StreamId,
        min_priority: Priority,
    ) -> Result<Vec<Frame>, String> {
        use crate::priority_assignment::{group_by_priority, sort_priorities};
        use pjson_rs_domain::entities::frame::FramePatch;

        let max_depth = self.security_config.max_depth();

        // Pre-allocate frames Vec with estimated capacity
        // Typical: 1 skeleton + ~2-4 priority groups + 1 complete = ~4-6 frames
        // Conservative estimate to avoid over-allocation
        let mut frames = Vec::with_capacity(6);
        let mut sequence = 0u64;

        // 1. Generate skeleton frame (with depth limit)
        let skeleton = Self::create_skeleton_with_limit(data, 0, max_depth);
        frames.push(Frame::skeleton(stream_id, sequence, skeleton));
        sequence += 1;

        // 2. Extract all fields with priorities (with depth limit)
        let prioritized_fields = self
            .priority_assigner
            .extract_prioritized_fields_with_limit(data, max_depth);

        // 3. Group fields by priority level
        let grouped = group_by_priority(prioritized_fields);

        // 4. Get sorted priorities (descending order)
        let mut priorities: Vec<Priority> = grouped.keys().copied().collect();
        priorities = sort_priorities(priorities);

        // 5. Generate patch frames for each priority level
        for priority in priorities {
            if priority < min_priority {
                continue;
            }

            if let Some(fields) = grouped.get(&priority) {
                // Pre-allocate patches Vec with exact capacity
                let mut patches = Vec::with_capacity(fields.len());
                for field in fields.iter() {
                    patches.push(FramePatch::set(field.path.clone(), field.value.clone()));
                }

                if !patches.is_empty() {
                    let frame = Frame::patch(stream_id, sequence, priority, patches)
                        .map_err(|e| format!("Failed to create patch frame: {:?}", e))?;

                    frames.push(frame);
                    sequence += 1;
                }
            }
        }

        // 6. Add completion frame
        frames.push(Frame::complete(stream_id, sequence, None));

        Ok(frames)
    }

    /// Create skeleton structure from data (with depth limit)
    fn create_skeleton_with_limit(
        data: &JsonData,
        current_depth: usize,
        max_depth: usize,
    ) -> JsonData {
        // Security: Stop recursion at max depth
        if current_depth >= max_depth {
            return JsonData::Null;
        }

        match data {
            JsonData::Object(map) => {
                // Pre-allocate HashMap with exact capacity to avoid reallocations
                let mut skeleton_map = HashMap::with_capacity(map.len());

                for (k, v) in map.iter() {
                    let skeleton_value = match v {
                        JsonData::Object(_) => {
                            Self::create_skeleton_with_limit(v, current_depth + 1, max_depth)
                        }
                        JsonData::Array(_) => JsonData::Array(vec![]),
                        JsonData::String(_) => JsonData::Null,
                        JsonData::Integer(_) => JsonData::Integer(0),
                        JsonData::Float(_) => JsonData::Float(0.0),
                        JsonData::Bool(_) => JsonData::Bool(false),
                        JsonData::Null => JsonData::Null,
                    };
                    skeleton_map.insert(k.clone(), skeleton_value);
                }

                JsonData::Object(skeleton_map)
            }
            JsonData::Array(_) => JsonData::Array(vec![]),
            _ => JsonData::Null,
        }
    }

    /// Emit frame to JavaScript callback
    fn emit_frame(&self, frame: &Frame) {
        if let Some(ref callback) = self.on_frame
            && let Ok(js_val) = serde_wasm_bindgen::to_value(&FrameData::from(frame))
        {
            let _ = callback.call1(&JsValue::null(), &js_val);
        }
    }

    /// Emit completion to JavaScript callback
    fn emit_complete(&self, stats: StreamStats) {
        if let Some(ref callback) = self.on_complete
            && let Ok(js_val) = serde_wasm_bindgen::to_value(&stats)
        {
            let _ = callback.call1(&JsValue::null(), &js_val);
        }
    }

    /// Emit error to JavaScript callback
    fn emit_error(&self, error: &str) {
        if let Some(ref callback) = self.on_error {
            let _ = callback.call1(&JsValue::null(), &JsValue::from_str(error));
        }
    }
}

impl Default for PriorityStream {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_stream_creation() {
        let stream = PriorityStream::new();
        assert_eq!(stream.min_priority, 1);
    }

    #[test]
    fn test_set_min_priority() {
        let mut stream = PriorityStream::new();
        assert!(stream.set_min_priority(50).is_ok());
        assert_eq!(stream.min_priority, 50);
    }

    // Note: test_set_min_priority_invalid is in wasm_tests since it uses JsValue

    #[test]
    fn test_stream_stats_fields() {
        let stats = StreamStats {
            total_frames: 5,
            patch_frames: 3,
            bytes_processed: 1024,
            duration_ms: 10.5,
        };

        assert_eq!(stats.total_frames, 5);
        assert_eq!(stats.patch_frames, 3);
        assert_eq!(stats.bytes_processed, 1024);
        assert!((stats.duration_ms - 10.5).abs() < 0.001);
    }

    #[test]
    fn test_frame_data_from_frame() {
        let stream_id = StreamId::new();
        let skeleton_data = JsonData::Object(std::collections::HashMap::new());
        let frame = Frame::skeleton(stream_id, 0, skeleton_data);

        let frame_data = FrameData::from(&frame);
        assert_eq!(frame_data.frame_type, "skeleton");
        assert_eq!(frame_data.sequence, 0);
    }

    #[test]
    fn test_generate_frames_internal() {
        let stream = PriorityStream::new();
        let stream_id = StreamId::new();

        let mut obj = std::collections::HashMap::new();
        obj.insert("id".to_string(), JsonData::Integer(1));
        obj.insert("name".to_string(), JsonData::String("Test".to_string()));
        let data = JsonData::Object(obj);

        let frames = stream
            .generate_frames_internal(&data, stream_id, Priority::LOW)
            .expect("Frame generation failed");

        assert!(frames.len() >= 3); // skeleton + patches + complete
        assert!(matches!(frames[0].frame_type(), FrameType::Skeleton));
        assert!(matches!(
            frames.last().unwrap().frame_type(),
            FrameType::Complete
        ));
    }

    #[test]
    fn test_frame_data_serde_roundtrip() {
        let original = FrameData {
            frame_type: "skeleton".to_string(),
            sequence: 42,
            priority: 100,
            payload: r#"{"id":1}"#.to_string(),
        };

        let json = serde_json::to_string(&original).expect("serialize failed");
        let restored: FrameData = serde_json::from_str(&json).expect("deserialize failed");

        assert_eq!(restored.frame_type, original.frame_type);
        assert_eq!(restored.sequence, original.sequence);
        assert_eq!(restored.priority, original.priority);
        assert_eq!(restored.payload, original.payload);
    }

    #[test]
    fn test_frame_data_serde_type_field_rename() {
        let frame_data = FrameData {
            frame_type: "patch".to_string(),
            sequence: 1,
            priority: 50,
            payload: "null".to_string(),
        };

        let json = serde_json::to_string(&frame_data).expect("serialize failed");
        let value: serde_json::Value = serde_json::from_str(&json).expect("parse failed");
        assert_eq!(value["type"], "patch");
        assert!(value.get("frame_type").is_none());
    }

    #[test]
    fn test_stream_stats_serde_camel_case() {
        let stats = StreamStats {
            total_frames: 3,
            patch_frames: 1,
            bytes_processed: 512,
            duration_ms: 5.0,
        };

        let json = serde_json::to_string(&stats).expect("serialize failed");
        let value: serde_json::Value = serde_json::from_str(&json).expect("parse failed");
        assert_eq!(value["totalFrames"], 3);
        assert_eq!(value["patchFrames"], 1);
        assert!(value.get("total_frames").is_none());
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
mod wasm_tests {
    use super::*;
    use wasm_bindgen_test::*;

    #[wasm_bindgen_test]
    fn test_priority_stream_start() {
        let stream = PriorityStream::new();
        let result = stream.start(r#"{"id": 1, "name": "Alice"}"#);
        assert!(result.is_ok());
    }

    #[wasm_bindgen_test]
    fn test_set_min_priority_invalid() {
        let mut stream = PriorityStream::new();
        assert!(stream.set_min_priority(0).is_err());
    }

    #[wasm_bindgen_test]
    fn test_priority_stream_invalid_json() {
        let stream = PriorityStream::new();
        let result = stream.start(r#"{"invalid"#);
        assert!(result.is_err());
    }

    #[wasm_bindgen_test]
    fn test_stream_with_config() {
        let config = PriorityConfigBuilder::new().add_critical_field("user_id".to_string());
        let stream = PriorityStream::with_config(config);
        let result = stream.start(r#"{"user_id": 123}"#);
        assert!(result.is_ok());
    }

    #[wasm_bindgen_test]
    fn test_frame_data_payload_not_empty() {
        let stream_id = StreamId::new();
        let mut obj = std::collections::HashMap::new();
        obj.insert("test".to_string(), JsonData::String("value".to_string()));
        let data = JsonData::Object(obj);
        let frame = Frame::skeleton(stream_id, 0, data);

        let frame_data = FrameData::from(&frame);
        assert!(!frame_data.payload.is_empty());
    }
}
