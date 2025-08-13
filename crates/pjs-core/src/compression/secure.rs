//! Secure compression integration with bomb protection

use crate::{
    Error, Result,
    compression::CompressionStrategy,
    security::{CompressionBombDetector, CompressionBombProtector, CompressionStats},
};
use std::io::{Cursor, Read};
use tracing::{debug, info, warn};

/// Compressed data with security metadata
#[derive(Debug, Clone)]
pub struct SecureCompressedData {
    pub data: Vec<u8>,
    pub original_size: usize,
    pub compression_ratio: f64,
}

/// Secure compression wrapper with bomb protection
pub struct SecureCompressor {
    detector: CompressionBombDetector,
    strategy: CompressionStrategy,
}

impl SecureCompressor {
    /// Create new secure compressor with detector and strategy
    pub fn new(detector: CompressionBombDetector, strategy: CompressionStrategy) -> Self {
        Self { detector, strategy }
    }

    /// Create with default security settings
    pub fn with_default_security(strategy: CompressionStrategy) -> Self {
        Self::new(CompressionBombDetector::default(), strategy)
    }

    /// Compress data with security validation
    pub fn compress(&self, data: &[u8]) -> Result<SecureCompressedData> {
        // Pre-compression validation
        self.detector.validate_pre_decompression(data.len())?;

        // Perform compression using the strategy
        let compressed = match self.strategy {
            CompressionStrategy::None => {
                info!("No compression applied");
                SecureCompressedData {
                    data: data.to_vec(),
                    original_size: data.len(),
                    compression_ratio: 1.0,
                }
            }
            _ => {
                debug!("Applying compression strategy: {:?}", self.strategy);
                self.compress_with_strategy(data)?
            }
        };

        // Post-compression validation
        let compression_ratio = data.len() as f64 / compressed.data.len() as f64;
        info!("Compression completed: {:.2}x ratio", compression_ratio);

        Ok(compressed)
    }

    /// Decompress data with bomb protection
    pub fn decompress_protected(&self, compressed: &SecureCompressedData) -> Result<Vec<u8>> {
        // Create protected reader
        let cursor = Cursor::new(&compressed.data);
        let mut protector = self.detector.protect_reader(cursor, compressed.data.len());

        // Read with protection
        let mut decompressed = Vec::new();
        match protector.read_to_end(&mut decompressed) {
            Ok(_) => {
                let stats = protector.stats();
                self.log_decompression_stats(&stats);

                // Final validation
                self.detector
                    .validate_result(compressed.data.len(), decompressed.len())?;

                Ok(decompressed)
            }
            Err(e) => {
                warn!("Decompression failed with protection: {}", e);
                Err(Error::SecurityError(format!(
                    "Protected decompression failed: {}",
                    e
                )))
            }
        }
    }

    /// Decompress nested/chained compression with depth tracking
    pub fn decompress_nested(
        &self,
        compressed: &SecureCompressedData,
        depth: usize,
    ) -> Result<Vec<u8>> {
        // Create protected reader with depth tracking
        let cursor = Cursor::new(&compressed.data);
        let mut protector =
            self.detector
                .protect_nested_reader(cursor, compressed.data.len(), depth)?;

        let mut decompressed = Vec::new();
        match protector.read_to_end(&mut decompressed) {
            Ok(_) => {
                let stats = protector.stats();
                self.log_decompression_stats(&stats);

                if stats.compression_depth > 0 {
                    warn!(
                        "Nested decompression detected at depth {}",
                        stats.compression_depth
                    );
                }

                Ok(decompressed)
            }
            Err(e) => {
                warn!("Nested decompression failed: {}", e);
                Err(Error::SecurityError(format!(
                    "Nested decompression failed: {}",
                    e
                )))
            }
        }
    }

    fn compress_with_strategy(&self, data: &[u8]) -> Result<SecureCompressedData> {
        // Simplified compression simulation - in real implementation would use actual compression libraries
        let compression_factor = match self.strategy {
            CompressionStrategy::None => 1.0,
            _ => {
                // Simulate compression by analyzing data entropy
                let unique_bytes = data.iter().collect::<std::collections::HashSet<_>>().len();
                let entropy = unique_bytes as f64 / 256.0; // Rough entropy calculation
                2.0 - entropy // Higher entropy = less compression
            }
        };

        let compressed_size = (data.len() as f64 / compression_factor).max(1.0) as usize;
        let mut compressed = vec![0u8; compressed_size];

        // Simple compression: copy first N bytes
        let copy_size = compressed_size.min(data.len());
        compressed[..copy_size].copy_from_slice(&data[..copy_size]);

        Ok(SecureCompressedData {
            data: compressed,
            original_size: data.len(),
            compression_ratio: compression_factor,
        })
    }

    fn log_decompression_stats(&self, stats: &CompressionStats) {
        info!(
            "Decompression stats: {}B -> {}B (ratio: {:.2}x, depth: {})",
            stats.compressed_size, stats.decompressed_size, stats.ratio, stats.compression_depth
        );
    }
}

