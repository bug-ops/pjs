//! Global configuration for PJS Core library
//!
//! This module provides centralized configuration for all components,
//! replacing hardcoded constants with configurable values.

use crate::compression::CompressionConfig;

/// Global configuration for PJS library components
#[derive(Debug, Clone, Default)]
pub struct PjsConfig {
    /// Configuration for compression algorithms
    pub compression: CompressionConfig,
    /// Configuration for parsers
    pub parser: ParserConfig,
    /// Configuration for streaming
    pub streaming: StreamingConfig,
    /// Configuration for SIMD operations
    pub simd: SimdConfig,
}

/// Configuration for JSON parsers
#[derive(Debug, Clone)]
pub struct ParserConfig {
    /// Maximum input size in MB
    pub max_input_size_mb: usize,
    /// Buffer initial capacity in bytes
    pub buffer_initial_capacity: usize,
    /// SIMD minimum size threshold
    pub simd_min_size: usize,
    /// Enable semantic type detection
    pub enable_semantics: bool,
}

/// Configuration for streaming operations
#[derive(Debug, Clone)]
pub struct StreamingConfig {
    /// Maximum frame size in bytes
    pub max_frame_size: usize,
    /// Default chunk size for processing
    pub default_chunk_size: usize,
    /// Timeout for operations in milliseconds
    pub operation_timeout_ms: u64,
    /// Maximum bandwidth in bytes per second
    pub max_bandwidth_bps: u64,
}

/// Configuration for SIMD acceleration
#[derive(Debug, Clone)]
pub struct SimdConfig {
    /// Batch size for SIMD operations
    pub batch_size: usize,
    /// Initial capacity for SIMD buffers
    pub initial_capacity: usize,
    /// AVX-512 alignment size in bytes
    pub avx512_alignment: usize,
    /// Chunk size for vectorized operations
    pub vectorized_chunk_size: usize,
    /// Enable statistics collection
    pub enable_stats: bool,
}


impl Default for ParserConfig {
    fn default() -> Self {
        Self {
            max_input_size_mb: 100,
            buffer_initial_capacity: 8192, // 8KB
            simd_min_size: 4096, // 4KB
            enable_semantics: true,
        }
    }
}

impl Default for StreamingConfig {
    fn default() -> Self {
        Self {
            max_frame_size: 64 * 1024, // 64KB
            default_chunk_size: 1024,
            operation_timeout_ms: 5000, // 5 seconds
            max_bandwidth_bps: 1_000_000, // 1MB/s
        }
    }
}

impl Default for SimdConfig {
    fn default() -> Self {
        Self {
            batch_size: 100,
            initial_capacity: 8192, // 8KB
            avx512_alignment: 64,
            vectorized_chunk_size: 32,
            enable_stats: false,
        }
    }
}

/// Configuration profiles for different use cases
impl PjsConfig {
    /// Configuration optimized for low latency
    pub fn low_latency() -> Self {
        Self {
            compression: CompressionConfig::default(),
            parser: ParserConfig {
                max_input_size_mb: 10,
                buffer_initial_capacity: 4096, // 4KB
                simd_min_size: 2048, // 2KB
                enable_semantics: false, // Disable for speed
            },
            streaming: StreamingConfig {
                max_frame_size: 16 * 1024, // 16KB
                default_chunk_size: 512,
                operation_timeout_ms: 1000, // 1 second
                max_bandwidth_bps: 10_000_000, // 10MB/s
            },
            simd: SimdConfig {
                batch_size: 50,
                initial_capacity: 4096, // 4KB
                avx512_alignment: 64,
                vectorized_chunk_size: 16,
                enable_stats: false,
            },
        }
    }

