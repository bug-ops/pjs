//! Streaming system for PJS protocol
//!
//! Provides frame-based streaming with priority-aware processing
//! and compression integration.

pub mod compression_integration;
pub mod priority;
pub mod reconstruction;

use crate::domain::{DomainResult, Priority};
use serde_json::Value as JsonValue;
use std::collections::HashMap;

/// Stream frame with priority and metadata
#[derive(Debug, Clone, serde::Serialize)]
pub struct StreamFrame {
    /// JSON data payload
    pub data: JsonValue,
    /// Frame priority for streaming order
    pub priority: Priority,
    /// Additional metadata for processing
    pub metadata: HashMap<String, String>,
}

/// Stream processing result
#[derive(Debug, Clone)]
pub enum ProcessResult {
    /// Frame processed successfully
    Processed(StreamFrame),
    /// Frame needs more data to process
    Incomplete,
    /// Processing completed
    Complete(Vec<StreamFrame>),
    /// Processing failed
    Error(String),
}

/// Stream processor configuration
#[derive(Debug, Clone)]
pub struct StreamConfig {
    /// Maximum frame size in bytes
    pub max_frame_size: usize,
    /// Buffer size for incomplete frames
    pub buffer_size: usize,
    /// Enable compression
    pub enable_compression: bool,
    /// Priority threshold for critical frames
    pub priority_threshold: u8,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            max_frame_size: 1024 * 1024, // 1MB
            buffer_size: 64,
            enable_compression: true,
            priority_threshold: Priority::HIGH.value(),
        }
    }
}

/// Stream processor for handling PJS frames
#[derive(Debug)]
pub struct StreamProcessor {
    config: StreamConfig,
    buffer: Vec<StreamFrame>,
    processed_count: usize,
}

impl StreamProcessor {
    /// Create new stream processor
    pub fn new(config: StreamConfig) -> Self {
        Self {
            config,
            buffer: Vec::new(),
            processed_count: 0,
        }
    }

    /// Process a stream frame
    pub fn process_frame(&mut self, frame: StreamFrame) -> DomainResult<ProcessResult> {
        // Check frame size
        let frame_size = serde_json::to_string(&frame.data)
            .map_err(|e| {
                crate::domain::DomainError::Logic(format!("JSON serialization failed: {e}"))
            })?
            .len();

        if frame_size > self.config.max_frame_size {
            return Ok(ProcessResult::Error(format!(
                "Frame size {} exceeds maximum {}",
                frame_size, self.config.max_frame_size
            )));
        }

        // Add to buffer
        self.buffer.push(frame);
        self.processed_count += 1;

        // Check if we should flush buffer
        if self.buffer.len() >= self.config.buffer_size {
            let frames = self.buffer.drain(..).collect();
            Ok(ProcessResult::Complete(frames))
        } else {
            Ok(ProcessResult::Incomplete)
        }
    }

    /// Flush remaining frames
    pub fn flush(&mut self) -> Vec<StreamFrame> {
        self.buffer.drain(..).collect()
    }

    /// Get processing statistics
    pub fn stats(&self) -> StreamStats {
        StreamStats {
            processed_frames: self.processed_count,
            buffered_frames: self.buffer.len(),
            buffer_utilization: self.buffer.len() as f32 / self.config.buffer_size as f32,
        }
    }
}

/// Stream processing statistics
#[derive(Debug, Clone)]
pub struct StreamStats {
    pub processed_frames: usize,
    pub buffered_frames: usize,
    pub buffer_utilization: f32,
}

// Re-export key types
pub use compression_integration::{
    CompressedFrame, CompressionStats, DecompressionMetadata, DecompressionStats,
    StreamingCompressor, StreamingDecompressor,
};
pub use priority::{PriorityStreamFrame, PriorityStreamer};
pub use reconstruction::JsonReconstructor;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_stream_processor_basic() {
        let config = StreamConfig::default();
        let mut processor = StreamProcessor::new(config);

        let frame = StreamFrame {
            data: json!({"test": "data"}),
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        };

        let result = processor.process_frame(frame).unwrap();
        assert!(matches!(result, ProcessResult::Incomplete));

        let stats = processor.stats();
        assert_eq!(stats.processed_frames, 1);
        assert_eq!(stats.buffered_frames, 1);
    }

    #[test]
    fn test_stream_frame_creation() {
        let frame = StreamFrame {
            data: json!({"message": "hello"}),
            priority: Priority::HIGH,
            metadata: {
                let mut meta = HashMap::new();
                meta.insert("source".to_string(), "test".to_string());
                meta
            },
        };

        assert_eq!(frame.priority, Priority::HIGH);
        assert_eq!(frame.metadata.get("source"), Some(&"test".to_string()));
    }

    #[test]
    fn test_buffer_flush() {
        let config = StreamConfig {
            buffer_size: 2,
            ..StreamConfig::default()
        };
        let mut processor = StreamProcessor::new(config);

        // Add first frame
        let frame1 = StreamFrame {
            data: json!({"id": 1}),
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        };
        let result1 = processor.process_frame(frame1).unwrap();
        assert!(matches!(result1, ProcessResult::Incomplete));

        // Add second frame - should trigger flush
        let frame2 = StreamFrame {
            data: json!({"id": 2}),
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        };
        let result2 = processor.process_frame(frame2).unwrap();

        match result2 {
            ProcessResult::Complete(frames) => {
                assert_eq!(frames.len(), 2);
            }
            _ => panic!("Expected Complete with 2 frames"),
        }
    }
}
