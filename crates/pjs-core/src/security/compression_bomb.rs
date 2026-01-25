//! Compression bomb protection to prevent memory exhaustion attacks

use crate::{Error, Result};
use std::io::Read;
use thiserror::Error;

/// Errors related to compression bomb detection
#[derive(Error, Debug, Clone)]
pub enum CompressionBombError {
    #[error("Compression ratio exceeded: {ratio:.2}x > {max_ratio:.2}x")]
    RatioExceeded { ratio: f64, max_ratio: f64 },

    #[error("Decompressed size exceeded: {size} bytes > {max_size} bytes")]
    SizeExceeded { size: usize, max_size: usize },

    #[error("Compression depth exceeded: {depth} > {max_depth}")]
    DepthExceeded { depth: usize, max_depth: usize },
}

/// Configuration for compression bomb protection
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CompressionBombConfig {
    /// Maximum allowed compression ratio (decompressed_size / compressed_size)
    pub max_ratio: f64,
    /// Maximum allowed decompressed size in bytes
    pub max_decompressed_size: usize,
    /// Maximum nested compression levels
    pub max_compression_depth: usize,
    /// Check interval - how often to check during decompression
    pub check_interval_bytes: usize,
}

impl Default for CompressionBombConfig {
    fn default() -> Self {
        Self {
            max_ratio: 100.0,                         // 100x compression ratio limit
            max_decompressed_size: 100 * 1024 * 1024, // 100MB
            max_compression_depth: 3,
            check_interval_bytes: 64 * 1024, // Check every 64KB
        }
    }
}

impl CompressionBombConfig {
    /// Configuration for high-security environments
    pub fn high_security() -> Self {
        Self {
            max_ratio: 20.0,
            max_decompressed_size: 10 * 1024 * 1024, // 10MB
            max_compression_depth: 2,
            check_interval_bytes: 32 * 1024, // Check every 32KB
        }
    }

    /// Configuration for low-memory environments
    pub fn low_memory() -> Self {
        Self {
            max_ratio: 50.0,
            max_decompressed_size: 5 * 1024 * 1024, // 5MB
            max_compression_depth: 2,
            check_interval_bytes: 16 * 1024, // Check every 16KB
        }
    }

    /// Configuration for high-throughput environments
    pub fn high_throughput() -> Self {
        Self {
            max_ratio: 200.0,
            max_decompressed_size: 500 * 1024 * 1024, // 500MB
            max_compression_depth: 5,
            check_interval_bytes: 128 * 1024, // Check every 128KB
        }
    }
}

/// Protected reader that monitors decompression ratios and sizes
#[derive(Debug)]
pub struct CompressionBombProtector<R: Read> {
    inner: R,
    config: CompressionBombConfig,
    compressed_size: usize,
    decompressed_size: usize,
    bytes_since_check: usize,
    compression_depth: usize,
}

impl<R: Read> CompressionBombProtector<R> {
    /// Create new protector with given reader and configuration
    pub fn new(inner: R, config: CompressionBombConfig, compressed_size: usize) -> Self {
        Self {
            inner,
            config,
            compressed_size,
            decompressed_size: 0,
            bytes_since_check: 0,
            compression_depth: 0,
        }
    }

    /// Create new protector with nested compression tracking
    pub fn with_depth(
        inner: R,
        config: CompressionBombConfig,
        compressed_size: usize,
        depth: usize,
    ) -> Result<Self> {
        if depth > config.max_compression_depth {
            return Err(Error::SecurityError(
                CompressionBombError::DepthExceeded {
                    depth,
                    max_depth: config.max_compression_depth,
                }
                .to_string(),
            ));
        }

        Ok(Self {
            inner,
            config,
            compressed_size,
            decompressed_size: 0,
            bytes_since_check: 0,
            compression_depth: depth,
        })
    }

    /// Check current compression ratio and size limits
    fn check_limits(&self) -> Result<()> {
        // Check decompressed size limit
        if self.decompressed_size > self.config.max_decompressed_size {
            return Err(Error::SecurityError(
                CompressionBombError::SizeExceeded {
                    size: self.decompressed_size,
                    max_size: self.config.max_decompressed_size,
                }
                .to_string(),
            ));
        }

        // Check compression ratio (avoid division by zero)
        if self.compressed_size > 0 && self.decompressed_size > 0 {
            let ratio = self.decompressed_size as f64 / self.compressed_size as f64;
            if ratio > self.config.max_ratio {
                return Err(Error::SecurityError(
                    CompressionBombError::RatioExceeded {
                        ratio,
                        max_ratio: self.config.max_ratio,
                    }
                    .to_string(),
                ));
            }
        }

        Ok(())
    }

