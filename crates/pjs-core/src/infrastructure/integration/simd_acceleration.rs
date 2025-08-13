// SIMD acceleration module using sonic-rs for high-performance JSON operations
//
// This module provides SIMD-accelerated JSON processing for streaming operations,
// significantly improving performance for large-scale data serialization.

use crate::stream::StreamFrame;
use bytes::{BufMut, BytesMut};
use sonic_rs::{JsonValueTrait, LazyValue};

/// High-performance SIMD-accelerated serializer for stream frames
pub struct SimdFrameSerializer {
    /// Pre-allocated buffer for serialization
    buffer: BytesMut,
    /// Statistics for performance monitoring
    stats: SerializationStats,
}

#[derive(Debug, Clone, Default)]
pub struct SerializationStats {
    pub frames_processed: usize,
    pub bytes_written: usize,
    pub simd_operations: usize,
}

impl SimdFrameSerializer {
    /// Create a new SIMD serializer with initial capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buffer: BytesMut::with_capacity(capacity),
            stats: SerializationStats::default(),
        }
    }

    /// Serialize a single frame using SIMD acceleration
    pub fn serialize_frame(&mut self, frame: &StreamFrame) -> Result<&[u8], sonic_rs::Error> {
        self.buffer.clear();

        // Use sonic-rs for SIMD-accelerated serialization
        let serialized = sonic_rs::to_vec(frame)?;
        self.buffer.extend_from_slice(&serialized);

        self.stats.frames_processed += 1;
        self.stats.bytes_written += self.buffer.len();
        self.stats.simd_operations += 1;

        Ok(&self.buffer)
    }

    /// Batch serialize multiple frames with SIMD optimization
    pub fn serialize_batch(&mut self, frames: &[StreamFrame]) -> Result<BytesMut, sonic_rs::Error> {
        // Estimate capacity for better performance
        let estimated_size = frames.len() * 256; // ~256 bytes per frame estimate
        let mut output = BytesMut::with_capacity(estimated_size);

        for frame in frames {
            let serialized = sonic_rs::to_vec(frame)?;
            output.extend_from_slice(&serialized);
            output.put_u8(b'\n'); // NDJSON separator
        }

        self.stats.frames_processed += frames.len();
        self.stats.bytes_written += output.len();
        self.stats.simd_operations += frames.len();

        Ok(output)
    }

    /// Serialize frames to Server-Sent Events format with SIMD
    pub fn serialize_sse_batch(
        &mut self,
        frames: &[StreamFrame],
    ) -> Result<BytesMut, sonic_rs::Error> {
        let estimated_size = frames.len() * 300; // SSE overhead + JSON
        let mut output = BytesMut::with_capacity(estimated_size);

        for frame in frames {
            output.extend_from_slice(b"data: ");
            let serialized = sonic_rs::to_vec(frame)?;
            output.extend_from_slice(&serialized);
            output.extend_from_slice(b"\n\n");
        }

        self.stats.frames_processed += frames.len();
        self.stats.bytes_written += output.len();
        self.stats.simd_operations += frames.len();

        Ok(output)
    }

    /// Get serialization statistics
    pub fn stats(&self) -> &SerializationStats {
        &self.stats
    }

    /// Reset statistics
    pub fn reset_stats(&mut self) {
        self.stats = SerializationStats::default();
    }
}

/// SIMD-accelerated JSON validator and parser
pub struct SimdJsonProcessor;

impl SimdJsonProcessor {
    /// Validate JSON using SIMD acceleration
    pub fn validate_json(input: &[u8]) -> Result<(), sonic_rs::Error> {
        // sonic-rs uses SIMD for ultra-fast validation
        let _doc = sonic_rs::from_slice::<sonic_rs::Value>(input)?;
        Ok(())
    }

    /// Parse and extract specific fields using SIMD with zero-copy where possible
    pub fn extract_priority_field(input: &[u8]) -> Result<Option<u8>, sonic_rs::Error> {
        let doc = sonic_rs::from_slice::<LazyValue<'_>>(input)?;

        if let Some(priority_value) = doc.get("priority")
            && let Some(priority) = priority_value.as_u64()
        {
            return Ok(Some(priority as u8));
        }

        Ok(None)
    }

    /// Batch validate multiple JSON documents
    pub fn validate_batch(inputs: &[&[u8]]) -> Vec<Result<(), sonic_rs::Error>> {
        inputs
            .iter()
            .map(|input| Self::validate_json(input))
            .collect()
    }
}

