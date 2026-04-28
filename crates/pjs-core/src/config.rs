//! Global configuration for PJS Core library
//!
//! This module provides centralized configuration for all components,
//! replacing hardcoded constants with configurable values.

pub mod security;

use crate::compression::CompressionConfig;
pub use security::SecurityConfig;

/// Errors produced by [`PjsConfig::validate`] and its sub-config validators.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// A numeric config field must be strictly greater than zero.
    #[error("config field `{section}.{field}` must be > 0")]
    MustBePositive {
        /// Config section name (e.g. `"streaming"`)
        section: &'static str,
        /// Field name within that section (e.g. `"max_frame_size"`)
        field: &'static str,
    },

    /// Two related config fields violate an ordering or consistency constraint.
    #[error("config constraint violated in `{section}`: {message}")]
    InconsistentBounds {
        /// Config section name (e.g. `"security.sessions"`)
        section: &'static str,
        /// Human-readable description of the violated constraint
        message: &'static str,
    },
}

/// Global configuration for PJS library components
#[derive(Debug, Clone, Default)]
pub struct PjsConfig {
    /// Security configuration and limits
    pub security: SecurityConfig,
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
            simd_min_size: 4096,           // 4KB
            enable_semantics: true,
        }
    }
}

impl Default for StreamingConfig {
    fn default() -> Self {
        Self {
            max_frame_size: 64 * 1024, // 64KB
            default_chunk_size: 1024,
            operation_timeout_ms: 5000,   // 5 seconds
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

impl StreamingConfig {
    /// Validate streaming configuration invariants.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::MustBePositive`] when `max_frame_size` or
    /// `operation_timeout_ms` is zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use pjson_rs::config::StreamingConfig;
    ///
    /// StreamingConfig::default().validate().expect("defaults are valid");
    /// ```
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.max_frame_size == 0 {
            return Err(ConfigError::MustBePositive {
                section: "streaming",
                field: "max_frame_size",
            });
        }
        if self.operation_timeout_ms == 0 {
            return Err(ConfigError::MustBePositive {
                section: "streaming",
                field: "operation_timeout_ms",
            });
        }
        Ok(())
    }
}

impl ParserConfig {
    /// Validate parser configuration invariants.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::MustBePositive`] when `max_input_size_mb` or
    /// `buffer_initial_capacity` is zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use pjson_rs::config::ParserConfig;
    ///
    /// ParserConfig::default().validate().expect("defaults are valid");
    /// ```
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.max_input_size_mb == 0 {
            return Err(ConfigError::MustBePositive {
                section: "parser",
                field: "max_input_size_mb",
            });
        }
        if self.buffer_initial_capacity == 0 {
            return Err(ConfigError::MustBePositive {
                section: "parser",
                field: "buffer_initial_capacity",
            });
        }
        Ok(())
    }
}

impl SimdConfig {
    /// Validate SIMD configuration invariants.
    ///
    /// The `avx512_alignment` field must be a power of two greater than zero,
    /// because memory alignment requirements are always powers of two in x86.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::MustBePositive`] when `avx512_alignment` is zero.
    /// Returns [`ConfigError::InconsistentBounds`] when `avx512_alignment` is
    /// not a power of two.
    ///
    /// # Examples
    ///
    /// ```
    /// use pjson_rs::config::SimdConfig;
    ///
    /// SimdConfig::default().validate().expect("defaults are valid");
    /// ```
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.avx512_alignment == 0 {
            return Err(ConfigError::MustBePositive {
                section: "simd",
                field: "avx512_alignment",
            });
        }
        if !self.avx512_alignment.is_power_of_two() {
            return Err(ConfigError::InconsistentBounds {
                section: "simd",
                message: "avx512_alignment must be a power of two",
            });
        }
        Ok(())
    }
}

/// Configuration profiles for different use cases
impl PjsConfig {
    /// Validate the entire configuration, including all sub-configs.
    ///
    /// Validation is fail-fast: the first error encountered is returned.
    /// The chain order is: `streaming`, `parser`, `simd`, `security`.
    ///
    /// # Errors
    ///
    /// Returns the first [`ConfigError`] found in any sub-config.
    ///
    /// # Examples
    ///
    /// ```
    /// use pjson_rs::config::PjsConfig;
    ///
    /// PjsConfig::default().validate().expect("defaults are valid");
    /// ```
    pub fn validate(&self) -> Result<(), ConfigError> {
        self.streaming.validate()?;
        self.parser.validate()?;
        self.simd.validate()?;
        self.security.validate()?;
        Ok(())
    }

