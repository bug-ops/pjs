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
    #[serde(with = "pjson_rs_domain::serde_support::priority")]
    pub priority: Priority,
    /// Additional metadata for processing
    pub metadata: HashMap<String, String>,
}

/// Stream processing result
#[derive(Debug, Clone)]
pub enum ProcessResult {
    /// Frame processed successfully and ready to stream to the client
    Processed(StreamFrame),
    /// Frame rejected: its serialized size exceeds the configured limit
    FrameTooLarge {
        /// Serialized size of the rejected frame in bytes
        size: usize,
        /// Configured [`StreamProcessorConfig::max_frame_size`]
        limit: usize,
    },
}

/// Stream processor configuration
#[derive(Debug, Clone)]
pub struct StreamProcessorConfig {
    /// Maximum frame size in bytes
    pub max_frame_size: usize,
}

impl Default for StreamProcessorConfig {
    fn default() -> Self {
        Self {
            max_frame_size: 1024 * 1024, // 1MB
        }
    }
}

/// Stream processor for handling PJS frames
#[derive(Debug)]
pub struct StreamProcessor {
    config: StreamProcessorConfig,
    processed_count: usize,
}

impl StreamProcessor {
    /// Create new stream processor
    pub fn new(config: StreamProcessorConfig) -> Self {
        Self {
            config,
            processed_count: 0,
        }
    }

    /// Process a stream frame.
    ///
    /// Each accepted frame immediately returns [`ProcessResult::Processed`] so
    /// callers can stream it to clients without waiting for a buffer to fill.
    /// Returns [`ProcessResult::FrameTooLarge`] only when the frame exceeds the
    /// configured size limit.
    pub fn process_frame(&mut self, frame: StreamFrame) -> DomainResult<ProcessResult> {
        let frame_size = serde_json::to_string(&frame.data)
            .map_err(|e| {
                crate::domain::DomainError::Logic(format!("JSON serialization failed: {e}"))
            })?
            .len();

        if frame_size > self.config.max_frame_size {
            return Ok(ProcessResult::FrameTooLarge {
                size: frame_size,
                limit: self.config.max_frame_size,
            });
        }

        self.processed_count += 1;
        Ok(ProcessResult::Processed(frame))
    }

    /// Get processing statistics
    pub fn stats(&self) -> StreamStats {
        StreamStats {
            processed_frames: self.processed_count,
        }
    }
}

/// Stream processing statistics
#[derive(Debug, Clone)]
pub struct StreamStats {
    /// Number of frames processed by the stream.
    pub processed_frames: usize,
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
    fn test_process_frame_returns_processed_immediately() {
        let config = StreamProcessorConfig::default();
        let mut processor = StreamProcessor::new(config);

        let frame = StreamFrame {
            data: json!({"test": "data"}),
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        };

        let result = processor.process_frame(frame).unwrap();
        assert!(matches!(result, ProcessResult::Processed(_)));

        let stats = processor.stats();
        assert_eq!(stats.processed_frames, 1);
    }

    #[test]
    fn test_processed_result_contains_original_frame() {
        let config = StreamProcessorConfig::default();
        let mut processor = StreamProcessor::new(config);

        let frame = StreamFrame {
            data: json!({"message": "hello"}),
            priority: Priority::HIGH,
            metadata: HashMap::new(),
        };

        match processor.process_frame(frame).unwrap() {
            ProcessResult::Processed(f) => {
                assert_eq!(f.priority, Priority::HIGH);
                assert_eq!(f.data, json!({"message": "hello"}));
            }
            other => panic!("Expected Processed, got {other:?}"),
        }
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
    fn test_oversized_frame_returns_error() {
        let config = StreamProcessorConfig {
            max_frame_size: 10, // very small limit
        };
        let mut processor = StreamProcessor::new(config);

        let frame = StreamFrame {
            data: json!({"large_key": "this value is definitely over ten bytes"}),
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        };

        let result = processor.process_frame(frame).unwrap();
        match result {
            ProcessResult::FrameTooLarge { size, limit } => {
                assert_eq!(limit, 10);
                assert!(size > limit);
            }
            other => panic!("Expected FrameTooLarge, got {other:?}"),
        }
    }

    fn frame_with_serialized_len(len: usize) -> StreamFrame {
        // A JSON string serializes to its content length plus two quotes.
        StreamFrame {
            data: JsonValue::String("a".repeat(len - 2)),
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn test_frame_exactly_at_limit_is_accepted() {
        let mut processor = StreamProcessor::new(StreamProcessorConfig { max_frame_size: 10 });

        let result = processor
            .process_frame(frame_with_serialized_len(10))
            .unwrap();
        assert!(matches!(result, ProcessResult::Processed(_)));
        assert_eq!(processor.stats().processed_frames, 1);
    }

    #[test]
    fn test_frame_one_byte_over_limit_is_rejected_and_not_counted() {
        let mut processor = StreamProcessor::new(StreamProcessorConfig { max_frame_size: 10 });

        let result = processor
            .process_frame(frame_with_serialized_len(11))
            .unwrap();
        assert!(matches!(
            result,
            ProcessResult::FrameTooLarge {
                size: 11,
                limit: 10
            }
        ));
        assert_eq!(processor.stats().processed_frames, 0);
    }

    #[test]
    fn test_multiple_frames_each_processed_immediately() {
        let config = StreamProcessorConfig::default();
        let mut processor = StreamProcessor::new(config);

        for i in 0..3u32 {
            let frame = StreamFrame {
                data: json!({"id": i}),
                priority: Priority::MEDIUM,
                metadata: HashMap::new(),
            };
            assert!(matches!(
                processor.process_frame(frame).unwrap(),
                ProcessResult::Processed(_)
            ));
        }

        assert_eq!(processor.stats().processed_frames, 3);
    }
}
