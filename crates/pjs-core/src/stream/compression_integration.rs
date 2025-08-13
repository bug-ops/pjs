//! Integration of schema-based compression with PJS streaming
//!
//! Provides streaming-aware compression that maintains the ability
//! to progressively decompress data as frames arrive.

use crate::{
    compression::{CompressedData, CompressionStrategy, SchemaCompressor},
    domain::{DomainError, DomainResult},
    stream::{Priority, StreamFrame},
};
use serde_json::Value as JsonValue;
use std::collections::HashMap;

/// Streaming compressor that maintains compression state across frames
#[derive(Debug, Clone)]
pub struct StreamingCompressor {
    /// Primary compressor for skeleton and critical data
    skeleton_compressor: SchemaCompressor,
    /// Secondary compressor for non-critical data
    content_compressor: SchemaCompressor,
    /// Compression statistics
    stats: CompressionStats,
}

#[derive(Debug, Clone, Default)]
pub struct CompressionStats {
    /// Total bytes processed
    pub total_input_bytes: usize,
    /// Total bytes after compression
    pub total_output_bytes: usize,
    /// Number of frames processed
    pub frames_processed: u32,
    /// Compression ratio by priority level
    pub priority_ratios: HashMap<u8, f32>,
}

/// Compressed stream frame with metadata
#[derive(Debug, Clone)]
pub struct CompressedFrame {
    /// Original frame metadata
    pub frame: StreamFrame,
    /// Compressed data
    pub compressed_data: CompressedData,
    /// Decompression instructions for client
    pub decompression_metadata: DecompressionMetadata,
}

#[derive(Debug, Clone)]
pub struct DecompressionMetadata {
    /// Compression strategy used
    pub strategy: CompressionStrategy,
    /// Dictionary indices mapping
    pub dictionary_map: HashMap<u16, String>,
    /// Delta base values for numeric decompression
    pub delta_bases: HashMap<String, f64>,
    /// Priority-specific decompression hints
    pub priority_hints: HashMap<u8, String>,
}

impl StreamingCompressor {
    /// Create new streaming compressor
    pub fn new() -> Self {
        Self {
            skeleton_compressor: SchemaCompressor::new(),
            content_compressor: SchemaCompressor::new(),
            stats: CompressionStats::default(),
        }
    }

    /// Create with custom compression strategies
    pub fn with_strategies(
        skeleton_strategy: CompressionStrategy,
        content_strategy: CompressionStrategy,
    ) -> Self {
        Self {
            skeleton_compressor: SchemaCompressor::with_strategy(skeleton_strategy),
            content_compressor: SchemaCompressor::with_strategy(content_strategy),
            stats: CompressionStats::default(),
        }
    }

    /// Process and compress a stream frame based on its priority
    pub fn compress_frame(&mut self, frame: StreamFrame) -> DomainResult<CompressedFrame> {
        let compressor = self.select_compressor_for_priority(frame.priority);

        // Calculate original size
        let original_size = serde_json::to_string(&frame.data)
            .map_err(|e| DomainError::CompressionError(format!("JSON serialization failed: {e}")))?
            .len();

        // Compress based on frame content and priority
        let compressed_data = compressor.compress(&frame.data)?;

        // Update statistics
        self.update_stats(
            frame.priority,
            original_size,
            compressed_data.compressed_size,
        );

        // Create decompression metadata
        let decompression_metadata = self.create_decompression_metadata(&compressed_data)?;

        Ok(CompressedFrame {
            frame,
            compressed_data,
            decompression_metadata,
        })
    }

    /// Analyze JSON data to optimize compression strategies
    pub fn optimize_for_data(
        &mut self,
        skeleton: &JsonValue,
        sample_data: &[JsonValue],
    ) -> DomainResult<()> {
        // Optimize skeleton compressor for critical structural data
        self.skeleton_compressor.analyze_and_optimize(skeleton)?;

        // Analyze sample content data to optimize content compressor
        if !sample_data.is_empty() {
            // Combine samples for comprehensive analysis
            let combined_sample = JsonValue::Array(sample_data.to_vec());
            self.content_compressor
                .analyze_and_optimize(&combined_sample)?;
        }

        Ok(())
    }

