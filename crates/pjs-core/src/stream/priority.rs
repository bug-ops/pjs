//! Priority-based JSON streaming implementation
//!
//! This module implements the core Priority JSON Streaming protocol with:
//! - Skeleton-first approach
//! - JSON Path based patching  
//! - Priority-based field ordering
//! - Incremental reconstruction

use crate::Result;
use serde_json::{Map as JsonMap, Value as JsonValue};
use smallvec::SmallVec;
use std::collections::VecDeque;

/// Priority levels for JSON fields and structures
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Critical = 100,  // ID fields, status, essential metadata
    High = 80,       // Names, titles, key identifiers
    Medium = 50,     // Regular content, descriptions
    Low = 20,        // Analytics, detailed metadata
    Background = 10, // Large arrays, reviews, logs
}

/// JSON Path for addressing specific nodes in the JSON structure
#[derive(Debug, Clone, PartialEq)]
pub struct JsonPath {
    segments: SmallVec<[PathSegment; 8]>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PathSegment {
    Root,
    Key(String),
    Index(usize),
    Wildcard,
}

/// Patch operation for updating JSON structure
#[derive(Debug, Clone)]
pub struct JsonPatch {
    pub path: JsonPath,
    pub operation: PatchOperation,
    pub priority: Priority,
}

#[derive(Debug, Clone)]
pub enum PatchOperation {
    Set { value: JsonValue },
    Append { values: Vec<JsonValue> },
    Replace { value: JsonValue },
    Remove,
}

/// Streaming frame containing skeleton or patch data
#[derive(Debug, Clone)]
pub enum StreamFrame {
    Skeleton {
        data: JsonValue,
        priority: Priority,
        complete: bool,
    },
    Patch {
        patches: Vec<JsonPatch>,
        priority: Priority,
    },
    Complete {
        checksum: Option<u64>,
    },
}

/// Priority-based JSON streamer
pub struct PriorityStreamer {
    config: StreamerConfig,
}

#[derive(Debug, Clone)]
pub struct StreamerConfig {
    pub detect_semantics: bool,
    pub max_patch_size: usize,
    pub priority_threshold: Priority,
}

impl Default for StreamerConfig {
    fn default() -> Self {
        Self {
            detect_semantics: true,
            max_patch_size: 100,
            priority_threshold: Priority::Low,
        }
    }
}

impl PriorityStreamer {
    /// Create new priority streamer
    pub fn new() -> Self {
        Self::with_config(StreamerConfig::default())
    }

    /// Create streamer with custom configuration
    pub fn with_config(config: StreamerConfig) -> Self {
        Self { config }
    }

    /// Analyze JSON and create streaming plan
    pub fn analyze(&self, json: &JsonValue) -> Result<StreamingPlan> {
        let mut plan = StreamingPlan::new();

        // Generate skeleton
        let skeleton = self.generate_skeleton(json);
        plan.frames.push_back(StreamFrame::Skeleton {
            data: skeleton,
            priority: Priority::Critical,
            complete: false,
        });

        // Extract patches by priority
        let mut patches = Vec::new();
        self.extract_patches(json, &JsonPath::root(), &mut patches)?;

        // Group patches by priority
        patches.sort_by(|a, b| b.priority.cmp(&a.priority));

        let mut current_priority = Priority::Critical;
        let mut current_batch = Vec::new();

        for patch in patches {
            if patch.priority != current_priority && !current_batch.is_empty() {
                plan.frames.push_back(StreamFrame::Patch {
                    patches: current_batch,
                    priority: current_priority,
                });
                current_batch = Vec::new();
            }
            current_priority = patch.priority;
            current_batch.push(patch);

            if current_batch.len() >= self.config.max_patch_size {
                plan.frames.push_back(StreamFrame::Patch {
                    patches: current_batch,
                    priority: current_priority,
                });
                current_batch = Vec::new();
            }
        }

        // Add remaining patches
        if !current_batch.is_empty() {
            plan.frames.push_back(StreamFrame::Patch {
                patches: current_batch,
                priority: current_priority,
            });
        }

        // Add completion frame
        plan.frames
            .push_back(StreamFrame::Complete { checksum: None });

        Ok(plan)
    }

