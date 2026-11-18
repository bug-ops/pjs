//! Client-side reconstruction engine for applying streaming patches
//!
//! This module provides functionality to reconstruct complete JSON from
//! skeleton + patch stream frames, enabling progressive data loading.

use crate::Result;
use crate::domain::value_objects::JsonPath;
use crate::error::FrameSequenceError;
use crate::stream::patch::apply_patch;
use crate::stream::priority::PriorityStreamFrame;
use serde_json::Value as JsonValue;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Client-side JSON reconstruction engine
///
/// Applies streaming patches to progressively build complete JSON structure.
///
/// # Untrusted peers
///
/// Patch application has no path depth limit (the WebSocket client enforces
/// `MAX_PATCH_PATH_DEPTH` itself, before it applies a patch). An embedder that
/// feeds frames from an untrusted peer must bound the patch path depth, the
/// document size and the frame count itself: a hostile peer can otherwise
/// grow the state's nesting without bound, and dropping a deeply nested value
/// recurses.
pub struct JsonReconstructor {
    /// Current state of JSON being reconstructed
    current_state: JsonValue,
    /// Queue of received frames waiting to be processed
    frame_queue: VecDeque<PriorityStreamFrame>,
    /// Statistics about reconstruction process
    stats: ReconstructionStats,
}

/// Timing of one reconstruction run; "finished without starting" and
/// "end before start" cannot be represented.
#[derive(Debug, Clone, Copy, Default)]
enum Timing {
    #[default]
    NotStarted,
    Running {
        start: Instant,
    },
    Finished {
        start: Instant,
        end: Instant,
    },
}

impl Timing {
    fn start(&mut self) {
        if matches!(self, Self::NotStarted) {
            *self = Self::Running {
                start: Instant::now(),
            };
        }
    }

    fn finish(&mut self) {
        if let Self::Running { start } = *self {
            *self = Self::Finished {
                start,
                end: Instant::now(),
            };
        }
    }

    fn duration(&self) -> Option<Duration> {
        match *self {
            Self::Finished { start, end } => Some(end.duration_since(start)),
            Self::NotStarted | Self::Running { .. } => None,
        }
    }
}

/// Read-only statistics about the reconstruction process.
///
/// Obtained from [`JsonReconstructor::stats`]; counters saturate at `u64::MAX`
/// instead of overflowing. Rates and the duration are `None` until the
/// reconstruction has completed.
#[derive(Debug, Clone, Default)]
pub struct ReconstructionStats {
    frames_processed: u64,
    patches_applied: u64,
    skeleton_frames: u64,
    patch_frames: u64,
    timing: Timing,
}

impl JsonReconstructor {
    /// Create new JSON reconstructor
    pub fn new() -> Self {
        Self {
            current_state: JsonValue::Null,
            frame_queue: VecDeque::new(),
            stats: ReconstructionStats::default(),
        }
    }

    /// Add a frame to the reconstruction queue
    pub fn add_frame(&mut self, frame: PriorityStreamFrame) {
        self.stats.timing.start();
        self.frame_queue.push_back(frame);
    }

    /// Process next frame in the queue
    ///
    /// # Errors
    ///
    /// [`FrameSequenceError::AfterComplete`] when the stream already completed;
    /// the frame is consumed and dropped, and state and stats are unchanged.
    /// Patch application errors are propagated.
    pub fn process_next_frame(&mut self) -> Result<ProcessResult> {
        if let Some(frame) = self.frame_queue.pop_front() {
            self.process_frame(frame)
        } else {
            Ok(ProcessResult::NoFrames)
        }
    }

    /// Process all queued frames
    ///
    /// # Errors
    ///
    /// Returns at the first rejected frame, see [`Self::process_next_frame`];
    /// frames queued behind it stay queued.
    pub fn process_all_frames(&mut self) -> Result<Vec<ProcessResult>> {
        let mut results = Vec::new();

        while let Some(frame) = self.frame_queue.pop_front() {
            results.push(self.process_frame(frame)?);
        }

        Ok(results)
    }

