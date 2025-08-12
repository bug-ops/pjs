//! Client-side reconstruction engine for applying streaming patches
//!
//! This module provides functionality to reconstruct complete JSON from
//! skeleton + patch stream frames, enabling progressive data loading.

use crate::Result;
use crate::stream::priority::{JsonPatch, JsonPath, PatchOperation, PathSegment, PriorityStreamFrame};
use serde_json::Value as JsonValue;
use std::collections::VecDeque;

/// Client-side JSON reconstruction engine
///
/// Applies streaming patches to progressively build complete JSON structure
pub struct JsonReconstructor {
    /// Current state of JSON being reconstructed
    current_state: JsonValue,
    /// Queue of received frames waiting to be processed
    frame_queue: VecDeque<PriorityStreamFrame>,
    /// Whether reconstruction is complete
    is_complete: bool,
    /// Statistics about reconstruction process
    stats: ReconstructionStats,
}

/// Statistics about the reconstruction process
#[derive(Debug, Clone, Default)]
pub struct ReconstructionStats {
    /// Total frames processed
    pub frames_processed: u32,
    /// Total patches applied
    pub patches_applied: u32,
    /// Number of skeleton frames received
    pub skeleton_frames: u32,
    /// Number of patch frames received
    pub patch_frames: u32,
    /// Time when reconstruction started
    pub start_time: Option<std::time::Instant>,
    /// Time when reconstruction completed
    pub end_time: Option<std::time::Instant>,
}

impl JsonReconstructor {
    /// Create new JSON reconstructor
    pub fn new() -> Self {
        Self {
            current_state: JsonValue::Null,
            frame_queue: VecDeque::new(),
            is_complete: false,
            stats: ReconstructionStats::default(),
        }
    }

    /// Add a frame to the reconstruction queue
    pub fn add_frame(&mut self, frame: PriorityStreamFrame) {
        if self.stats.start_time.is_none() {
            self.stats.start_time = Some(std::time::Instant::now());
        }

        self.frame_queue.push_back(frame);
    }

    /// Process next frame in the queue
    pub fn process_next_frame(&mut self) -> Result<ProcessResult> {
        if let Some(frame) = self.frame_queue.pop_front() {
            self.process_frame(frame)
        } else {
            Ok(ProcessResult::NoFrames)
        }
    }

    /// Process all queued frames
    pub fn process_all_frames(&mut self) -> Result<Vec<ProcessResult>> {
        let mut results = Vec::new();

        while let Some(frame) = self.frame_queue.pop_front() {
            results.push(self.process_frame(frame)?);
        }

        Ok(results)
    }

    /// Process a single frame
    fn process_frame(&mut self, frame: PriorityStreamFrame) -> Result<ProcessResult> {
        self.stats.frames_processed += 1;

        match frame {
            PriorityStreamFrame::Skeleton {
                data,
                priority: _,
                complete: _,
            } => {
                self.stats.skeleton_frames += 1;
                self.current_state = data;
                Ok(ProcessResult::SkeletonApplied)
            }

            PriorityStreamFrame::Patch { patches, priority } => {
                self.stats.patch_frames += 1;
                let mut applied_paths = Vec::new();

                for patch in patches {
                    let path = patch.path.clone();
                    self.apply_patch(patch)?;
                    applied_paths.push(path);
                    self.stats.patches_applied += 1;
                }

                Ok(ProcessResult::PatchesApplied {
                    count: applied_paths.len(),
                    priority,
                    paths: applied_paths,
                })
            }

            PriorityStreamFrame::Complete { checksum: _ } => {
                self.is_complete = true;
                self.stats.end_time = Some(std::time::Instant::now());
                Ok(ProcessResult::ReconstructionComplete)
            }
        }
    }

    /// Apply a single patch to the current JSON state
    fn apply_patch(&mut self, patch: JsonPatch) -> Result<()> {
        match patch.operation {
            PatchOperation::Set { value } => {
                self.set_at_path(&patch.path, value)?;
            }
            PatchOperation::Append { values } => {
                self.append_at_path(&patch.path, values)?;
            }
            PatchOperation::Replace { value } => {
                self.set_at_path(&patch.path, value)?;
            }
            PatchOperation::Remove => {
                self.remove_at_path(&patch.path)?;
            }
        }

        Ok(())
    }