    /// Generate skeleton structure with null/empty values
    fn generate_skeleton(&self, json: &JsonValue) -> JsonValue {
        match json {
            JsonValue::Object(map) => {
                let mut skeleton = JsonMap::new();
                for (key, value) in map {
                    skeleton.insert(
                        key.clone(),
                        match value {
                            JsonValue::Array(_) => JsonValue::Array(vec![]),
                            JsonValue::Object(_) => self.generate_skeleton(value),
                            JsonValue::String(_) => JsonValue::Null,
                            JsonValue::Number(_) => JsonValue::Number(0.into()),
                            JsonValue::Bool(_) => JsonValue::Bool(false),
                            JsonValue::Null => JsonValue::Null,
                        },
                    );
                }
                JsonValue::Object(skeleton)
            }
            JsonValue::Array(_) => JsonValue::Array(vec![]),
            _ => JsonValue::Null,
        }
    }

    /// Extract patches from JSON structure
    fn extract_patches(
        &self,
        json: &JsonValue,
        current_path: &JsonPath,
        patches: &mut Vec<JsonPatch>,
    ) -> Result<()> {
        match json {
            JsonValue::Object(map) => {
                for (key, value) in map {
                    let field_path = current_path.append_key(key);
                    let priority = self.calculate_field_priority(&field_path, key, value);

                    // Create patch for this field
                    patches.push(JsonPatch {
                        path: field_path.clone(),
                        operation: PatchOperation::Set {
                            value: value.clone(),
                        },
                        priority,
                    });

                    // Recursively process nested structures
                    self.extract_patches(value, &field_path, patches)?;
                }
            }
            JsonValue::Array(arr) => {
                // For arrays, create append operations in chunks
                if arr.len() > 10 {
                    // Chunk large arrays
                    for chunk in arr.chunks(self.config.max_patch_size) {
                        patches.push(JsonPatch {
                            path: current_path.clone(),
                            operation: PatchOperation::Append {
                                values: chunk.to_vec(),
                            },
                            priority: self.calculate_array_priority(current_path, chunk),
                        });
                    }
                } else if !arr.is_empty() {
                    patches.push(JsonPatch {
                        path: current_path.clone(),
                        operation: PatchOperation::Append {
                            values: arr.clone(),
                        },
                        priority: self.calculate_array_priority(current_path, arr),
                    });
                }
            }
            _ => {
                // Primitive values handled by parent object/array
            }
        }

        Ok(())
    }

    /// Calculate priority for a field based on path and content
    fn calculate_field_priority(&self, _path: &JsonPath, key: &str, value: &JsonValue) -> Priority {
        // Critical fields
        if matches!(key, "id" | "uuid" | "status" | "type" | "kind") {
            return Priority::Critical;
        }

        // High priority fields
        if matches!(key, "name" | "title" | "label" | "email" | "username") {
            return Priority::High;
        }

        // Low priority patterns
        if key.contains("analytics") || key.contains("stats") || key.contains("meta") {
            return Priority::Low;
        }

        if matches!(key, "reviews" | "comments" | "logs" | "history") {
            return Priority::Background;
        }

        // Content-based priority
        match value {
            JsonValue::Array(arr) if arr.len() > 100 => Priority::Background,
            JsonValue::Object(obj) if obj.contains_key("timestamp") => Priority::Medium,
            JsonValue::String(s) if s.len() > 1000 => Priority::Low,
            _ => Priority::Medium,
        }
    }

    /// Calculate priority for array elements
    fn calculate_array_priority(&self, path: &JsonPath, elements: &[JsonValue]) -> Priority {
        // Large arrays get background priority
        if elements.len() > 50 {
            return Priority::Background;
        }

        // Arrays in certain paths get different priorities
        if let Some(last_key) = path.last_key() {
            if matches!(last_key.as_str(), "reviews" | "comments" | "logs") {
                return Priority::Background;
            }
            if matches!(last_key.as_str(), "items" | "data" | "results") {
                return Priority::Medium;
            }
        }

        Priority::Medium
    }
}

/// Plan for streaming JSON with priority ordering
#[derive(Debug)]
pub struct StreamingPlan {
    pub frames: VecDeque<StreamFrame>,
}

impl StreamingPlan {
    pub fn new() -> Self {
        Self {
            frames: VecDeque::new(),
        }
    }