    /// Process a single frame
    fn process_frame(&mut self, frame: PriorityStreamFrame) -> Result<ProcessResult> {
        if self.is_complete() {
            return Err(FrameSequenceError::AfterComplete.into());
        }
        self.stats.frames_processed = self.stats.frames_processed.saturating_add(1);

        match frame {
            PriorityStreamFrame::Skeleton {
                data,
                priority: _,
                complete: _,
            } => {
                self.stats.skeleton_frames = self.stats.skeleton_frames.saturating_add(1);
                self.current_state = data;
                Ok(ProcessResult::SkeletonApplied)
            }

            PriorityStreamFrame::Patch { patches, priority } => {
                self.stats.patch_frames = self.stats.patch_frames.saturating_add(1);
                let mut applied_paths = Vec::new();

                for patch in patches {
                    let path = apply_patch(&mut self.current_state, patch.patch)?;
                    applied_paths.push(path);
                    self.stats.patches_applied = self.stats.patches_applied.saturating_add(1);
                }

                Ok(ProcessResult::PatchesApplied {
                    priority,
                    paths: applied_paths,
                })
            }

            PriorityStreamFrame::Complete { checksum: _ } => {
                self.stats.timing.finish();
                Ok(ProcessResult::ReconstructionComplete)
            }
        }
    }

    /// Get current JSON state (read-only)
    pub fn current_state(&self) -> &JsonValue {
        &self.current_state
    }

    /// Check if reconstruction is complete
    pub fn is_complete(&self) -> bool {
        matches!(self.stats.timing, Timing::Finished { .. })
    }

    /// Get reconstruction statistics
    pub fn stats(&self) -> &ReconstructionStats {
        &self.stats
    }

    /// Reset reconstructor to initial state
    pub fn reset(&mut self) {
        self.current_state = JsonValue::Null;
        self.frame_queue.clear();
        self.stats = ReconstructionStats::default();
    }

    /// Get reconstruction progress (0.0 to 1.0)
    pub fn progress(&self) -> f32 {
        if self.is_complete() {
            1.0
        } else if self.stats.frames_processed == 0 {
            0.0
        } else {
            // Rough estimate based on frames processed vs expected
            let queued = u64::try_from(self.frame_queue.len()).unwrap_or(u64::MAX);
            let total = self.stats.frames_processed.saturating_add(queued);
            (self.stats.frames_processed as f32 / total as f32).min(0.95) // Never show 100% until complete
        }
    }

    /// Get reconstruction duration if available
    pub fn duration(&self) -> Option<Duration> {
        self.stats.duration()
    }
}

/// Result of processing a frame
#[derive(Debug, Clone)]
pub enum ProcessResult {
    /// No frames in queue
    NoFrames,
    /// Skeleton frame was applied
    SkeletonApplied,
    /// Patch frames were applied
    PatchesApplied {
        /// Priority of the patches
        priority: crate::stream::Priority,
        /// Paths that were modified
        paths: Vec<JsonPath>,
    },
    /// Reconstruction completed
    ReconstructionComplete,
}

impl Default for JsonReconstructor {
    fn default() -> Self {
        Self::new()
    }
}

impl ReconstructionStats {
    /// Total frames processed
    pub fn frames_processed(&self) -> u64 {
        self.frames_processed
    }

    /// Total patches applied
    pub fn patches_applied(&self) -> u64 {
        self.patches_applied
    }

    /// Number of skeleton frames received
    pub fn skeleton_frames(&self) -> u64 {
        self.skeleton_frames
    }

    /// Number of patch frames received
    pub fn patch_frames(&self) -> u64 {
        self.patch_frames
    }

    /// Time from the first queued frame to the processed `Complete` frame,
    /// `None` until the reconstruction has completed
    pub fn duration(&self) -> Option<Duration> {
        self.timing.duration()
    }

    /// Get processing rate in frames per second
    pub fn frames_per_second(&self) -> Option<f32> {
        self.rate(self.frames_processed)
    }

    /// Get patches per second rate
    pub fn patches_per_second(&self) -> Option<f32> {
        self.rate(self.patches_applied)
    }

