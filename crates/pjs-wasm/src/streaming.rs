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
use pjs_domain::entities::frame::FrameType;
use pjs_domain::entities::Frame;
use pjs_domain::value_objects::{JsonData, Priority, StreamId};
use std::collections::HashMap;
use wasm_bindgen::prelude::*;

/// Statistics about a completed stream.
#[wasm_bindgen]
#[derive(Debug, Clone)]
pub struct StreamStats {
    /// Total number of frames generated
    total_frames: u32,
    /// Number of patch frames
    patch_frames: u32,
    /// Total bytes processed
    bytes_processed: u32,
    /// Time taken in milliseconds
    duration_ms: f64,
}

#[wasm_bindgen]
impl StreamStats {
    /// Get the total number of frames generated.
    #[wasm_bindgen(getter, js_name = totalFrames)]
    pub fn total_frames(&self) -> u32 {
        self.total_frames
    }

    /// Get the number of patch frames.
    #[wasm_bindgen(getter, js_name = patchFrames)]
    pub fn patch_frames(&self) -> u32 {
        self.patch_frames
    }

    /// Get the total bytes processed.
    #[wasm_bindgen(getter, js_name = bytesProcessed)]
    pub fn bytes_processed(&self) -> u32 {
        self.bytes_processed
    }

    /// Get the duration in milliseconds.
    #[wasm_bindgen(getter, js_name = durationMs)]
    pub fn duration_ms(&self) -> f64 {
        self.duration_ms
    }
}

/// Frame data exposed to JavaScript.
///
/// This struct wraps frame information in a JavaScript-friendly format.
#[wasm_bindgen]
#[derive(Debug, Clone)]
pub struct FrameData {
    /// Frame type: "skeleton", "patch", "complete", or "error"
    frame_type: String,
    /// Frame sequence number
    sequence: u64,
    /// Priority level (1-255)
    priority: u8,
    /// Frame payload as JSON string
    payload: String,
}

#[wasm_bindgen]
impl FrameData {
    /// Get the frame type.
    #[wasm_bindgen(getter, js_name = type)]
    pub fn frame_type(&self) -> String {
        self.frame_type.clone()
    }

    /// Get the sequence number.
    #[wasm_bindgen(getter)]
    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Get the priority level.
    #[wasm_bindgen(getter)]
    pub fn priority(&self) -> u8 {
        self.priority
    }

    /// Get the payload as JSON string.
    #[wasm_bindgen(getter)]
    pub fn payload(&self) -> String {
        self.payload.clone()
    }

    /// Get the payload as a JavaScript object.
    #[wasm_bindgen(js_name = getPayloadObject)]
    pub fn get_payload_object(&self) -> Result<JsValue, JsValue> {
        let value: serde_json::Value = serde_json::from_str(&self.payload)
            .map_err(|e| JsValue::from_str(&format!("Failed to parse payload: {}", e)))?;
        serde_wasm_bindgen::to_value(&value)
            .map_err(|e| JsValue::from_str(&format!("Failed to convert payload: {}", e)))
    }
}

impl From<&Frame> for FrameData {
    fn from(frame: &Frame) -> Self {
        let frame_type = match frame.frame_type() {
            FrameType::Skeleton => "skeleton",
            FrameType::Patch => "patch",
            FrameType::Complete => "complete",
            FrameType::Error => "error",
        };

        // Serialize frame payload to JSON
        let payload = serde_json::to_string(frame.payload())
            .unwrap_or_else(|_| "null".to_string());

        Self {
            frame_type: frame_type.to_string(),
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
///
/// # Example
///
/// ```javascript
/// const stream = new PriorityStream();
///
/// stream.onFrame((frame) => {
///     if (frame.type === 'skeleton') {
///         renderSkeleton(frame.getPayloadObject());
///     } else if (frame.type === 'patch') {
///         applyPatch(frame.getPayloadObject());
///     }
/// });
///
/// stream.start(jsonString);
/// ```
#[wasm_bindgen]
pub struct PriorityStream {
    priority_assigner: PriorityAssigner,
    min_priority: u8,
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
            on_frame: None,
            on_complete: None,
            on_error: None,
        }
    }

    /// Set the minimum priority threshold.
    ///
    /// Frames with priority below this threshold will not be delivered.
    ///
    /// # Arguments
    ///
    /// * `priority` - Minimum priority (1-255)
    ///
    /// # Returns
    ///
    /// Self for method chaining
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
    /// * `callback` - JavaScript function(frame: FrameData)
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
    /// * `callback` - JavaScript function(stats: StreamStats)
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
    /// * `callback` - JavaScript function(error: string)
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
        use pjs_domain::entities::frame::FramePatch;

        let mut frames = Vec::new();
        let mut sequence = 0u64;

        // 1. Generate skeleton frame
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

        // 5. Generate patch frames for each priority level
        for priority in priorities {
            if priority < min_priority {
                continue;
            }

            if let Some(fields) = grouped.get(&priority) {
                let patches: Result<Vec<FramePatch>, String> = fields
                    .iter()
                    .map(|field| Ok(FramePatch::set(field.path.clone(), field.value.clone())))
                    .collect();

                let patches = patches?;

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

    /// Create skeleton structure from data
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

    /// Emit frame to JavaScript callback
    fn emit_frame(&self, frame: &Frame) {
        if let Some(ref callback) = self.on_frame {
            let frame_data = FrameData::from(frame);
            let this = JsValue::null();
            let _ = callback.call1(&this, &JsValue::from(frame_data));
        }
    }

    /// Emit completion to JavaScript callback
    fn emit_complete(&self, stats: StreamStats) {
        if let Some(ref callback) = self.on_complete {
            let this = JsValue::null();
            let _ = callback.call1(&this, &JsValue::from(stats));
        }
    }

    /// Emit error to JavaScript callback
    fn emit_error(&self, error: &str) {
        if let Some(ref callback) = self.on_error {
            let this = JsValue::null();
            let _ = callback.call1(&this, &JsValue::from_str(error));
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
    fn test_stream_stats_getters() {
        let stats = StreamStats {
            total_frames: 5,
            patch_frames: 3,
            bytes_processed: 1024,
            duration_ms: 10.5,
        };

        assert_eq!(stats.total_frames(), 5);
        assert_eq!(stats.patch_frames(), 3);
        assert_eq!(stats.bytes_processed(), 1024);
        assert!((stats.duration_ms() - 10.5).abs() < 0.001);
    }

    #[test]
    fn test_frame_data_from_frame() {
        let stream_id = StreamId::new();
        let skeleton_data = JsonData::Object(std::collections::HashMap::new());
        let frame = Frame::skeleton(stream_id, 0, skeleton_data);

        let frame_data = FrameData::from(&frame);
        assert_eq!(frame_data.frame_type(), "skeleton");
        assert_eq!(frame_data.sequence(), 0);
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
    fn test_frame_data_payload() {
        let stream_id = StreamId::new();
        let mut obj = std::collections::HashMap::new();
        obj.insert("test".to_string(), JsonData::String("value".to_string()));
        let data = JsonData::Object(obj);
        let frame = Frame::skeleton(stream_id, 0, data);

        let frame_data = FrameData::from(&frame);
        assert!(!frame_data.payload().is_empty());

        let payload_obj = frame_data.get_payload_object();
        assert!(payload_obj.is_ok());
    }
}
