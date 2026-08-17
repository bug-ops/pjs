//! Priority-based JSON streaming implementation
//!
//! This module implements the core Priority JSON Streaming protocol with:
//! - Skeleton-first approach
//! - JSON Path based patching
//! - Priority-based field ordering
//! - Incremental reconstruction

use crate::Result;
use crate::domain::value_objects::{JsonPath, Priority};
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::collections::VecDeque;

/// Custom serde for Priority in stream module
mod serde_priority {
    use crate::domain::value_objects::Priority;
    use serde::{Serialize, Serializer};

    pub fn serialize<S>(priority: &Priority, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        priority.value().serialize(serializer)
    }
}

/// Patch operation for updating JSON structure
#[derive(Debug, Clone, serde::Serialize)]
pub struct JsonPatch {
    /// Path within the target JSON document.
    pub path: JsonPath,
    /// Operation to apply at `path`.
    pub operation: PatchOperation,
    /// Priority assigned to this patch.
    #[serde(with = "serde_priority")]
    pub priority: Priority,
}

/// Operation a [`JsonPatch`] performs at its target path.
#[derive(Debug, Clone, serde::Serialize)]
pub enum PatchOperation {
    /// Replace the value at the path with `value`.
    Set {
        /// New value to set at the path.
        value: JsonValue,
    },
    /// Append values to the array at the path.
    Append {
        /// Values appended to the target array.
        values: Vec<JsonValue>,
    },
    /// Replace the value at the path with `value` (semantically distinct from `Set`).
    Replace {
        /// Replacement value.
        value: JsonValue,
    },
    /// Remove the value at the path.
    Remove,
}

/// Streaming frame containing skeleton or patch data
#[derive(Debug, Clone, serde::Serialize)]
pub enum PriorityStreamFrame {
    /// Initial skeleton frame with placeholder values.
    Skeleton {
        /// Skeleton JSON value (nulls/empties for fields filled later).
        data: JsonValue,
        /// Priority of the skeleton frame.
        #[serde(with = "serde_priority")]
        priority: Priority,
        /// Whether the skeleton is final or further skeletons may follow.
        complete: bool,
    },
    /// Batch of patches sharing the same priority.
    Patch {
        /// Patches in this batch.
        patches: Vec<JsonPatch>,
        /// Priority shared by all patches in the batch.
        #[serde(with = "serde_priority")]
        priority: Priority,
    },
    /// Terminal frame indicating the stream is complete.
    Complete {
        /// Optional checksum of the reconstructed payload.
        checksum: Option<u64>,
    },
}

/// Priority-based JSON streamer
pub struct PriorityStreamer {
    config: StreamerConfig,
}

/// Configuration for [`PriorityStreamer`].
#[derive(Debug, Clone)]
pub struct StreamerConfig {
    /// Enable name-based heuristics that infer priorities from common field names.
    pub detect_semantics: bool,
    /// Maximum number of patches per [`PriorityStreamFrame::Patch`] batch.
    pub max_patch_size: usize,
    /// Patches with priority below this threshold are dropped.
    pub priority_threshold: Priority,
}