    /// Get current compression statistics
    pub fn stats(&self) -> CompressionStats {
        let ratio = if self.compressed_size > 0 {
            self.decompressed_size as f64 / self.compressed_size as f64
        } else {
            0.0
        };

        CompressionStats {
            compressed_size: self.compressed_size,
            decompressed_size: self.decompressed_size,
            ratio,
            compression_depth: self.compression_depth,
        }
    }

    /// Get inner reader
    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: Read> Read for CompressionBombProtector<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let bytes_read = self.inner.read(buf)?;

        self.decompressed_size += bytes_read;
        self.bytes_since_check += bytes_read;

        // Check limits periodically
        if self.bytes_since_check >= self.config.check_interval_bytes {
            if let Err(e) = self.check_limits() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    e.to_string(),
                ));
            }
            self.bytes_since_check = 0;
        }

        Ok(bytes_read)
    }
}

/// Compression statistics for monitoring
#[derive(Debug, Clone)]
pub struct CompressionStats {
    pub compressed_size: usize,
    pub decompressed_size: usize,
    pub ratio: f64,
    pub compression_depth: usize,
}

/// High-level compression bomb detector
pub struct CompressionBombDetector {
    config: CompressionBombConfig,
}

impl Default for CompressionBombDetector {
    fn default() -> Self {
        Self::new(CompressionBombConfig::default())
    }
}

impl CompressionBombDetector {
    /// Create new detector with configuration
    pub fn new(config: CompressionBombConfig) -> Self {
        Self { config }
    }

    /// Validate compressed data before decompression
    pub fn validate_pre_decompression(&self, compressed_size: usize) -> Result<()> {
        if compressed_size > self.config.max_decompressed_size {
            return Err(Error::SecurityError(format!(
                "Compressed data size {} exceeds maximum allowed {}",
                compressed_size, self.config.max_decompressed_size
            )));
        }
        Ok(())
    }

    /// Create protected reader for safe decompression
    pub fn protect_reader<R: Read>(
        &self,
        reader: R,
        compressed_size: usize,
    ) -> CompressionBombProtector<R> {
        CompressionBombProtector::new(reader, self.config.clone(), compressed_size)
    }

    /// Create protected reader with compression depth tracking
    pub fn protect_nested_reader<R: Read>(
        &self,
        reader: R,
        compressed_size: usize,
        depth: usize,
    ) -> Result<CompressionBombProtector<R>> {
        CompressionBombProtector::with_depth(reader, self.config.clone(), compressed_size, depth)
    }