    /// Set value at specified JSON path
    fn set_at_path(&mut self, path: &JsonPath, value: JsonValue) -> Result<()> {
        let segments = path.segments();
        if segments.is_empty() {
            return Err(crate::Error::Other("Empty path".to_string()));
        }

        // Handle root replacement
        if segments.len() == 1 {
            self.current_state = value;
            return Ok(());
        }

        // Navigate to parent and set the target field
        let target_key = path
            .last_key()
            .ok_or_else(|| crate::Error::Other("Invalid path for set operation".to_string()))?;

        let parent = self.get_parent_mut(path)?;

        match parent {
            JsonValue::Object(map) => {
                map.insert(target_key, value);
            }
            JsonValue::Array(arr) => {
                if let Ok(index) = target_key.parse::<usize>() {
                    if index < arr.len() {
                        arr[index] = value;
                    } else {
                        return Err(crate::Error::Other("Array index out of bounds".to_string()));
                    }
                } else {
                    return Err(crate::Error::Other("Invalid array index".to_string()));
                }
            }
            _ => {
                return Err(crate::Error::Other(
                    "Cannot set field on non-object/array".to_string(),
                ));
            }
        }

        Ok(())
    }

    /// Append values to array at specified path
    fn append_at_path(&mut self, path: &JsonPath, values: Vec<JsonValue>) -> Result<()> {
        let target = self.get_mut_at_path(path)?;

        match target {
            JsonValue::Array(arr) => {
                arr.extend(values);
            }
            _ => {
                return Err(crate::Error::Other(
                    "Cannot append to non-array".to_string(),
                ));
            }
        }

        Ok(())
    }

    /// Remove value at specified path
    fn remove_at_path(&mut self, path: &JsonPath) -> Result<()> {
        let target_key = path
            .last_key()
            .ok_or_else(|| crate::Error::Other("Invalid path for remove operation".to_string()))?;

        let parent = self.get_parent_mut(path)?;

        match parent {
            JsonValue::Object(map) => {
                map.remove(&target_key);
            }
            JsonValue::Array(arr) => {
                if let Ok(index) = target_key.parse::<usize>()
                    && index < arr.len() {
                        arr.remove(index);
                    }
            }
            _ => {
                return Err(crate::Error::Other(
                    "Cannot remove from non-object/array".to_string(),
                ));
            }
        }

        Ok(())
    }

    /// Get mutable reference to parent of target path
    fn get_parent_mut(&mut self, path: &JsonPath) -> Result<&mut JsonValue> {
        if path.len() < 2 {
            return Ok(&mut self.current_state);
        }

        let segments = path.segments();
        let parent_segments = &segments[..segments.len() - 1];
        let mut current = &mut self.current_state;

        for segment in parent_segments.iter().skip(1) {
            // Skip root
            match segment {
                PathSegment::Key(key) => {
                    if let JsonValue::Object(map) = current {
                        current = map.get_mut(key).ok_or_else(|| {
                            crate::Error::Other(format!("Key not found: {key}"))
                        })?;
                    } else {
                        return Err(crate::Error::Other("Expected object".to_string()));
                    }
                }
                PathSegment::Index(idx) => {
                    if let JsonValue::Array(arr) = current {
                        current = arr.get_mut(*idx).ok_or_else(|| {
                            crate::Error::Other(format!("Index out of bounds: {idx}"))
                        })?;
                    } else {
                        return Err(crate::Error::Other("Expected array".to_string()));
                    }
                }
                _ => {
                    return Err(crate::Error::Other("Unsupported path segment".to_string()));
                }
            }
        }

        Ok(current)
    }

    /// Get mutable reference at specified path
    fn get_mut_at_path(&mut self, path: &JsonPath) -> Result<&mut JsonValue> {
        let mut current = &mut self.current_state;

        for segment in path.segments().iter().skip(1) {
            // Skip root
            match segment {
                PathSegment::Key(key) => {
                    if let JsonValue::Object(map) = current {
                        current = map.get_mut(key).ok_or_else(|| {
                            crate::Error::Other(format!("Key not found: {key}"))
                        })?;
                    } else {
                        return Err(crate::Error::Other("Expected object".to_string()));
                    }
                }
                PathSegment::Index(idx) => {
                    if let JsonValue::Array(arr) = current {
                        current = arr.get_mut(*idx).ok_or_else(|| {
                            crate::Error::Other(format!("Index out of bounds: {idx}"))
                        })?;
                    } else {
                        return Err(crate::Error::Other("Expected array".to_string()));
                    }
                }
                _ => {
                    return Err(crate::Error::Other("Unsupported path segment".to_string()));
                }
            }
        }

        Ok(current)
    }

