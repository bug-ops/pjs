//! Streaming functionality for PJS
//!
//! This module provides streaming capabilities for processing large JSON datasets
//! with priority-based delivery and incremental reconstruction.

use crate::{Frame, Result};

pub mod priority;
pub mod reconstruction;

pub use priority::{
    PriorityStreamer, StreamerConfig, Priority,
    JsonPath, JsonPatch, PatchOperation, StreamFrame, StreamingPlan, PathSegment
};
pub use reconstruction::{JsonReconstructor, ReconstructionStats, ProcessResult};

/// High-level stream processor that combines parsing with priority streaming
pub struct StreamProcessor {
    streamer: PriorityStreamer,
}

impl StreamProcessor {
    /// Create new stream processor
    pub fn new() -> Self {
        Self {
            streamer: PriorityStreamer::new(),
        }
    }
    
    /// Create with custom streamer configuration
    pub fn with_config(config: StreamerConfig) -> Self {
        Self {
            streamer: PriorityStreamer::with_config(config),
        }
    }

    /// Process JSON data into prioritized streaming frames
    pub fn process_json(&self, json_data: &[u8]) -> Result<StreamingPlan> {
        // Parse JSON using serde_json for now
        let json: serde_json::Value = serde_json::from_slice(json_data)
            .map_err(|e| crate::Error::invalid_json(0, e.to_string()))?;
        
        // Create streaming plan
        self.streamer.analyze(&json)
    }

    /// Legacy frame processing for compatibility
    pub fn process(&self, _frames: Vec<Frame>) -> Result<Vec<Frame>> {
        // TODO: Implement frame-to-frame processing if needed
        Ok(vec![])
    }
    
    /// Get reference to internal streamer
    pub fn streamer(&self) -> &PriorityStreamer {
        &self.streamer
    }
}

impl Default for StreamProcessor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_processor_creation() {
        let _processor = StreamProcessor::new();
    }
}