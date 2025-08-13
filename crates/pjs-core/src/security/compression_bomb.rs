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
            max_ratio: 100.0, // 100x compression ratio limit
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
    pub fn with_depth(inner: R, config: CompressionBombConfig, compressed_size: usize, depth: usize) -> Result<Self> {
        if depth > config.max_compression_depth {
            return Err(Error::SecurityError(
                CompressionBombError::DepthExceeded {
                    depth,
                    max_depth: config.max_compression_depth,
                }.to_string()
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
                }.to_string()
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
                    }.to_string()
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

impl CompressionBombDetector {
    /// Create new detector with configuration
    pub fn new(config: CompressionBombConfig) -> Self {
        Self { config }
    }
    
    /// Create detector with default configuration
    pub fn default() -> Self {
        Self::new(CompressionBombConfig::default())
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
    pub fn protect_reader<R: Read>(&self, reader: R, compressed_size: usize) -> CompressionBombProtector<R> {
        CompressionBombProtector::new(reader, self.config.clone(), compressed_size)
    }
    
    /// Create protected reader with compression depth tracking
    pub fn protect_nested_reader<R: Read>(
        &self,
        reader: R,
        compressed_size: usize,
        depth: usize
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
                }.to_string()
            ));
        }
        
        if compressed_size > 0 {
            let ratio = decompressed_size as f64 / compressed_size as f64;
            if ratio > self.config.max_ratio {
                return Err(Error::SecurityError(
                    CompressionBombError::RatioExceeded {
                        ratio,
                        max_ratio: self.config.max_ratio,
                    }.to_string()
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
        assert!(result.unwrap_err().to_string().contains("Compression ratio exceeded"));
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
            check_interval_bytes: 512, // Check every 512 bytes
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
}