    /// Get current compression statistics
    pub fn get_stats(&self) -> &CompressionStats {
        &self.stats
    }

    /// Reset compression statistics
    pub fn reset_stats(&mut self) {
        self.stats = CompressionStats::default();
    }

    /// Select appropriate compressor based on frame priority
    fn select_compressor_for_priority(&mut self, priority: Priority) -> &mut SchemaCompressor {
        match priority {
            // Critical data (skeleton, errors) - use specialized compressor
            Priority::CRITICAL | Priority::HIGH => &mut self.skeleton_compressor,
            // Regular content data - use content compressor
            _ => &mut self.content_compressor,
        }
    }

    /// Update compression statistics
    fn update_stats(&mut self, priority: Priority, original_size: usize, compressed_size: usize) {
        self.stats.total_input_bytes += original_size;
        self.stats.total_output_bytes += compressed_size;
        self.stats.frames_processed += 1;

        let ratio = if original_size > 0 {
            compressed_size as f32 / original_size as f32
        } else {
            1.0
        };

        self.stats.priority_ratios.insert(priority.value(), ratio);
    }

    /// Create decompression metadata for client
    fn create_decompression_metadata(
        &self,
        compressed_data: &CompressedData,
    ) -> DomainResult<DecompressionMetadata> {
        let mut dictionary_map = HashMap::new();
        let mut delta_bases = HashMap::new();

        // Extract dictionary mappings
        for (key, value) in &compressed_data.compression_metadata {
            if key.starts_with("dict_") {
                if let Ok(index) = key.strip_prefix("dict_").unwrap().parse::<u16>()
                    && let Some(string_val) = value.as_str()
                {
                    dictionary_map.insert(index, string_val.to_string());
                }
            } else if key.starts_with("base_") {
                let path = key.strip_prefix("base_").unwrap();
                if let Some(num) = value.as_f64() {
                    delta_bases.insert(path.to_string(), num);
                }
            }
        }

        Ok(DecompressionMetadata {
            strategy: compressed_data.strategy.clone(),
            dictionary_map,
            delta_bases,
            priority_hints: HashMap::new(), // TODO: Add priority-specific hints
        })
    }
}

impl CompressionStats {
    /// Calculate overall compression ratio
    pub fn overall_compression_ratio(&self) -> f32 {
        if self.total_input_bytes == 0 {
            return 1.0;
        }
        self.total_output_bytes as f32 / self.total_input_bytes as f32
    }

    /// Get compression ratio for specific priority level
    pub fn priority_compression_ratio(&self, priority: u8) -> f32 {
        self.priority_ratios.get(&priority).copied().unwrap_or(1.0)
    }

    /// Calculate bytes saved
    pub fn bytes_saved(&self) -> isize {
        self.total_input_bytes as isize - self.total_output_bytes as isize
    }

    /// Calculate percentage saved
    pub fn percentage_saved(&self) -> f32 {
        if self.total_input_bytes == 0 {
            return 0.0;
        }
        let ratio = self.overall_compression_ratio();
        (1.0 - ratio) * 100.0
    }
}

/// Client-side decompressor for receiving compressed frames
#[derive(Debug, Clone)]
pub struct StreamingDecompressor {
    /// Active dictionary for string decompression
    active_dictionary: HashMap<u16, String>,
    /// Delta base values for numeric decompression  
    delta_bases: HashMap<String, f64>,
    /// Decompression statistics
    stats: DecompressionStats,
}

#[derive(Debug, Clone, Default)]
pub struct DecompressionStats {
    /// Total frames decompressed
    pub frames_decompressed: u32,
    /// Total bytes decompressed
    pub total_decompressed_bytes: usize,
    /// Average decompression time in microseconds
    pub avg_decompression_time_us: u64,
}