    /// Get current JSON state (read-only)
    pub fn current_state(&self) -> &JsonValue {
        &self.current_state
    }

    /// Check if reconstruction is complete
    pub fn is_complete(&self) -> bool {
        self.is_complete
    }

    /// Get reconstruction statistics
    pub fn stats(&self) -> &ReconstructionStats {
        &self.stats
    }

    /// Reset reconstructor to initial state
    pub fn reset(&mut self) {
        self.current_state = JsonValue::Null;
        self.frame_queue.clear();
        self.is_complete = false;
        self.stats = ReconstructionStats::default();
    }

    /// Get reconstruction progress (0.0 to 1.0)
    pub fn progress(&self) -> f32 {
        if self.is_complete {
            1.0
        } else if self.stats.frames_processed == 0 {
            0.0
        } else {
            // Rough estimate based on frames processed vs expected
            (self.stats.frames_processed as f32
                / (self.stats.frames_processed + self.frame_queue.len() as u32) as f32)
                .min(0.95) // Never show 100% until complete
        }
    }

    /// Get reconstruction duration if available
    pub fn duration(&self) -> Option<std::time::Duration> {
        if let (Some(start), Some(end)) = (self.stats.start_time, self.stats.end_time) {
            Some(end.duration_since(start))
        } else {
            None
        }
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
        /// Number of patches applied
        count: usize,
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
    /// Get processing rate in frames per second
    pub fn frames_per_second(&self) -> Option<f32> {
        if let (Some(start), Some(end)) = (self.start_time, self.end_time) {
            let duration = end.duration_since(start);
            if duration.as_secs_f32() > 0.0 {
                Some(self.frames_processed as f32 / duration.as_secs_f32())
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Get patches per second rate
    pub fn patches_per_second(&self) -> Option<f32> {
        if let (Some(start), Some(end)) = (self.start_time, self.end_time) {
            let duration = end.duration_since(start);
            if duration.as_secs_f32() > 0.0 {
                Some(self.patches_applied as f32 / duration.as_secs_f32())
            } else {
                None
            }
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::Priority;
    use serde_json::json;

    #[test]
    fn test_reconstructor_creation() {
        let reconstructor = JsonReconstructor::new();
        assert!(!reconstructor.is_complete());
        assert_eq!(reconstructor.stats().frames_processed, 0);
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

        assert!(matches!(result, ProcessResult::SkeletonApplied));
        assert_eq!(reconstructor.current_state(), &skeleton);
        assert_eq!(reconstructor.stats().skeleton_frames, 1);
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
        let path = JsonPath::from_segments(vec![
            PathSegment::Root,
            PathSegment::Key("name".to_string()),
        ]);

        let patch = JsonPatch {
            path,
            operation: PatchOperation::Set {
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

        if let ProcessResult::PatchesApplied { count, .. } = result {
            assert_eq!(count, 1);
        } else {
            panic!("Expected PatchesApplied result");
        }

        let expected = json!({
            "name": "John Doe",
            "age": 0
        });

        assert_eq!(reconstructor.current_state(), &expected);
        assert_eq!(reconstructor.stats().patches_applied, 1);
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
        let path = JsonPath::from_segments(vec![
            PathSegment::Root,
            PathSegment::Key("items".to_string()),
        ]);

        let patch = JsonPatch {
            path,
            operation: PatchOperation::Append {
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
        assert!(matches!(result, ProcessResult::ReconstructionComplete));
        assert!(reconstructor.is_complete());
        assert!(reconstructor.stats().end_time.is_some());
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
        reconstructor.is_complete = true;
        reconstructor.stats.frames_processed = 5;

        // Reset
        reconstructor.reset();

        assert_eq!(reconstructor.current_state(), &JsonValue::Null);
        assert!(!reconstructor.is_complete());
        assert_eq!(reconstructor.stats().frames_processed, 0);
    }
}