/// SIMD-optimized buffer for stream processing
pub struct SimdStreamBuffer {
    /// Main data buffer, AVX-512 aligned
    data: BytesMut,
    /// Position tracking for efficient processing
    position: usize,
    /// Capacity management
    capacity: usize,
}

impl SimdStreamBuffer {
    /// Create a new SIMD-optimized buffer with specified capacity
    pub fn with_capacity(capacity: usize) -> Self {
        // Ensure capacity is aligned for SIMD operations
        let aligned_capacity = (capacity + 63) & !63; // Align to 64 bytes for AVX-512

        Self {
            data: BytesMut::with_capacity(aligned_capacity),
            position: 0,
            capacity: aligned_capacity,
        }
    }

    /// Write a frame to buffer using SIMD serialization
    pub fn write_frame(&mut self, frame: &StreamFrame) -> Result<usize, sonic_rs::Error> {
        let start_pos = self.data.len();

        // Ensure we have enough space
        self.ensure_capacity(512); // Reserve space for frame

        let serialized = sonic_rs::to_vec(frame)?;
        self.data.extend_from_slice(&serialized);

        let bytes_written = self.data.len() - start_pos;
        Ok(bytes_written)
    }

    /// Write multiple frames efficiently
    pub fn write_frames(&mut self, frames: &[StreamFrame]) -> Result<usize, sonic_rs::Error> {
        let start_len = self.data.len();

        // Pre-allocate space for all frames
        self.ensure_capacity(frames.len() * 256);

        for frame in frames {
            let serialized = sonic_rs::to_vec(frame)?;
            self.data.extend_from_slice(&serialized);
            self.data.put_u8(b'\n');
        }

        Ok(self.data.len() - start_len)
    }

    /// Get the current buffer contents
    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }

    /// Consume the buffer and return bytes
    pub fn into_bytes(self) -> bytes::Bytes {
        self.data.freeze()
    }

    /// Clear the buffer for reuse
    pub fn clear(&mut self) {
        self.data.clear();
        self.position = 0;
    }

    /// Ensure buffer has at least the specified additional capacity
    fn ensure_capacity(&mut self, additional: usize) {
        if self.data.len() + additional > self.capacity {
            let new_capacity = ((self.data.len() + additional) * 2 + 63) & !63;
            self.data.reserve(new_capacity - self.data.capacity());
            self.capacity = new_capacity;
        }
    }
}

/// Configuration for SIMD operations
#[derive(Debug, Clone)]
pub struct SimdConfig {
    /// Buffer size for batch operations
    pub batch_size: usize,
    /// Initial capacity for buffers
    pub initial_capacity: usize,
    /// Enable statistics collection
    pub collect_stats: bool,
}

impl Default for SimdConfig {
    fn default() -> Self {
        Self {
            batch_size: 100,
            initial_capacity: 8192, // 8KB initial capacity
            collect_stats: false,
        }
    }
}

/// High-level SIMD streaming interface
pub struct SimdStreamProcessor {
    serializer: SimdFrameSerializer,
    buffer: SimdStreamBuffer,
    config: SimdConfig,
}

impl SimdStreamProcessor {
    /// Create a new SIMD stream processor
    pub fn new(config: SimdConfig) -> Self {
        Self {
            serializer: SimdFrameSerializer::with_capacity(config.initial_capacity),
            buffer: SimdStreamBuffer::with_capacity(config.initial_capacity),
            config,
        }
    }

    /// Process frames to JSON format with SIMD acceleration
    pub fn process_to_json(
        &mut self,
        frames: &[StreamFrame],
    ) -> Result<bytes::Bytes, sonic_rs::Error> {
        let result = self.serializer.serialize_batch(frames)?;
        Ok(result.freeze())
    }

    /// Process frames to Server-Sent Events format
    pub fn process_to_sse(
        &mut self,
        frames: &[StreamFrame],
    ) -> Result<bytes::Bytes, sonic_rs::Error> {
        let result = self.serializer.serialize_sse_batch(frames)?;
        Ok(result.freeze())
    }