impl StreamingDecompressor {
    /// Create new streaming decompressor
    pub fn new() -> Self {
        Self {
            active_dictionary: HashMap::new(),
            delta_bases: HashMap::new(),
            stats: DecompressionStats::default(),
        }
    }

    /// Decompress a compressed frame
    pub fn decompress_frame(
        &mut self,
        compressed_frame: CompressedFrame,
    ) -> DomainResult<StreamFrame> {
        let start_time = std::time::Instant::now();

        // Update decompression context with metadata
        self.update_context(&compressed_frame.decompression_metadata)?;

        // Decompress data based on strategy
        let decompressed_data = self.decompress_data(
            &compressed_frame.compressed_data,
            &compressed_frame.decompression_metadata.strategy,
        )?;

        // Update statistics
        let decompression_time = start_time.elapsed();
        self.update_decompression_stats(&decompressed_data, decompression_time);

        Ok(StreamFrame {
            data: decompressed_data,
            priority: compressed_frame.frame.priority,
            metadata: compressed_frame.frame.metadata,
        })
    }

    /// Update decompression context with new metadata
    fn update_context(&mut self, metadata: &DecompressionMetadata) -> DomainResult<()> {
        // Update dictionary
        for (&index, string) in &metadata.dictionary_map {
            self.active_dictionary.insert(index, string.clone());
        }

        // Update delta bases
        for (path, &base) in &metadata.delta_bases {
            self.delta_bases.insert(path.clone(), base);
        }

        Ok(())
    }

    /// Decompress data according to strategy
    fn decompress_data(
        &self,
        compressed_data: &CompressedData,
        strategy: &CompressionStrategy,
    ) -> DomainResult<JsonValue> {
        match strategy {
            CompressionStrategy::None => Ok(compressed_data.data.clone()),

            CompressionStrategy::Dictionary { .. } => {
                self.decompress_dictionary(&compressed_data.data)
            }

            CompressionStrategy::Delta { .. } => self.decompress_delta(&compressed_data.data),

            CompressionStrategy::RunLength => self.decompress_run_length(&compressed_data.data),

            CompressionStrategy::Hybrid { .. } => {
                // Apply decompression in reverse order: delta first, then dictionary
                let delta_decompressed = self.decompress_delta(&compressed_data.data)?;
                self.decompress_dictionary(&delta_decompressed)
            }
        }
    }

    /// Decompress dictionary-encoded strings
    fn decompress_dictionary(&self, data: &JsonValue) -> DomainResult<JsonValue> {
        match data {
            JsonValue::Object(obj) => {
                let mut decompressed = serde_json::Map::new();
                for (key, value) in obj {
                    decompressed.insert(key.clone(), self.decompress_dictionary(value)?);
                }
                Ok(JsonValue::Object(decompressed))
            }
            JsonValue::Array(arr) => {
                let decompressed: Result<Vec<_>, _> = arr
                    .iter()
                    .map(|item| self.decompress_dictionary(item))
                    .collect();
                Ok(JsonValue::Array(decompressed?))
            }
            JsonValue::Number(n) => {
                // Check if this is a dictionary index
                if let Some(index) = n.as_u64()
                    && let Some(string_val) = self.active_dictionary.get(&(index as u16))
                {
                    return Ok(JsonValue::String(string_val.clone()));
                }
                Ok(data.clone())
            }
            _ => Ok(data.clone()),
        }
    }

    /// Decompress delta-encoded values
    fn decompress_delta(&self, data: &JsonValue) -> DomainResult<JsonValue> {
        // TODO: Implement delta decompression for numeric sequences
        // This would reconstruct original values from deltas and base values
        Ok(data.clone())
    }

    /// Decompress run-length encoded data
    fn decompress_run_length(&self, data: &JsonValue) -> DomainResult<JsonValue> {
        // TODO: Implement run-length decompression
        Ok(data.clone())
    }

