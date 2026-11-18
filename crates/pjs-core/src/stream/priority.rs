//! Priority-based JSON streaming implementation
//!
//! This module implements the core Priority JSON Streaming protocol with:
//! - Skeleton-first approach
//! - JSON Path based patching
//! - Priority-based field ordering
//! - Incremental reconstruction

use crate::Result;
use crate::domain::value_objects::{JsonPath, PathSegment, Priority};
use crate::stream::patch::WirePatch;
use pjson_rs_domain::services::{PriorityHeuristicConfig, ValueShape, compute_priority};
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::collections::VecDeque;
use std::num::NonZeroUsize;

/// Patch operation for updating JSON structure
///
/// Applying a patch has no path depth limit; see
/// [`JsonReconstructor`](crate::stream::JsonReconstructor) for what an
/// embedder must bound for untrusted peers.
#[derive(Debug, Clone, serde::Serialize)]
pub struct JsonPatch {
    /// Operation and target path, in the wire vocabulary.
    pub patch: WirePatch,
    /// Priority assigned to this patch.
    #[serde(with = "pjson_rs_domain::serde_support::priority")]
    pub priority: Priority,
}

/// Streaming frame containing skeleton or patch data
#[derive(Debug, Clone, serde::Serialize)]
pub enum PriorityStreamFrame {
    /// Initial skeleton frame with placeholder values.
    Skeleton {
        /// Skeleton JSON value (nulls/empties for fields filled later).
        data: JsonValue,
        /// Priority of the skeleton frame.
        #[serde(with = "pjson_rs_domain::serde_support::priority")]
        priority: Priority,
        /// Whether the skeleton is final or further skeletons may follow.
        complete: bool,
    },
    /// Batch of patches sharing the same priority.
    Patch {
        /// Patches in this batch.
        patches: Vec<JsonPatch>,
        /// Priority shared by all patches in the batch.
        #[serde(with = "pjson_rs_domain::serde_support::priority")]
        priority: Priority,
    },
    /// Terminal frame indicating the stream is complete.
    Complete {
        /// Optional checksum of the reconstructed payload.
        checksum: Option<u64>,
    },
}

/// Priority-based JSON streamer
///
/// Every `Set` and `Append` patch is priced with the shared
/// [`compute_priority`] heuristic and its default field lists, so a document
/// is ordered the same way on every transport. A `Set` is raised to the
/// highest `Append` priority in its subtree, so it is never applied after an
/// `Append` it must precede. A `Set` that an ancestor `Set` of at least the same
/// priority already makes redundant is not emitted.
pub struct PriorityStreamer {
    config: StreamerConfig,
    heuristic: PriorityHeuristicConfig,
}

/// Configuration for [`PriorityStreamer`].
///
/// # Examples
///
/// ```
/// use std::num::NonZeroUsize;
/// use pjson_rs::stream::priority::{PriorityStreamer, StreamerConfig};
///
/// let streamer = PriorityStreamer::with_config(StreamerConfig {
///     max_patch_size: NonZeroUsize::new(50).expect("non-zero"),
/// });
/// # let _ = streamer;
/// ```
#[derive(Debug, Clone)]
pub struct StreamerConfig {
    /// Maximum number of patches per [`PriorityStreamFrame::Patch`] batch.
    pub max_patch_size: NonZeroUsize,
}