    fn rate(&self, count: u64) -> Option<f32> {
        let secs = self.duration()?.as_secs_f32();
        (secs > 0.0).then_some(count as f32 / secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::value_objects::PathSegment;
    use crate::stream::Priority;
    use crate::stream::patch::WirePatch;
    use crate::stream::priority::JsonPatch;
    use core::assert_matches;
    use serde_json::json;

    #[test]
    fn test_reconstructor_creation() {
        let reconstructor = JsonReconstructor::new();
        assert!(!reconstructor.is_complete());
        assert_eq!(reconstructor.stats().frames_processed(), 0);
        assert_eq!(reconstructor.current_state(), &JsonValue::Null);
    }

    #[test]
    fn test_skeleton_application() {
        let mut reconstructor = JsonReconstructor::new();

        let skeleton = json!({
            "name": null,
            "age": 0,
            "active": false
        });

        let frame = PriorityStreamFrame::Skeleton {
            data: skeleton.clone(),
            priority: Priority::CRITICAL,
            complete: false,
        };

        reconstructor.add_frame(frame);
        let result = reconstructor.process_next_frame().unwrap();

        assert_matches!(result, ProcessResult::SkeletonApplied);
        assert_eq!(reconstructor.current_state(), &skeleton);
        assert_eq!(reconstructor.stats().skeleton_frames(), 1);
    }

    #[test]
    fn test_patch_application() {
        let mut reconstructor = JsonReconstructor::new();

        // Start with skeleton
        let skeleton = json!({
            "name": null,
            "age": 0
        });

        reconstructor.current_state = skeleton;

        // Create patch to set name
        let path = JsonPath::from_segments(vec![PathSegment::Key("name".to_string())]);

        let patch = JsonPatch {
            patch: WirePatch::Set {
                path,
                value: json!("John Doe"),
            },
            priority: Priority::HIGH,
        };

        let frame = PriorityStreamFrame::Patch {
            patches: vec![patch],
            priority: Priority::HIGH,
        };

        reconstructor.add_frame(frame);
        let result = reconstructor.process_next_frame().unwrap();

        assert_matches!(result, ProcessResult::PatchesApplied { ref paths, .. } if paths.len() == 1);

        let expected = json!({
            "name": "John Doe",
            "age": 0
        });

        assert_eq!(reconstructor.current_state(), &expected);
        assert_eq!(reconstructor.stats().patches_applied(), 1);
    }

    #[test]
    fn test_array_append() {
        let mut reconstructor = JsonReconstructor::new();

        // Start with skeleton having empty array
        let skeleton = json!({
            "items": []
        });

        reconstructor.current_state = skeleton;

        // Create patch to append items
        let path = JsonPath::from_segments(vec![PathSegment::Key("items".to_string())]);

        let patch = JsonPatch {
            patch: WirePatch::Append {
                path,
                values: vec![json!("item1"), json!("item2")],
            },
            priority: Priority::MEDIUM,
        };

        let frame = PriorityStreamFrame::Patch {
            patches: vec![patch],
            priority: Priority::MEDIUM,
        };

        reconstructor.add_frame(frame);
        reconstructor.process_next_frame().unwrap();

        let expected = json!({
            "items": ["item1", "item2"]
        });

        assert_eq!(reconstructor.current_state(), &expected);
    }

    #[test]
    fn test_completion_tracking() {
        let mut reconstructor = JsonReconstructor::new();

        let complete_frame = PriorityStreamFrame::Complete { checksum: None };
        reconstructor.add_frame(complete_frame);

        let result = reconstructor.process_next_frame().unwrap();
        assert_matches!(result, ProcessResult::ReconstructionComplete);
        assert!(reconstructor.is_complete());
        assert!(reconstructor.stats().duration().is_some());
    }

    #[test]
    fn test_progress_calculation() {
        let mut reconstructor = JsonReconstructor::new();

        // Initially no progress
        assert_eq!(reconstructor.progress(), 0.0);

        // Add some frames but don't process
        let skeleton = json!({"test": null});
        reconstructor.add_frame(PriorityStreamFrame::Skeleton {
            data: skeleton,
            priority: Priority::CRITICAL,
            complete: false,
        });
        reconstructor.add_frame(PriorityStreamFrame::Complete { checksum: None });

        // Process one frame
        reconstructor.process_next_frame().unwrap();
        let progress = reconstructor.progress();
        assert!(
            progress > 0.0 && progress < 1.0,
            "Progress was: {}",
            progress
        );

        // Process remaining frame (Complete)
        reconstructor.process_next_frame().unwrap();
        assert_eq!(reconstructor.progress(), 1.0);
    }

    #[test]
    fn test_reset_functionality() {
        let mut reconstructor = JsonReconstructor::new();

        // Setup some state
        reconstructor.current_state = json!({"test": true});
        reconstructor.add_frame(PriorityStreamFrame::Complete { checksum: None });
        reconstructor.process_next_frame().unwrap();
        assert!(reconstructor.is_complete());

        // Reset
        reconstructor.reset();

        assert_eq!(reconstructor.current_state(), &JsonValue::Null);
        assert!(!reconstructor.is_complete());
        assert_eq!(reconstructor.stats().frames_processed(), 0);
    }

    // NEW TESTS FOR COVERAGE IMPROVEMENT (Task P2-TEST-003)

    #[test]
    fn test_deep_nesting_level_3() {
        let mut reconstructor = JsonReconstructor::new();

        let skeleton = json!({
            "level1": {
                "level2": {
                    "level3": null
                }
            }
        });

        reconstructor.current_state = skeleton;

        let path = JsonPath::from_segments(vec![
            PathSegment::Key("level1".to_string()),
            PathSegment::Key("level2".to_string()),
            PathSegment::Key("level3".to_string()),
        ]);

        let patch = JsonPatch {
            patch: WirePatch::Set {
                path,
                value: json!("deep value"),
            },
            priority: Priority::MEDIUM,
        };

        let frame = PriorityStreamFrame::Patch {
            patches: vec![patch],
            priority: Priority::MEDIUM,
        };

        reconstructor.add_frame(frame);
        reconstructor.process_next_frame().unwrap();

        assert_eq!(
            reconstructor.current_state()["level1"]["level2"]["level3"],
            json!("deep value")
        );
    }

    #[test]
    fn test_deep_nesting_level_5() {
        let mut reconstructor = JsonReconstructor::new();

        let skeleton = json!({
            "l1": {
                "l2": {
                    "l3": {
                        "l4": {
                            "l5": null
                        }
                    }
                }
            }
        });

        reconstructor.current_state = skeleton;

        let path = JsonPath::from_segments(vec![
            PathSegment::Key("l1".to_string()),
            PathSegment::Key("l2".to_string()),
            PathSegment::Key("l3".to_string()),
            PathSegment::Key("l4".to_string()),
            PathSegment::Key("l5".to_string()),
        ]);

        let patch = JsonPatch {
            patch: WirePatch::Set {
                path,
                value: json!({"final": "value"}),
            },
            priority: Priority::HIGH,
        };

        let frame = PriorityStreamFrame::Patch {
            patches: vec![patch],
            priority: Priority::HIGH,
        };

        reconstructor.add_frame(frame);
        reconstructor.process_next_frame().unwrap();

        assert_eq!(
            reconstructor.current_state()["l1"]["l2"]["l3"]["l4"]["l5"],
            json!({"final": "value"})
        );
    }

    #[test]
    fn test_out_of_order_patches() {
        let mut reconstructor = JsonReconstructor::new();

        let skeleton = json!({"a": null, "b": null, "c": null});
        reconstructor.current_state = skeleton;

        // Add patches in reverse order
        let patch_c = JsonPatch {
            patch: WirePatch::Set {
                path: JsonPath::from_segments(vec![PathSegment::Key("c".to_string())]),
                value: json!("third"),
            },
            priority: Priority::LOW,
        };

        let patch_a = JsonPatch {
            patch: WirePatch::Set {
                path: JsonPath::from_segments(vec![PathSegment::Key("a".to_string())]),
                value: json!("first"),
            },
            priority: Priority::LOW,
        };

        let patch_b = JsonPatch {
            patch: WirePatch::Set {
                path: JsonPath::from_segments(vec![PathSegment::Key("b".to_string())]),
                value: json!("second"),
            },
            priority: Priority::LOW,
        };

        reconstructor.add_frame(PriorityStreamFrame::Patch {
            patches: vec![patch_c],
            priority: Priority::LOW,
        });
        reconstructor.add_frame(PriorityStreamFrame::Patch {
            patches: vec![patch_a],
            priority: Priority::LOW,
        });
        reconstructor.add_frame(PriorityStreamFrame::Patch {
            patches: vec![patch_b],
            priority: Priority::LOW,
        });

        reconstructor.process_all_frames().unwrap();

        assert_eq!(reconstructor.current_state()["a"], json!("first"));
        assert_eq!(reconstructor.current_state()["b"], json!("second"));
        assert_eq!(reconstructor.current_state()["c"], json!("third"));
    }

    #[test]
    fn test_object_field_set() {
        let mut reconstructor = JsonReconstructor::new();

        let skeleton = json!({"obj": {"field": null}});
        reconstructor.current_state = skeleton;

        let path = JsonPath::from_segments(vec![
            PathSegment::Key("obj".to_string()),
            PathSegment::Key("field".to_string()),
        ]);

        let patch = JsonPatch {
            patch: WirePatch::Set {
                path,
                value: json!("updated"),
            },
            priority: Priority::MEDIUM,
        };

        let frame = PriorityStreamFrame::Patch {
            patches: vec![patch],
            priority: Priority::MEDIUM,
        };

        reconstructor.add_frame(frame);
        reconstructor.process_next_frame().unwrap();

        assert_eq!(
            reconstructor.current_state()["obj"]["field"],
            json!("updated")
        );
    }

    #[test]
    fn test_array_append_multiple() {
        let mut reconstructor = JsonReconstructor::new();

        let skeleton = json!({"items": []});
        reconstructor.current_state = skeleton;

        let path = JsonPath::from_segments(vec![PathSegment::Key("items".to_string())]);

        let patch = JsonPatch {
            patch: WirePatch::Append {
                path,
                values: vec![json!(1), json!(2), json!(3)],
            },
            priority: Priority::MEDIUM,
        };

        let frame = PriorityStreamFrame::Patch {
            patches: vec![patch],
            priority: Priority::MEDIUM,
        };

        reconstructor.add_frame(frame);
        reconstructor.process_next_frame().unwrap();

        assert_eq!(reconstructor.current_state()["items"], json!([1, 2, 3]));
    }

    #[test]
    fn test_root_path_set_replaces_document() {
        let mut reconstructor = JsonReconstructor::new();
        reconstructor.current_state = json!({"test": "data"});

        let patch = JsonPatch {
            patch: WirePatch::Set {
                path: JsonPath::root(),
                value: json!("new"),
            },
            priority: Priority::MEDIUM,
        };

        apply_patch(&mut reconstructor.current_state, patch.patch).unwrap();
        assert_eq!(reconstructor.current_state(), &json!("new"));
    }

    #[test]
    fn test_error_invalid_parent() {
        let mut reconstructor = JsonReconstructor::new();
        reconstructor.current_state = json!({"value": 123});

        let path = JsonPath::from_segments(vec![
            PathSegment::Key("value".to_string()),
            PathSegment::Key("nested".to_string()),
        ]);

        let patch = JsonPatch {
            patch: WirePatch::Set {
                path,
                value: json!("new"),
            },
            priority: Priority::MEDIUM,
        };

        let result = apply_patch(&mut reconstructor.current_state, patch.patch);
        // Should error because "value" is number, not object
        assert!(result.is_err());
    }

    #[test]
    fn test_error_array_index_out_of_bounds() {
        let mut reconstructor = JsonReconstructor::new();
        reconstructor.current_state = json!({"arr": [1, 2]});

        let path = JsonPath::from_segments(vec![
            PathSegment::Key("arr".to_string()),
            PathSegment::Index(5),
        ]);

        let patch = JsonPatch {
            patch: WirePatch::Set {
                path,
                value: json!("value"),
            },
            priority: Priority::MEDIUM,
        };

        let result = apply_patch(&mut reconstructor.current_state, patch.patch);
        assert!(result.is_err());
    }

    #[test]
    fn test_process_result_no_frames() {
        let mut reconstructor = JsonReconstructor::new();

        let result = reconstructor.process_next_frame().unwrap();
        assert_matches!(result, ProcessResult::NoFrames);
    }

    #[test]
    fn test_duration_tracking() {
        let mut reconstructor = JsonReconstructor::new();

        // Initially no duration
        assert!(reconstructor.duration().is_none());

        // Add and process frame
        let skeleton = json!({"test": "data"});
        reconstructor.add_frame(PriorityStreamFrame::Skeleton {
            data: skeleton,
            priority: Priority::CRITICAL,
            complete: false,
        });

        // Process and complete
        reconstructor.process_next_frame().unwrap();

        reconstructor.add_frame(PriorityStreamFrame::Complete { checksum: None });
        reconstructor.process_next_frame().unwrap();

        // Should have duration now
        assert!(reconstructor.duration().is_some());
    }
}