    /// Update decompression statistics
    fn update_decompression_stats(&mut self, data: &JsonValue, duration: std::time::Duration) {
        self.stats.frames_decompressed += 1;

        if let Ok(serialized) = serde_json::to_string(data) {
            self.stats.total_decompressed_bytes += serialized.len();
        }

        let new_time_us = duration.as_micros() as u64;
        if self.stats.frames_decompressed == 1 {
            self.stats.avg_decompression_time_us = new_time_us;
        } else {
            // Calculate running average
            let total_frames = self.stats.frames_decompressed as u64;
            let total_time =
                self.stats.avg_decompression_time_us * (total_frames - 1) + new_time_us;
            self.stats.avg_decompression_time_us = total_time / total_frames;
        }
    }

    /// Get decompression statistics
    pub fn get_stats(&self) -> &DecompressionStats {
        &self.stats
    }
}

impl Default for StreamingCompressor {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for StreamingDecompressor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_streaming_compressor_basic() {
        let mut compressor = StreamingCompressor::new();

        let frame = StreamFrame {
            data: json!({
                "message": "test message",
                "count": 42
            }),
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        };

        let result = compressor.compress_frame(frame);
        assert!(result.is_ok());

        let compressed = result.unwrap();
        assert_eq!(compressed.frame.priority, Priority::MEDIUM);
    }

    #[test]
    fn test_compression_stats() {
        let stats = CompressionStats {
            total_input_bytes: 1000,
            total_output_bytes: 600,
            ..Default::default()
        };

        assert_eq!(stats.overall_compression_ratio(), 0.6);
        assert_eq!(stats.bytes_saved(), 400);
        // Use approximate comparison for float precision
        let percentage = stats.percentage_saved();
        assert!((percentage - 40.0).abs() < 0.001);
    }

    #[test]
    fn test_streaming_decompressor_basic() {
        let mut decompressor = StreamingDecompressor::new();

        let compressed_frame = CompressedFrame {
            frame: StreamFrame {
                data: json!({"test": "data"}),
                priority: Priority::MEDIUM,
                metadata: HashMap::new(),
            },
            compressed_data: CompressedData {
                strategy: CompressionStrategy::None,
                compressed_size: 20,
                data: json!({"test": "data"}),
                compression_metadata: HashMap::new(),
            },
            decompression_metadata: DecompressionMetadata {
                strategy: CompressionStrategy::None,
                dictionary_map: HashMap::new(),
                delta_bases: HashMap::new(),
                priority_hints: HashMap::new(),
            },
        };

        let result = decompressor.decompress_frame(compressed_frame);
        assert!(result.is_ok());

        let decompressed = result.unwrap();
        assert_eq!(decompressed.data, json!({"test": "data"}));
    }

    #[test]
    fn test_dictionary_decompression() {
        let mut decompressor = StreamingDecompressor::new();
        decompressor
            .active_dictionary
            .insert(0, "hello".to_string());
        decompressor
            .active_dictionary
            .insert(1, "world".to_string());

        // Test with dictionary indices
        let compressed = json!({
            "greeting": 0,
            "target": 1
        });

        let result = decompressor.decompress_dictionary(&compressed).unwrap();
        assert_eq!(
            result,
            json!({
                "greeting": "hello",
                "target": "world"
            })
        );
    }

    #[test]
    fn test_priority_based_compression() {
        let mut compressor = StreamingCompressor::new();

        let critical_frame = StreamFrame {
            data: json!({"error": "critical failure"}),
            priority: Priority::CRITICAL,
            metadata: HashMap::new(),
        };

        let low_frame = StreamFrame {
            data: json!({"debug": "verbose information"}),
            priority: Priority::LOW,
            metadata: HashMap::new(),
        };

        let _critical_result = compressor.compress_frame(critical_frame).unwrap();
        let _low_result = compressor.compress_frame(low_frame).unwrap();

        let stats = compressor.get_stats();
        assert_eq!(stats.frames_processed, 2);
        assert!(stats.total_input_bytes > 0);
    }
}