    /// Validate decompression result after completion
    pub fn validate_result(&self, compressed_size: usize, decompressed_size: usize) -> Result<()> {
        if decompressed_size > self.config.max_decompressed_size {
            return Err(Error::SecurityError(
                CompressionBombError::SizeExceeded {
                    size: decompressed_size,
                    max_size: self.config.max_decompressed_size,
                }
                .to_string(),
            ));
        }

        if compressed_size > 0 {
            let ratio = decompressed_size as f64 / compressed_size as f64;
            if ratio > self.config.max_ratio {
                return Err(Error::SecurityError(
                    CompressionBombError::RatioExceeded {
                        ratio,
                        max_ratio: self.config.max_ratio,
                    }
                    .to_string(),
                ));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_compression_bomb_config() {
        let config = CompressionBombConfig::default();
        assert!(config.max_ratio > 0.0);
        assert!(config.max_decompressed_size > 0);

        let high_sec = CompressionBombConfig::high_security();
        assert!(high_sec.max_ratio < config.max_ratio);

        let low_mem = CompressionBombConfig::low_memory();
        assert!(low_mem.max_decompressed_size < config.max_decompressed_size);

        let high_throughput = CompressionBombConfig::high_throughput();
        assert!(high_throughput.max_decompressed_size > config.max_decompressed_size);
    }

    #[test]
    fn test_compression_bomb_detector() {
        let detector = CompressionBombDetector::default();

        // Should pass validation for reasonable sizes
        assert!(detector.validate_pre_decompression(1024).is_ok());
        assert!(detector.validate_result(1024, 10 * 1024).is_ok());
    }

    #[test]
    fn test_size_limit_exceeded() {
        let config = CompressionBombConfig {
            max_decompressed_size: 1024,
            ..Default::default()
        };
        let detector = CompressionBombDetector::new(config);

        // Should fail for size exceeding limit
        let result = detector.validate_result(100, 2048);
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("Size exceeded") || error_msg.contains("Security error"));
    }

    #[test]
    fn test_ratio_limit_exceeded() {
        let config = CompressionBombConfig {
            max_ratio: 10.0,
            ..Default::default()
        };
        let detector = CompressionBombDetector::new(config);

        // Should fail for ratio exceeding limit (100 -> 2000 = 20x ratio)
        let result = detector.validate_result(100, 2000);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Compression ratio exceeded")
        );
    }

    #[test]
    fn test_protected_reader() {
        let data = b"Hello, world! This is test data for compression testing.";
        let cursor = Cursor::new(data.as_slice());

        let config = CompressionBombConfig::default();
        let mut protector = CompressionBombProtector::new(cursor, config, data.len());

        let mut buffer = Vec::new();
        let bytes_read = protector.read_to_end(&mut buffer).unwrap();

        assert_eq!(bytes_read, data.len());
        assert_eq!(buffer.as_slice(), data);

        let stats = protector.stats();
        assert_eq!(stats.compressed_size, data.len());
        assert_eq!(stats.decompressed_size, data.len());
        assert!((stats.ratio - 1.0).abs() < 0.01); // Should be ~1.0 for identical data
    }

    #[test]
    fn test_protected_reader_size_limit() {
        let data = vec![0u8; 2048]; // 2KB of data
        let cursor = Cursor::new(data);

        let config = CompressionBombConfig {
            max_decompressed_size: 1024, // 1KB limit
            check_interval_bytes: 512,   // Check every 512 bytes
            ..Default::default()
        };

        let mut protector = CompressionBombProtector::new(cursor, config, 100); // Simulating high compression

        let mut buffer = vec![0u8; 2048];
        let result = protector.read(&mut buffer);

        // Should either succeed initially or fail on second read
        if result.is_ok() {
            // Try reading more to trigger the limit
            let result2 = protector.read(&mut buffer[512..]);
            assert!(result2.is_err());
        } else {
            // Failed immediately
            assert!(result.is_err());
        }
    }

    #[test]
    fn test_compression_depth_limit() {
        let data = b"test data";
        let cursor = Cursor::new(data.as_slice());

        let config = CompressionBombConfig {
            max_compression_depth: 2,
            ..Default::default()
        };

        // Depth 2 should succeed
        let protector = CompressionBombProtector::with_depth(cursor, config.clone(), data.len(), 2);
        assert!(protector.is_ok());

        // Depth 3 should fail
        let cursor2 = Cursor::new(data.as_slice());
        let result = CompressionBombProtector::with_depth(cursor2, config, data.len(), 3);
        assert!(result.is_err());
    }

    #[test]
    fn test_zero_compressed_size_handling() {
        let detector = CompressionBombDetector::default();

        // Zero compressed size should not cause division by zero
        assert!(detector.validate_result(0, 1024).is_ok());
    }

    #[test]
    fn test_stats_calculation() {
        let data = b"test";
        let cursor = Cursor::new(data.as_slice());

        let protector = CompressionBombProtector::new(cursor, CompressionBombConfig::default(), 2);
        let stats = protector.stats();

        assert_eq!(stats.compressed_size, 2);
        assert_eq!(stats.decompressed_size, 0); // No reads yet
        assert_eq!(stats.ratio, 0.0);
        assert_eq!(stats.compression_depth, 0);
    }

    #[test]
    fn test_stats_with_zero_compressed_size() {
        let data = b"test";
        let cursor = Cursor::new(data.as_slice());

        // Create protector with zero compressed size
        let protector = CompressionBombProtector::new(cursor, CompressionBombConfig::default(), 0);
        let stats = protector.stats();

        assert_eq!(stats.compressed_size, 0);
        assert_eq!(stats.ratio, 0.0); // Should handle division by zero
    }

    #[test]
    fn test_into_inner() {
        let data = b"test data";
        let cursor = Cursor::new(data.as_slice());
        let original_position = cursor.position();

        let protector =
            CompressionBombProtector::new(cursor, CompressionBombConfig::default(), data.len());

        // Extract inner reader
        let inner = protector.into_inner();
        assert_eq!(inner.position(), original_position);
    }

    #[test]
    fn test_protect_nested_reader_success() {
        let detector = CompressionBombDetector::new(CompressionBombConfig {
            max_compression_depth: 3,
            ..Default::default()
        });

        let data = b"nested compression test";
        let cursor = Cursor::new(data.as_slice());

        // Create nested reader at depth 1 (within limit)
        let result = detector.protect_nested_reader(cursor, data.len(), 1);
        assert!(result.is_ok());

        let protector = result.unwrap();
        let stats = protector.stats();
        assert_eq!(stats.compression_depth, 1);
    }

    #[test]
    fn test_protect_nested_reader_depth_exceeded() {
        let detector = CompressionBombDetector::new(CompressionBombConfig {
            max_compression_depth: 2,
            ..Default::default()
        });

        let data = b"nested compression test";
        let cursor = Cursor::new(data.as_slice());

        // Try to create nested reader at depth 3 (exceeds limit)
        let result = detector.protect_nested_reader(cursor, data.len(), 3);
        assert!(result.is_err());

        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("Compression depth exceeded")
                || error_msg.contains("Security error")
        );
    }