impl Default for StreamerConfig {
    fn default() -> Self {
        Self {
            max_patch_size: NonZeroUsize::new(100).expect("100 is non-zero"),
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
        Self {
            config,
            heuristic: PriorityHeuristicConfig::default(),
        }
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
        if Self::needs_root_set(json) {
            // Pushed first so the stable priority sort applies it before any
            // other CRITICAL patch; later patches then refine its value.
            patches.push(JsonPatch {
                patch: WirePatch::Set {
                    path: JsonPath::root(),
                    value: Self::skeletonize_arrays(json),
                },
                priority: Priority::CRITICAL,
            });
        }
        let mut path = JsonPath::root();
        let (effective, _) = self.collect_effective_priorities(json, &mut path);
        self.extract_patches(json, &mut path, None, effective, &mut patches);

        // Group patches by priority
        patches.sort_by_key(|patch| std::cmp::Reverse(patch.priority));

        let mut current_priority = Priority::CRITICAL;
        let mut current_batch = Vec::new();

        for patch in patches {
            if patch.priority != current_priority && !current_batch.is_empty() {
                plan.frames.push_back(PriorityStreamFrame::Patch {
                    patches: std::mem::take(&mut current_batch),
                    priority: current_priority,
                });
            }
            current_priority = patch.priority;
            current_batch.push(patch);

            if current_batch.len() >= self.config.max_patch_size.get() {
                plan.frames.push_back(PriorityStreamFrame::Patch {
                    patches: std::mem::take(&mut current_batch),
                    priority: current_priority,
                });
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

    /// Whether the root cannot be rebuilt from path-addressed patches alone:
    /// a scalar root has no path-addressable children.
    fn needs_root_set(json: &JsonValue) -> bool {
        !matches!(json, JsonValue::Object(_) | JsonValue::Array(_))
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
                            JsonValue::Number(n) if n.is_f64() => JsonValue::from(0.0),
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

    /// Deep-clone `value`, emptying every array reachable through objects —
    /// mirrors `extract_patches`'s own traversal exactly, so each emptied array
    /// gets its compensating `Append` (see #394 C3).
    fn skeletonize_arrays(value: &JsonValue) -> JsonValue {
        match value {
            JsonValue::Object(map) => {
                let skeleton = map
                    .iter()
                    .map(|(key, v)| (key.clone(), Self::skeletonize_arrays(v)))
                    .collect();
                JsonValue::Object(skeleton)
            }
            JsonValue::Array(_) => JsonValue::Array(vec![]),
            other => other.clone(),
        }
    }

    /// First pass: price every object field, mirroring the document's shape, and
    /// return the highest `Append` priority in `json`'s subtree.
    ///
    /// A field's effective priority is its own priority raised to the highest
    /// `Append` priority below it, so its `Set` can never be applied after an
    /// `Append` it must precede (#394 C1/C2).
    fn collect_effective_priorities(
        &self,
        json: &JsonValue,
        path: &mut JsonPath,
    ) -> (FieldPriorities, Option<Priority>) {
        match json {
            JsonValue::Object(map) => {
                let mut fields = Vec::with_capacity(map.len());
                let mut ceiling = None;
                for (key, value) in map {
                    path.push(PathSegment::Key(key.clone()));
                    let own = self.priority_of(path, value);
                    let (children, below) = self.collect_effective_priorities(value, path);
                    fields.push((below.map_or(own, |append| own.max(append)), children));
                    ceiling = ceiling.max(below);
                    path.pop();
                }
                (FieldPriorities(fields), ceiling)
            }
            JsonValue::Array(arr) if !arr.is_empty() => (
                FieldPriorities::default(),
                Some(self.priority_of_array(path, arr)),
            ),
            _ => (FieldPriorities::default(), None),
        }
    }

    /// Second pass: emit patches in pre-order.
    ///
    /// A field's `Set` is emitted only when its effective priority exceeds that
    /// of every ancestor field: otherwise an ancestor `Set` with at least that
    /// priority is applied first and already carries the same content, so the
    /// field's `Set` would be a no-op. Along any chain a leaf is therefore
    /// cloned once per strictly increasing priority step, not once per depth.
    fn extract_patches(
        &self,
        json: &JsonValue,
        path: &mut JsonPath,
        ancestor_priority: Option<Priority>,
        effective: FieldPriorities,
        patches: &mut Vec<JsonPatch>,
    ) {
        match json {
            JsonValue::Object(map) => {
                for ((key, value), (priority, children)) in map.iter().zip(effective.0) {
                    path.push(PathSegment::Key(key.clone()));
                    if ancestor_priority.is_none_or(|ancestor| priority > ancestor) {
                        patches.push(JsonPatch {
                            patch: WirePatch::Set {
                                path: path.clone(),
                                value: Self::skeletonize_arrays(value),
                            },
                            priority,
                        });
                    }
                    let below = ancestor_priority.max(Some(priority));
                    self.extract_patches(value, path, below, children, patches);
                    path.pop();
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
                    let priority = self.priority_of_array(path, arr);
                    for chunk in arr.chunks(self.config.max_patch_size.get()) {
                        patches.push(JsonPatch {
                            patch: WirePatch::Append {
                                path: path.clone(),
                                values: chunk.to_vec(),
                            },
                            priority,
                        });
                    }
                } else if !arr.is_empty() {
                    patches.push(JsonPatch {
                        patch: WirePatch::Append {
                            path: path.clone(),
                            values: arr.clone(),
                        },
                        priority: self.priority_of_array(path, arr),
                    });
                }
            }
            _ => {
                // Primitive values handled by parent object/array
            }
        }
    }

    fn priority_of(&self, path: &JsonPath, value: &JsonValue) -> Priority {
        compute_priority(&self.heuristic, path, ValueShape::from(value))
    }

    fn priority_of_array(&self, path: &JsonPath, elements: &[JsonValue]) -> Priority {
        compute_priority(
            &self.heuristic,
            path,
            ValueShape::Array {
                len: elements.len(),
            },
        )
    }
}

/// Effective priority of each object field of one value, with the same nesting
/// as the value; empty for arrays and scalars.
#[derive(Debug, Default)]
struct FieldPriorities(Vec<(Priority, FieldPriorities)>);

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

    #[test]
    fn test_needs_root_set_only_for_scalar_roots() {
        for key in ["", "a.b", "a[0]", "a]", "ok"] {
            assert!(!PriorityStreamer::needs_root_set(&json!({ key: 1 })));
        }
        assert!(!PriorityStreamer::needs_root_set(&json!([1])));
        assert!(PriorityStreamer::needs_root_set(&json!(1)));
    }

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
        assert_eq!(path.to_json_pointer(), "");

        let path = path.append_key("users").append_index(0).append_key("name");
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
        let at = |text: &str| JsonPath::new(text).expect("valid path");

        assert_eq!(
            streamer.priority_of(&at("$.id"), &json!(123)),
            Priority::CRITICAL
        );

        assert_eq!(
            streamer.priority_of(&at("$.name"), &json!("John")),
            Priority::HIGH
        );

        assert_eq!(
            streamer.priority_of(&at("$.reviews"), &json!([])),
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
            max_patch_size: NonZeroUsize::new(5).unwrap(),
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
        // #394 M2: guards against pricing each chunk slice instead of the whole
        // array once. With max_patch_size 60 over a 130-element array, chunks
        // are 60/60/10: pricing per slice would give the 10-element tail a
        // higher priority than the two 60-element head chunks, so the tail
        // would sort ahead of the head and be applied first, corrupting
        // element order.
        let config = StreamerConfig {
            max_patch_size: NonZeroUsize::new(60).unwrap(),
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
    fn test_round_trip_scalar_roots() {
        let streamer = PriorityStreamer::new();
        for payload in [
            json!(42),
            json!("text"),
            json!(true),
            json!(null),
            json!(1.5),
        ] {
            assert_eq!(round_trip(&streamer, &payload), payload);
        }
    }

    #[test]
    fn test_round_trip_top_level_unencodable_keys() {
        let streamer = PriorityStreamer::new();
        for payload in [
            json!({"a.b": 1, "ok": 2}),
            json!({"a[0]": "v"}),
            json!({"a.b": [1, 2, 3], "id": 7, "list": [1, 2], "n": {"x": [1]}}),
        ] {
            assert_eq!(round_trip(&streamer, &payload), payload);
        }
    }

    #[test]
    fn test_round_trip_large_string_field() {
        let streamer = PriorityStreamer::new();
        let payload = json!({"blob": "x".repeat(70 * 1024)});
        assert_eq!(round_trip(&streamer, &payload), payload);
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