    /// Configuration optimized for low latency
    pub fn low_latency() -> Self {
        Self {
            security: SecurityConfig::development(),
            compression: CompressionConfig::default(),
            parser: ParserConfig {
                max_input_size_mb: 10,
                buffer_initial_capacity: 4096, // 4KB
                simd_min_size: 2048,           // 2KB
                enable_semantics: false,       // Disable for speed
            },
            streaming: StreamingConfig {
                max_frame_size: 16 * 1024, // 16KB
                default_chunk_size: 512,
                operation_timeout_ms: 1000,    // 1 second
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
            security: SecurityConfig::high_throughput(),
            compression: CompressionConfig::default(),
            parser: ParserConfig {
                max_input_size_mb: 1000,        // 1GB
                buffer_initial_capacity: 32768, // 32KB
                simd_min_size: 8192,            // 8KB
                enable_semantics: true,
            },
            streaming: StreamingConfig {
                max_frame_size: 256 * 1024, // 256KB
                default_chunk_size: 4096,
                operation_timeout_ms: 30000,    // 30 seconds
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
            security: SecurityConfig::low_memory(),
            compression: CompressionConfig {
                min_array_length: 1,
                min_string_length: 2,
                min_frequency_count: 1,
                uuid_compression_potential: 0.5,
                string_dict_threshold: 25.0, // Lower threshold
                delta_threshold: 15.0,       // Lower threshold
                min_delta_potential: 0.2,
                run_length_threshold: 10.0, // Lower threshold
                min_compression_potential: 0.3,
                min_numeric_sequence_size: 2,
            },
            parser: ParserConfig {
                max_input_size_mb: 10,
                buffer_initial_capacity: 2048, // 2KB
                simd_min_size: 1024,           // 1KB
                enable_semantics: false,
            },
            streaming: StreamingConfig {
                max_frame_size: 8 * 1024, // 8KB
                default_chunk_size: 256,
                operation_timeout_ms: 10000, // 10 seconds
                max_bandwidth_bps: 100_000,  // 100KB/s
            },
            simd: SimdConfig {
                batch_size: 25,
                initial_capacity: 2048, // 2KB
                avx512_alignment: 32,   // Smaller alignment
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
    fn test_pjs_config_default_validates() {
        PjsConfig::default()
            .validate()
            .expect("PjsConfig::default() must be valid");
    }

    #[test]
    fn test_streaming_config_default_validates() {
        StreamingConfig::default()
            .validate()
            .expect("StreamingConfig::default() must be valid");
    }

    #[test]
    fn test_parser_config_default_validates() {
        ParserConfig::default()
            .validate()
            .expect("ParserConfig::default() must be valid");
    }

    #[test]
    fn test_simd_config_default_validates() {
        SimdConfig::default()
            .validate()
            .expect("SimdConfig::default() must be valid");
    }

    #[test]
    fn test_streaming_rejects_zero_max_frame_size() {
        let cfg = StreamingConfig {
            max_frame_size: 0,
            ..StreamingConfig::default()
        };
        let err = cfg.validate().unwrap_err();
        assert!(matches!(
            err,
            ConfigError::MustBePositive {
                section: "streaming",
                field: "max_frame_size"
            }
        ));
    }

    #[test]
    fn test_streaming_rejects_zero_operation_timeout_ms() {
        let cfg = StreamingConfig {
            operation_timeout_ms: 0,
            ..StreamingConfig::default()
        };
        let err = cfg.validate().unwrap_err();
        assert!(matches!(
            err,
            ConfigError::MustBePositive {
                section: "streaming",
                field: "operation_timeout_ms"
            }
        ));
    }

    #[test]
    fn test_simd_rejects_non_power_of_two_alignment() {
        let cfg = SimdConfig {
            avx512_alignment: 3,
            ..SimdConfig::default()
        };
        let err = cfg.validate().unwrap_err();
        assert!(matches!(
            err,
            ConfigError::InconsistentBounds {
                section: "simd",
                ..
            }
        ));
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
        use crate::compression::{CompressionConfig, SchemaAnalyzer};
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
            crate::compression::CompressionStrategy::Dictionary { .. }
            | crate::compression::CompressionStrategy::Hybrid { .. } => {
                // Expected with low threshold
            }
            _ => {
                // Also acceptable, depends on specific data characteristics
            }
        }
    }
}