    #[test]
    fn test_validate_pre_decompression_size_exceeded() {
        let config = CompressionBombConfig {
            max_decompressed_size: 1024,
            ..Default::default()
        };
        let detector = CompressionBombDetector::new(config);

        // Try to validate compressed data larger than max decompressed size
        let result = detector.validate_pre_decompression(2048);
        assert!(result.is_err());

        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("exceeds maximum allowed"));
    }

    #[test]
    fn test_validate_pre_decompression_success() {
        let detector = CompressionBombDetector::default();

        // Reasonable size should pass
        let result = detector.validate_pre_decompression(1024);
        assert!(result.is_ok());
    }

    #[test]
    fn test_protected_reader_stats_after_read() {
        let data = b"Hello, world!";
        let cursor = Cursor::new(data.as_slice());

        let compressed_size = 5; // Simulating 5 bytes compressed to 13 bytes
        let mut protector = CompressionBombProtector::new(
            cursor,
            CompressionBombConfig::default(),
            compressed_size,
        );

        let mut buffer = Vec::new();
        protector.read_to_end(&mut buffer).unwrap();

        let stats = protector.stats();
        assert_eq!(stats.compressed_size, compressed_size);
        assert_eq!(stats.decompressed_size, data.len());

        let expected_ratio = data.len() as f64 / compressed_size as f64;
        assert!((stats.ratio - expected_ratio).abs() < 0.01);
    }

    #[test]
    fn test_compression_bomb_error_display() {
        let ratio_err = CompressionBombError::RatioExceeded {
            ratio: 150.5,
            max_ratio: 100.0,
        };
        assert!(ratio_err.to_string().contains("150.5"));
        assert!(ratio_err.to_string().contains("100.0"));

        let size_err = CompressionBombError::SizeExceeded {
            size: 2048,
            max_size: 1024,
        };
        assert!(size_err.to_string().contains("2048"));
        assert!(size_err.to_string().contains("1024"));

        let depth_err = CompressionBombError::DepthExceeded {
            depth: 5,
            max_depth: 3,
        };
        assert!(depth_err.to_string().contains("5"));
        assert!(depth_err.to_string().contains("3"));
    }

    #[test]
    fn test_detector_default() {
        let detector1 = CompressionBombDetector::default();
        let detector2 = CompressionBombDetector::new(CompressionBombConfig::default());

        // Both should have same configuration values
        assert_eq!(detector1.config.max_ratio, detector2.config.max_ratio);
        assert_eq!(
            detector1.config.max_decompressed_size,
            detector2.config.max_decompressed_size
        );
    }

    #[test]
    fn test_slow_drip_decompression_bomb() {
        // Simulate a slow-drip attack: many small expansions that sum to a large total
        let config = CompressionBombConfig {
            max_decompressed_size: 10_000,
            check_interval_bytes: 1000, // Check every 1KB
            ..Default::default()
        };

        // Create 15KB of data (exceeds 10KB limit)
        let data = vec![0u8; 15_000];
        let cursor = Cursor::new(data);

        let mut protector = CompressionBombProtector::new(cursor, config, 100);

        let mut buffer = [0u8; 1024];
        let mut total_read = 0;
        let mut detected = false;

        // Read in small chunks until bomb detected
        loop {
            match protector.read(&mut buffer) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    total_read += n;
                }
                Err(e) => {
                    // Should detect bomb before all data is read
                    // Error message can be either "Size exceeded" or generic security error
                    let err_str = e.to_string();
                    assert!(
                        err_str.contains("Size exceeded") || err_str.contains("Security"),
                        "Expected size limit error, got: {}",
                        err_str
                    );
                    detected = true;
                    break;
                }
            }
        }

        assert!(detected, "Slow-drip bomb should be detected");
        assert!(total_read < 15_000, "Should not read all data");
    }

    #[test]
    fn test_integer_overflow_protection_in_ratio() {
        let detector = CompressionBombDetector::default();

        // Try extreme values that could cause overflow
        let result = detector.validate_result(1, usize::MAX);
        assert!(result.is_err());
    }

    #[test]
    fn test_integer_overflow_protection_in_size() {
        let config = CompressionBombConfig {
            max_decompressed_size: usize::MAX - 1,
            ..Default::default()
        };
        let detector = CompressionBombDetector::new(config);

        // Should reject at MAX
        let result = detector.validate_result(100, usize::MAX);
        assert!(result.is_err());
    }

    #[test]
    fn test_boundary_max_decompressed_size() {
        let max_size = 10_000;
        let config = CompressionBombConfig {
            max_decompressed_size: max_size,
            ..Default::default()
        };
        let detector = CompressionBombDetector::new(config);

        // Exactly at limit should pass
        assert!(detector.validate_result(100, max_size).is_ok());

        // One byte over should fail
        assert!(detector.validate_result(100, max_size + 1).is_err());
    }

    #[test]
    fn test_boundary_max_ratio() {
        let max_ratio = 50.0;
        let config = CompressionBombConfig {
            max_ratio,
            ..Default::default()
        };
        let detector = CompressionBombDetector::new(config);

        let compressed = 100;
        let at_limit = (compressed as f64 * max_ratio) as usize;

        // At limit should pass
        assert!(detector.validate_result(compressed, at_limit).is_ok());

        // Just over limit should fail
        assert!(
            detector
                .validate_result(compressed, at_limit + 100)
                .is_err()
        );
    }

    #[test]
    fn test_boundary_max_compression_depth() {
        let max_depth = 5;
        let config = CompressionBombConfig {
            max_compression_depth: max_depth,
            ..Default::default()
        };

        let data = b"test";
        let cursor = Cursor::new(data.as_slice());

        // At limit should succeed
        let result =
            CompressionBombProtector::with_depth(cursor, config.clone(), data.len(), max_depth);
        assert!(result.is_ok());

        // Over limit should fail
        let cursor2 = Cursor::new(data.as_slice());
        let result2 =
            CompressionBombProtector::with_depth(cursor2, config, data.len(), max_depth + 1);
        assert!(result2.is_err());
    }

    #[test]
    fn test_nested_compression_attack_simulation() {
        // Simulate nested compression: each layer expands the data
        let detector = CompressionBombDetector::new(CompressionBombConfig {
            max_compression_depth: 2,
            max_decompressed_size: 10_000,
            ..Default::default()
        });

        // Layer 1: 100 bytes compressed
        let layer1_data = vec![0u8; 1000]; // Expands to 1KB
        let cursor1 = Cursor::new(layer1_data.clone());

        let protector1 = detector.protect_nested_reader(cursor1, 100, 1);
        assert!(protector1.is_ok());

        // Layer 2: Within limit
        let cursor2 = Cursor::new(layer1_data.clone());
        let protector2 = detector.protect_nested_reader(cursor2, 100, 2);
        assert!(protector2.is_ok());

        // Layer 3: Exceeds depth limit
        let cursor3 = Cursor::new(layer1_data);
        let protector3 = detector.protect_nested_reader(cursor3, 100, 3);
        assert!(protector3.is_err());
    }

    #[test]
    fn test_check_limits_called_at_intervals() {
        let check_interval = 100;
        let config = CompressionBombConfig {
            max_decompressed_size: 500,
            check_interval_bytes: check_interval,
            max_ratio: 10.0,
            ..Default::default()
        };

        // Create data that will exceed limits after multiple reads
        let data = vec![0u8; 600];
        let cursor = Cursor::new(data);

        let mut protector = CompressionBombProtector::new(cursor, config, 10); // High compression ratio

        let mut buffer = [0u8; 50]; // Read in small chunks
        let mut total_read = 0;
        let mut error_occurred = false;

        loop {
            match protector.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    total_read += n;
                    // Check should trigger every check_interval bytes
                    if total_read > 500 {
                        // Should have failed by now
                        break;
                    }
                }
                Err(_) => {
                    error_occurred = true;
                    break;
                }
            }
        }

        assert!(error_occurred, "Should detect bomb during periodic checks");
    }

    #[test]
    fn test_ratio_calculation_with_large_numbers() {
        let detector = CompressionBombDetector::new(CompressionBombConfig {
            max_ratio: 100.0,
            ..Default::default()
        });

        // Large numbers that are still within ratio
        let compressed = 1_000_000;
        let decompressed = 50_000_000; // 50x ratio

        assert!(detector.validate_result(compressed, decompressed).is_ok());

        // Exceeds ratio (150x)
        let decompressed_bad = 150_000_000;
        assert!(
            detector
                .validate_result(compressed, decompressed_bad)
                .is_err()
        );
    }

    #[test]
    fn test_protected_reader_multiple_small_reads() {
        // Test that protection works across many small read operations
        let data = vec![1u8; 5000];
        let cursor = Cursor::new(data);

        let config = CompressionBombConfig {
            max_decompressed_size: 10_000,
            check_interval_bytes: 1000,
            ..Default::default()
        };

        let mut protector = CompressionBombProtector::new(cursor, config, 5000);

        // Read in very small increments
        let mut buffer = [0u8; 10];
        let mut total = 0;

        while let Ok(n) = protector.read(&mut buffer) {
            if n == 0 {
                break;
            }
            total += n;
        }

        assert_eq!(total, 5000);
        let stats = protector.stats();
        assert_eq!(stats.decompressed_size, 5000);
    }

    #[test]
    fn test_error_on_exact_check_interval_boundary() {
        let check_interval = 1000;
        let config = CompressionBombConfig {
            max_decompressed_size: 1500,
            check_interval_bytes: check_interval,
            ..Default::default()
        };

        // Data that exceeds limit at exactly the check interval
        let data = vec![0u8; 2000];
        let cursor = Cursor::new(data);

        let mut protector = CompressionBombProtector::new(cursor, config, 100);

        let mut buffer = [0u8; 1000]; // Read exactly check_interval bytes
        let mut detected = false;

        loop {
            match protector.read(&mut buffer) {
                Ok(0) => break,
                Ok(_) => {}
                Err(_) => {
                    detected = true;
                    break;
                }
            }
        }

        assert!(detected);
    }

    #[test]
    fn test_config_serialization_roundtrip() {
        let config = CompressionBombConfig {
            max_ratio: 123.45,
            max_decompressed_size: 999_888,
            max_compression_depth: 7,
            check_interval_bytes: 16_384,
        };

        // Serialize to JSON
        let json = serde_json::to_string(&config).unwrap();

        // Deserialize back
        let deserialized: CompressionBombConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(config.max_ratio, deserialized.max_ratio);
        assert_eq!(
            config.max_decompressed_size,
            deserialized.max_decompressed_size
        );
        assert_eq!(
            config.max_compression_depth,
            deserialized.max_compression_depth
        );
        assert_eq!(
            config.check_interval_bytes,
            deserialized.check_interval_bytes
        );
    }

    #[test]
    fn test_all_preset_configs() {
        // Ensure all preset configurations are valid and ordered correctly
        let default_cfg = CompressionBombConfig::default();
        let high_sec = CompressionBombConfig::high_security();
        let low_mem = CompressionBombConfig::low_memory();
        let high_throughput = CompressionBombConfig::high_throughput();

        // High security should be strictest
        assert!(high_sec.max_ratio < default_cfg.max_ratio);
        assert!(high_sec.max_decompressed_size < default_cfg.max_decompressed_size);

        // Low memory should limit size
        assert!(low_mem.max_decompressed_size < default_cfg.max_decompressed_size);

        // High throughput should be most permissive
        assert!(high_throughput.max_ratio > default_cfg.max_ratio);
        assert!(high_throughput.max_decompressed_size > default_cfg.max_decompressed_size);
    }
}