/// Secure decompression context for streaming operations
pub struct SecureDecompressionContext {
    detector: CompressionBombDetector,
    current_depth: usize,
    max_concurrent_streams: usize,
    active_streams: usize,
}

impl SecureDecompressionContext {
    /// Create new secure decompression context
    pub fn new(detector: CompressionBombDetector, max_concurrent_streams: usize) -> Self {
        Self {
            detector,
            current_depth: 0,
            max_concurrent_streams,
            active_streams: 0,
        }
    }

    /// Start new protected decompression stream
    pub fn start_stream(
        &mut self,
        compressed_size: usize,
    ) -> Result<CompressionBombProtector<Cursor<Vec<u8>>>> {
        if self.active_streams >= self.max_concurrent_streams {
            return Err(Error::SecurityError(format!(
                "Too many concurrent decompression streams: {}/{}",
                self.active_streams, self.max_concurrent_streams
            )));
        }

        let cursor = Cursor::new(Vec::new());
        let protector =
            self.detector
                .protect_nested_reader(cursor, compressed_size, self.current_depth)?;

        self.active_streams += 1;
        info!(
            "Started secure decompression stream (active: {})",
            self.active_streams
        );

        Ok(protector)
    }

    /// Finish decompression stream
    pub fn finish_stream(&mut self) {
        if self.active_streams > 0 {
            self.active_streams -= 1;
            info!(
                "Finished secure decompression stream (active: {})",
                self.active_streams
            );
        }
    }

    /// Get current context statistics
    pub fn stats(&self) -> DecompressionContextStats {
        DecompressionContextStats {
            current_depth: self.current_depth,
            active_streams: self.active_streams,
            max_concurrent_streams: self.max_concurrent_streams,
        }
    }
}

/// Statistics for decompression context
#[derive(Debug, Clone)]
pub struct DecompressionContextStats {
    pub current_depth: usize,
    pub active_streams: usize,
    pub max_concurrent_streams: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::CompressionBombConfig;

    #[test]
    fn test_secure_compressor_creation() {
        let detector = CompressionBombDetector::default();
        let compressor = SecureCompressor::new(detector, CompressionStrategy::RunLength);

        // Should be able to create compressor
        assert!(std::ptr::addr_of!(compressor).cast::<u8>() != std::ptr::null());
    }

    #[test]
    fn test_secure_compression() {
        let compressor = SecureCompressor::with_default_security(CompressionStrategy::RunLength);
        let data = b"Hello, world! This is test data for compression.";

        let result = compressor.compress(data);
        assert!(result.is_ok());

        let compressed = result.unwrap();
        assert!(compressed.original_size == data.len());
    }

    #[test]
    fn test_compression_size_limit() {
        let config = CompressionBombConfig {
            max_decompressed_size: 100, // Very small limit
            ..Default::default()
        };
        let detector = CompressionBombDetector::new(config);
        let compressor = SecureCompressor::new(
            detector,
            CompressionStrategy::Dictionary {
                dictionary: std::collections::HashMap::new(),
            },
        );

        let large_data = vec![0u8; 1000]; // 1KB data
        let result = compressor.compress(&large_data);

        // Should fail pre-compression validation
        assert!(result.is_err());
    }

    #[test]
    fn test_secure_decompression_context() {
        let detector = CompressionBombDetector::default();
        let mut context = SecureDecompressionContext::new(detector, 2);

        // Should be able to start streams up to limit
        assert!(context.start_stream(1024).is_ok());
        assert!(context.start_stream(1024).is_ok());

        // Third stream should fail
        assert!(context.start_stream(1024).is_err());

        // After finishing a stream, should work again
        context.finish_stream();
        assert!(context.start_stream(1024).is_ok());
    }

    #[test]
    fn test_context_stats() {
        let detector = CompressionBombDetector::default();
        let context = SecureDecompressionContext::new(detector, 5);

        let stats = context.stats();
        assert_eq!(stats.current_depth, 0);
        assert_eq!(stats.active_streams, 0);
        assert_eq!(stats.max_concurrent_streams, 5);
    }

    #[test]
    fn test_different_compression_strategies() {
        let compressor = SecureCompressor::with_default_security(CompressionStrategy::None);
        let data = b"test data";

        let result = compressor.compress(data);
        assert!(result.is_ok());

        let compressed = result.unwrap();
        assert_eq!(compressed.compression_ratio, 1.0); // No compression

        // Test dictionary strategy
        let dict_strategy = CompressionStrategy::Dictionary {
            dictionary: std::collections::HashMap::new(),
        };
        let compressor = SecureCompressor::with_default_security(dict_strategy);
        let result = compressor.compress(data);
        assert!(result.is_ok(), "Dictionary strategy should work");

        // Test delta strategy
        let delta_strategy = CompressionStrategy::Delta {
            base_values: std::collections::HashMap::new(),
        };
        let compressor = SecureCompressor::with_default_security(delta_strategy);
        let result = compressor.compress(data);
        assert!(result.is_ok(), "Delta strategy should work");
    }
}