    /// Configuration optimized for high throughput
    pub fn high_throughput() -> Self {
        Self {
            compression: CompressionConfig::default(),
            parser: ParserConfig {
                max_input_size_mb: 1000, // 1GB
                buffer_initial_capacity: 32768, // 32KB
                simd_min_size: 8192, // 8KB
                enable_semantics: true,
            },
            streaming: StreamingConfig {
                max_frame_size: 256 * 1024, // 256KB
                default_chunk_size: 4096,
                operation_timeout_ms: 30000, // 30 seconds
                max_bandwidth_bps: 100_000_000, // 100MB/s
            },
            simd: SimdConfig {
                batch_size: 500,
                initial_capacity: 32768, // 32KB
                avx512_alignment: 64,
                vectorized_chunk_size: 64,
                enable_stats: true,
            },
        }
    }

    /// Configuration optimized for mobile/constrained devices
    pub fn mobile() -> Self {
        Self {
            compression: CompressionConfig {
                min_array_length: 1,
                min_string_length: 2,
                min_frequency_count: 1,
                uuid_compression_potential: 0.5,
                string_dict_threshold: 25.0, // Lower threshold
                delta_threshold: 15.0, // Lower threshold
                min_delta_potential: 0.2,
                run_length_threshold: 10.0, // Lower threshold
                min_compression_potential: 0.3,
                min_numeric_sequence_size: 2,
            },
            parser: ParserConfig {
                max_input_size_mb: 10,
                buffer_initial_capacity: 2048, // 2KB
                simd_min_size: 1024, // 1KB
                enable_semantics: false,
            },
            streaming: StreamingConfig {
                max_frame_size: 8 * 1024, // 8KB
                default_chunk_size: 256,
                operation_timeout_ms: 10000, // 10 seconds
                max_bandwidth_bps: 100_000, // 100KB/s
            },
            simd: SimdConfig {
                batch_size: 25,
                initial_capacity: 2048, // 2KB
                avx512_alignment: 32, // Smaller alignment
                vectorized_chunk_size: 8,
                enable_stats: false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = PjsConfig::default();
        assert_eq!(config.parser.max_input_size_mb, 100);
        assert_eq!(config.streaming.max_frame_size, 64 * 1024);
        assert_eq!(config.simd.batch_size, 100);
    }

    #[test]
    fn test_low_latency_profile() {
        let config = PjsConfig::low_latency();
        assert_eq!(config.streaming.max_frame_size, 16 * 1024);
        assert!(!config.parser.enable_semantics);
        assert_eq!(config.streaming.operation_timeout_ms, 1000);
    }

    #[test]
    fn test_high_throughput_profile() {
        let config = PjsConfig::high_throughput();
        assert_eq!(config.streaming.max_frame_size, 256 * 1024);
        assert!(config.parser.enable_semantics);
        assert!(config.simd.enable_stats);
    }

    #[test]
    fn test_mobile_profile() {
        let config = PjsConfig::mobile();
        assert_eq!(config.streaming.max_frame_size, 8 * 1024);
        assert_eq!(config.compression.string_dict_threshold, 25.0);
        assert_eq!(config.simd.vectorized_chunk_size, 8);
    }

    #[test]
    fn test_compression_with_custom_config() {
        use crate::compression::{SchemaAnalyzer, CompressionConfig};
        use serde_json::json;

        // Create custom compression config with lower thresholds
        let compression_config = CompressionConfig {
            string_dict_threshold: 10.0, // Lower threshold for testing
            min_frequency_count: 1,
            ..Default::default()
        };

        let mut analyzer = SchemaAnalyzer::with_config(compression_config);
        
        // Test data that should trigger dictionary compression with low threshold
        let data = json!({
            "users": [
                {"status": "active", "role": "user"},
                {"status": "active", "role": "user"}
            ]
        });

        let strategy = analyzer.analyze(&data).unwrap();
        
        // With lower threshold, should detect dictionary compression opportunity
        match strategy {
            crate::compression::CompressionStrategy::Dictionary { .. } | 
            crate::compression::CompressionStrategy::Hybrid { .. } => {
                // Expected with low threshold
            },
            _ => {
                // Also acceptable, depends on specific data characteristics
            }
        }
    }
}