impl Default for StreamerConfig {
    fn default() -> Self {
        Self {
            detect_semantics: true,
            max_patch_size: 100,
            priority_threshold: Priority::LOW,
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
        plan.frames.push_back(PriorityStreamFrame::Skeleton {
            data: skeleton,
            priority: Priority::CRITICAL,
            complete: false,
        });

        // Extract patches by priority
        let mut patches = Vec::new();
        self.extract_patches(json, &JsonPath::root(), &mut patches)?;

        // Group patches by priority
        patches.sort_by_key(|patch| std::cmp::Reverse(patch.priority));

        let mut current_priority = Priority::CRITICAL;
        let mut current_batch = Vec::new();

        for patch in patches {
            if patch.priority != current_priority && !current_batch.is_empty() {
                plan.frames.push_back(PriorityStreamFrame::Patch {
                    patches: current_batch,
                    priority: current_priority,
                });
                current_batch = Vec::new();
            }
            current_priority = patch.priority;
            current_batch.push(patch);

            if current_batch.len() >= self.config.max_patch_size {
                plan.frames.push_back(PriorityStreamFrame::Patch {
                    patches: current_batch,
                    priority: current_priority,
                });
                current_batch = Vec::new();
            }
        }

        // Add remaining patches
        if !current_batch.is_empty() {
            plan.frames.push_back(PriorityStreamFrame::Patch {
                patches: current_batch,
                priority: current_priority,
            });
        }

        // Add completion frame
        plan.frames
            .push_back(PriorityStreamFrame::Complete { checksum: None });

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

    /// Deep-clone `value`, emptying every array reachable via a JsonPath-encodable
    /// key — mirrors `extract_patches`'s own traversal exactly. A subtree under a
    /// key `JsonPath` cannot encode (`.`/`[`/`]`) is left fully populated, since
    /// `extract_patches`'s recursion will never reach it to emit a compensating
    /// `Append` (see #394 C3). Encodability depends only on the key string itself
    /// (`JsonPath::append_key`'s sole failure mode), not on the accumulated path,
    /// so this check does not need to thread a `JsonPath` through the recursion.
    fn skeletonize_arrays(value: &JsonValue) -> JsonValue {
        match value {
            JsonValue::Object(map) => {
                let skeleton = map
                    .iter()
                    .map(|(key, v)| match JsonPath::root().append_key(key) {
                        Ok(_) => (key.clone(), Self::skeletonize_arrays(v)),
                        Err(_) => (key.clone(), v.clone()),
                    })
                    .collect();
                JsonValue::Object(skeleton)
            }
            JsonValue::Array(_) => JsonValue::Array(vec![]),
            other => other.clone(),
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
                    // Keys JsonPath cannot encode (`.`, `[`, `]`) are skipped:
                    // one weird key must not abort the whole streaming plan.
                    let Ok(field_path) = current_path.append_key(key) else {
                        continue;
                    };
                    let own_priority = self.calculate_field_priority(&field_path, key, value);

                    let mut child_patches = Vec::new();
                    self.extract_patches(value, &field_path, &mut child_patches)?;

                    // Hoist the Set's priority to at least the highest Append
                    // priority anywhere in its subtree, so a Set can never be
                    // sorted/applied after an Append it must precede (#394 C1/C2).
                    let append_ceiling = child_patches
                        .iter()
                        .filter(|p| matches!(p.operation, PatchOperation::Append { .. }))
                        .map(|p| p.priority)
                        .max();
                    let priority =
                        append_ceiling.map_or(own_priority, |ceiling| own_priority.max(ceiling));

                    patches.push(JsonPatch {
                        path: field_path.clone(),
                        operation: PatchOperation::Set {
                            value: Self::skeletonize_arrays(value),
                        },
                        priority,
                    });

                    patches.extend(child_patches);
                }
            }
            JsonValue::Array(arr) => {
                // For arrays, create append operations in chunks
                if arr.len() > 10 {
                    // Priority is computed once from the full array so every
                    // chunk of the same array shares it: computing it per-chunk
                    // let a short tail chunk outrank the bulk chunks and jump
                    // ahead of them in the priority sort, corrupting element
                    // order on reconstruction (#394 C2, chunked variant).
                    let priority = self.calculate_array_priority(current_path, arr);
                    for chunk in arr.chunks(self.config.max_patch_size) {
                        patches.push(JsonPatch {
                            path: current_path.clone(),
                            operation: PatchOperation::Append {
                                values: chunk.to_vec(),
                            },
                            priority,
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
            return Priority::CRITICAL;
        }

        // High priority fields
        if matches!(key, "name" | "title" | "label" | "email" | "username") {
            return Priority::HIGH;
        }

        // Low priority patterns
        if key.contains("analytics") || key.contains("stats") || key.contains("meta") {
            return Priority::LOW;
        }

        if matches!(key, "reviews" | "comments" | "logs" | "history") {
            return Priority::BACKGROUND;
        }

        // Content-based priority
        match value {
            JsonValue::Array(arr) if arr.len() > 100 => Priority::BACKGROUND,
            JsonValue::Object(obj) if obj.contains_key("timestamp") => Priority::MEDIUM,
            JsonValue::String(s) if s.len() > 1000 => Priority::LOW,
            _ => Priority::MEDIUM,
        }
    }

    /// Calculate priority for array elements
    fn calculate_array_priority(&self, path: &JsonPath, elements: &[JsonValue]) -> Priority {
        // Large arrays get background priority
        if elements.len() > 50 {
            return Priority::BACKGROUND;
        }

        // Arrays in certain paths get different priorities
        if let Some(last_key) = path.last_key() {
            if matches!(last_key, "reviews" | "comments" | "logs") {
                return Priority::BACKGROUND;
            }
            if matches!(last_key, "items" | "data" | "results") {
                return Priority::MEDIUM;
            }
        }

        Priority::MEDIUM
    }
}

/// Plan for streaming JSON with priority ordering
#[derive(Debug)]
pub struct StreamingPlan {
    /// Ordered queue of frames produced by analysis.
    pub frames: VecDeque<PriorityStreamFrame>,
}

impl Default for StreamingPlan {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamingPlan {
    /// Create an empty plan with no frames.
    pub fn new() -> Self {
        Self {
            frames: VecDeque::new(),
        }
    }

    /// Get next frame to send
    pub fn next_frame(&mut self) -> Option<PriorityStreamFrame> {
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
    pub fn frames(&self) -> impl Iterator<Item = &PriorityStreamFrame> {
        self.frames.iter()
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
    use crate::stream::reconstruction::JsonReconstructor;
    use serde_json::json;

    /// Runs `payload` through `PriorityStreamer::analyze()` and applies every
    /// produced frame to a fresh `JsonReconstructor`, returning the reconstructed
    /// document. Exercises the full `analyze()` -> `JsonReconstructor` pipeline,
    /// not just patch inspection (see spec NFR-004).
    fn round_trip(streamer: &PriorityStreamer, payload: &JsonValue) -> JsonValue {
        let plan = streamer.analyze(payload).unwrap();
        let mut reconstructor = JsonReconstructor::new();
        for frame in plan.frames {
            reconstructor.add_frame(frame);
        }
        reconstructor.process_all_frames().unwrap();
        reconstructor.current_state().clone()
    }

    #[test]
    fn test_json_path_creation() {
        let path = JsonPath::root();
        assert_eq!(path.to_json_pointer(), "/");

        let path = path
            .append_key("users")
            .unwrap()
            .append_index(0)
            .append_key("name")
            .unwrap();
        assert_eq!(path.to_json_pointer(), "/users/0/name");
    }

    #[test]
    fn test_priority_comparison() {
        assert!(Priority::CRITICAL > Priority::HIGH);
        assert!(Priority::HIGH > Priority::MEDIUM);
        assert!(Priority::MEDIUM > Priority::LOW);
        assert!(Priority::LOW > Priority::BACKGROUND);
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
            Priority::CRITICAL
        );

        assert_eq!(
            streamer.calculate_field_priority(&path, "name", &json!("John")),
            Priority::HIGH
        );

        assert_eq!(
            streamer.calculate_field_priority(&path, "reviews", &json!([])),
            Priority::BACKGROUND
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

    // Regression tests for object-nested array duplication (spec 031 / issue #394):
    // `extract_patches` used to emit a `Set` patch carrying the full array value for
    // an object field, then recurse and emit an `Append` patch for the same array,
    // duplicating every non-empty object-nested array after a full analyze() ->
    // JsonReconstructor round trip. These tests wire analyze() directly into
    // JsonReconstructor (not just patch inspection) per US-003/NFR-004.

    #[test]
    fn test_round_trip_exact_repro_case() {
        let streamer = PriorityStreamer::new();
        let payload = json!({"items": [1, 2, 3]});

        let result = round_trip(&streamer, &payload);

        assert_eq!(result, payload);
        assert_eq!(result["items"], json!([1, 2, 3]));
    }

    #[test]
    fn test_round_trip_multi_entity_payload() {
        let streamer = PriorityStreamer::new();
        let payload = json!({
            "users": [
                {"id": 1, "name": "Alice"},
                {"id": 2, "name": "Bob"}
            ],
            "metadata": {
                "nested": {
                    "deep": [1, 2, 3, 4, 5]
                }
            }
        });

        let result = round_trip(&streamer, &payload);

        assert_eq!(result, payload);
    }

    #[test]
    fn test_round_trip_chunked_array_exceeds_max_patch_size() {
        let config = StreamerConfig {
            max_patch_size: 5,
            ..StreamerConfig::default()
        };
        let streamer = PriorityStreamer::with_config(config);
        let items: Vec<JsonValue> = (0..23).map(|i| json!(i)).collect();
        let payload = json!({ "data": items });

        let result = round_trip(&streamer, &payload);

        assert_eq!(result, payload);
        assert_eq!(result["data"].as_array().unwrap().len(), 23);
    }

    #[test]
    fn test_round_trip_chunked_array_divergent_chunk_priority() {
        // #394 M2: guards against computing `calculate_array_priority` per
        // chunk slice instead of once for the whole array. With
        // max_patch_size 60 over a 130-element "items" array, chunks are
        // 60/60/10: under the old per-chunk scheme the 10-element tail (len
        // <= 50) would fall through to the "items" last-key boost and get
        // MEDIUM priority, while the two 60-element head chunks (len > 50)
        // get BACKGROUND — the higher-priority tail would then sort ahead of
        // the head chunks and be applied first, corrupting element order.
        let config = StreamerConfig {
            max_patch_size: 60,
            ..StreamerConfig::default()
        };
        let streamer = PriorityStreamer::with_config(config);
        let items: Vec<JsonValue> = (0..130).map(|i| json!(i)).collect();
        let payload = json!({ "items": items });

        let result = round_trip(&streamer, &payload);

        assert_eq!(result, payload);
    }

    #[test]
    fn test_round_trip_array_nested_at_depth_three() {
        let streamer = PriorityStreamer::new();
        let payload = json!({
            "level1": {
                "level2": {
                    "level3": [1, 2, 3, 4]
                }
            }
        });

        let result = round_trip(&streamer, &payload);

        assert_eq!(result, payload);
    }

    #[test]
    fn test_round_trip_array_of_arrays() {
        let streamer = PriorityStreamer::new();
        let payload = json!({
            "matrix": [[1, 2], [3, 4]]
        });

        let result = round_trip(&streamer, &payload);

        assert_eq!(result, payload);
    }

    #[test]
    fn test_round_trip_array_of_objects_with_nested_arrays() {
        let streamer = PriorityStreamer::new();
        let payload = json!({
            "users": [
                {"name": "Alice", "tags": ["admin", "active"]},
                {"name": "Bob", "tags": ["guest"]}
            ]
        });

        let result = round_trip(&streamer, &payload);

        assert_eq!(result, payload);
    }

    #[test]
    fn test_round_trip_empty_array_field() {
        let streamer = PriorityStreamer::new();
        let payload = json!({"items": []});

        let result = round_trip(&streamer, &payload);

        assert_eq!(result, payload);
    }

    #[test]
    fn test_round_trip_top_level_bare_array() {
        let streamer = PriorityStreamer::new();
        let payload = json!([1, 2, 3]);

        let result = round_trip(&streamer, &payload);

        assert_eq!(result, payload);
    }

    #[test]
    fn test_round_trip_mixed_payload_no_regression() {
        let streamer = PriorityStreamer::new();
        let payload = json!({
            "id": 1,
            "name": "widget",
            "tags": ["a", "b", "c"],
            "details": {
                "color": "red",
                "size": 10
            }
        });

        let result = round_trip(&streamer, &payload);

        assert_eq!(result, payload);
        assert_eq!(result["tags"], json!(["a", "b", "c"]));
        assert_eq!(result["details"], json!({"color": "red", "size": 10}));
    }

    // Regression cases for the Set/Append priority-inversion data-loss bug
    // (impl-critic findings C1-C3, tracked alongside issue #394's redesign).
    // `analyze()` sorts patches by priority *descending*, independently of
    // path/depth, so a field's `Append` (or a descendant's `Set`/`Append`) can
    // land in an earlier-processed, higher-priority batch than its own or an
    // ancestor's `Set`. Pre-fix this was harmless (`Set` always carried the
    // full pristine value); post-fix `Set` carries a skeleton, so an
    // out-of-order `Set` destructively wipes already-applied data. These are
    // expected to FAIL until the ordering issue is fixed (see task #9).

    #[test]
    fn test_round_trip_same_path_priority_inversion() {
        // "history" gets BACKGROUND field priority (matches the
        // id/uuid/.../history critical-field-name list) but its Append gets
        // MEDIUM array priority ("history" is absent from the
        // reviews|comments|logs array-priority boost list) -> Append (MEDIUM)
        // applies before Set (BACKGROUND) wipes the field to `[]`.
        let streamer = PriorityStreamer::new();
        let payload = json!({"history": [1, 2, 3]});

        let result = round_trip(&streamer, &payload);

        assert_eq!(result, payload);
    }

    #[test]
    fn test_round_trip_parent_object_priority_inversion() {
        // Any key containing "stats"/"analytics"/"meta" gets LOW field
        // priority, but its nested array field defaults to MEDIUM -> the
        // nested Set/Append pair (MEDIUM) applies and populates correctly,
        // then the ancestor's skeletonized Set (LOW) applies afterward and
        // wipes the nested field back to `[]`.
        let streamer = PriorityStreamer::new();
        let payload = json!({"stats": {"values": [1, 2, 3]}});

        let result = round_trip(&streamer, &payload);

        assert_eq!(result, payload);
    }

    #[test]
    fn test_round_trip_parent_object_priority_inversion_chunked() {
        // Same class as above, but with a >100-element array so the chunking
        // branch fires: the tail chunk (<=50 elements) gets MEDIUM while the
        // head chunk (>50 elements) and the ancestor's Set get BACKGROUND/LOW
        // respectively, so only some elements survive the ancestor Set wipe.
        let streamer = PriorityStreamer::new();
        let values: Vec<JsonValue> = (0..101).map(|i| json!(i)).collect();
        let payload = json!({"stats": {"values": values}});

        let result = round_trip(&streamer, &payload);

        assert_eq!(result, payload);
        assert_eq!(
            result["stats"]["values"].as_array().unwrap().len(),
            101,
            "expected all 101 elements to survive the round trip"
        );
    }

    #[test]
    fn test_round_trip_unencodable_key_parent_wipe() {
        // `extract_patches` skips recursion into keys JsonPath cannot encode
        // (containing '.', '[', ']'), so no Set/Append is ever emitted for
        // "weird.key" itself. Pre-fix, the parent "outer" field's Set carried
        // the full pristine value (including "weird.key"'s array) as a safety
        // net. Post-fix, `skeletonize_arrays` recurses into "outer" and empties
        // "weird.key"'s array too, permanently losing the only copy of its data.
        let streamer = PriorityStreamer::new();
        let payload = json!({"outer": {"weird.key": [1, 2, 3]}});

        let result = round_trip(&streamer, &payload);

        assert_eq!(result, payload);
    }
}