    /// Process frames to NDJSON with buffered approach
    pub fn process_to_ndjson(
        &mut self,
        frames: &[StreamFrame],
    ) -> Result<bytes::Bytes, sonic_rs::Error> {
        self.buffer.clear();
        self.buffer.write_frames(frames)?;
        let data = self.buffer.as_slice().to_vec();
        Ok(data.into())
    }

    /// Get processing statistics if enabled
    pub fn stats(&self) -> Option<&SerializationStats> {
        if self.config.collect_stats {
            Some(self.serializer.stats())
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Priority;
    use std::collections::HashMap;

    #[test]
    fn test_simd_frame_serialization() {
        let mut serializer = SimdFrameSerializer::with_capacity(1024);

        let frame = StreamFrame {
            data: serde_json::json!({"test": "data", "priority": "high"}),
            priority: Priority::HIGH,
            metadata: HashMap::new(),
        };

        let result = serializer.serialize_frame(&frame);
        assert!(result.is_ok());

        let serialized = result.unwrap();
        assert!(!serialized.is_empty());

        // Verify we can deserialize back
        let parsed: serde_json::Value = sonic_rs::from_slice(serialized).unwrap();
        assert_eq!(parsed["data"]["test"], "data");
    }

    #[test]
    fn test_batch_serialization() {
        let mut serializer = SimdFrameSerializer::with_capacity(2048);

        let frames = vec![
            StreamFrame {
                data: serde_json::json!({"id": 1}),
                priority: Priority::HIGH,
                metadata: HashMap::new(),
            },
            StreamFrame {
                data: serde_json::json!({"id": 2}),
                priority: Priority::MEDIUM,
                metadata: HashMap::new(),
            },
        ];

        let result = serializer.serialize_batch(&frames);
        assert!(result.is_ok());

        let serialized = result.unwrap();
        assert!(!serialized.is_empty());

        // Should contain both frames separated by newlines
        let content = String::from_utf8(serialized.to_vec()).unwrap();
        assert!(content.contains("\"id\":1"));
        assert!(content.contains("\"id\":2"));
    }

    #[test]
    fn test_simd_json_validation() {
        let valid_json = br#"{"test": "value", "number": 42}"#;
        let invalid_json = br#"{"test": "value", "number": 42"#;

        assert!(SimdJsonProcessor::validate_json(valid_json).is_ok());
        assert!(SimdJsonProcessor::validate_json(invalid_json).is_err());
    }

    #[test]
    fn test_priority_extraction() {
        let json_with_priority = br#"{"data": "test", "priority": 5}"#;
        let json_without_priority = br#"{"data": "test"}"#;

        let result1 = SimdJsonProcessor::extract_priority_field(json_with_priority).unwrap();
        assert_eq!(result1, Some(5));

        let result2 = SimdJsonProcessor::extract_priority_field(json_without_priority).unwrap();
        assert_eq!(result2, None);
    }

    #[test]
    fn test_simd_stream_buffer() {
        let mut buffer = SimdStreamBuffer::with_capacity(1024);

        let frame = StreamFrame {
            data: serde_json::json!({"buffer_test": true}),
            priority: Priority::HIGH,
            metadata: HashMap::new(),
        };

        let bytes_written = buffer.write_frame(&frame).unwrap();
        assert!(bytes_written > 0);

        let content = buffer.as_slice();
        assert!(!content.is_empty());

        // Verify content is valid JSON
        let parsed: serde_json::Value = sonic_rs::from_slice(content).unwrap();
        assert_eq!(parsed["data"]["buffer_test"], true);
    }

    #[test]
    fn test_full_simd_processor() {
        let config = SimdConfig {
            batch_size: 50,
            initial_capacity: 2048,
            collect_stats: true,
        };

        let mut processor = SimdStreamProcessor::new(config);

        let frames = vec![StreamFrame {
            data: serde_json::json!({"processor": "test", "id": 1}),
            priority: Priority::CRITICAL,
            metadata: HashMap::new(),
        }];

        let json_result = processor.process_to_json(&frames);
        assert!(json_result.is_ok());

        let sse_result = processor.process_to_sse(&frames);
        assert!(sse_result.is_ok());

        // Verify SSE format
        let sse_content = String::from_utf8(sse_result.unwrap().to_vec()).unwrap();
        assert!(sse_content.starts_with("data: "));
        assert!(sse_content.ends_with("\n\n"));

        // Check stats if enabled
        if let Some(stats) = processor.stats() {
            assert!(stats.frames_processed > 0);
        }
    }
}