    /// Get next frame to send
    pub fn next_frame(&mut self) -> Option<StreamFrame> {
        self.frames.pop_front()
    }

    /// Check if streaming is complete
    pub fn is_complete(&self) -> bool {
        self.frames.is_empty()
    }

    /// Get remaining frame count
    pub fn remaining_frames(&self) -> usize {
        self.frames.len()
    }

    /// Get iterator over frames
    pub fn frames(&self) -> impl Iterator<Item = &StreamFrame> {
        self.frames.iter()
    }
}

impl JsonPath {
    /// Create root path
    pub fn root() -> Self {
        let mut segments = SmallVec::new();
        segments.push(PathSegment::Root);
        Self { segments }
    }

    /// Append key segment
    pub fn append_key(&self, key: &str) -> Self {
        let mut segments = self.segments.clone();
        segments.push(PathSegment::Key(key.to_string()));
        Self { segments }
    }

    /// Append index segment
    pub fn append_index(&self, index: usize) -> Self {
        let mut segments = self.segments.clone();
        segments.push(PathSegment::Index(index));
        Self { segments }
    }

    /// Get the last key in the path
    pub fn last_key(&self) -> Option<String> {
        self.segments.iter().rev().find_map(|segment| {
            if let PathSegment::Key(key) = segment {
                Some(key.clone())
            } else {
                None
            }
        })
    }

    /// Get segments (read-only)
    pub fn segments(&self) -> &[PathSegment] {
        &self.segments
    }

    /// Get number of segments
    pub fn len(&self) -> usize {
        self.segments.len()
    }

    /// Create JsonPath from segments (for testing)
    pub fn from_segments(segments: SmallVec<[PathSegment; 8]>) -> Self {
        Self { segments }
    }

    /// Convert to JSON Pointer string format
    pub fn to_json_pointer(&self) -> String {
        let mut pointer = String::new();
        for segment in &self.segments {
            match segment {
                PathSegment::Root => {}
                PathSegment::Key(key) => {
                    pointer.push('/');
                    pointer.push_str(key);
                }
                PathSegment::Index(idx) => {
                    pointer.push('/');
                    pointer.push_str(&idx.to_string());
                }
                PathSegment::Wildcard => {
                    pointer.push_str("/*");
                }
            }
        }
        if pointer.is_empty() {
            "/".to_string()
        } else {
            pointer
        }
    }
}

impl Default for PriorityStreamer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_json_path_creation() {
        let path = JsonPath::root();
        assert_eq!(path.to_json_pointer(), "/");

        let path = path.append_key("users").append_index(0).append_key("name");
        assert_eq!(path.to_json_pointer(), "/users/0/name");
    }

    #[test]
    fn test_priority_comparison() {
        assert!(Priority::Critical > Priority::High);
        assert!(Priority::High > Priority::Medium);
        assert!(Priority::Medium > Priority::Low);
        assert!(Priority::Low > Priority::Background);
    }

    #[test]
    fn test_skeleton_generation() {
        let streamer = PriorityStreamer::new();
        let json = json!({
            "name": "John",
            "age": 30,
            "active": true,
            "posts": ["post1", "post2"]
        });

        let skeleton = streamer.generate_skeleton(&json);
        let expected = json!({
            "name": null,
            "age": 0,
            "active": false,
            "posts": []
        });

        assert_eq!(skeleton, expected);
    }

    #[test]
    fn test_field_priority_calculation() {
        let streamer = PriorityStreamer::new();
        let path = JsonPath::root();

        assert_eq!(
            streamer.calculate_field_priority(&path, "id", &json!(123)),
            Priority::Critical
        );

        assert_eq!(
            streamer.calculate_field_priority(&path, "name", &json!("John")),
            Priority::High
        );

        assert_eq!(
            streamer.calculate_field_priority(&path, "reviews", &json!([])),
            Priority::Background
        );
    }

    #[test]
    fn test_streaming_plan_creation() {
        let streamer = PriorityStreamer::new();
        let json = json!({
            "id": 1,
            "name": "John",
            "bio": "Software developer",
            "reviews": ["Good", "Excellent"]
        });

        let plan = streamer.analyze(&json).unwrap();
        assert!(!plan.is_complete());
        assert!(plan.remaining_frames() > 0);
    }
}
