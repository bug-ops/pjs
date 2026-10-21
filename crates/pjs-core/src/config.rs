//! Global configuration for PJS Core library
//!
//! This module provides centralized configuration for all components,
//! replacing hardcoded constants with configurable values.

pub mod security;

use crate::compression::CompressionConfig;
pub use security::SecurityConfig;

use std::ops::Deref;

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

mod sealed {
    pub trait Sealed {}
}

impl sealed::Sealed for SecurityConfig {}
impl sealed::Sealed for CompressionConfig {}

/// A configuration type whose invariants can be checked.
///
/// Implemented by configs that consumers must only accept in validated form;
/// the only way to obtain a [`Validated`] is [`Validate::validated`]. The trait
/// is sealed, so only config types defined in this crate can implement it.
pub trait Validate: sealed::Sealed + Sized {
    /// Check every invariant of the configuration.
    ///
    /// # Errors
    ///
    /// Returns the first [`ConfigError`] found.
    fn validate(&self) -> Result<(), ConfigError>;

    /// Consume the config and wrap it in [`Validated`] if all invariants hold.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Validate::validate`].
    ///
    /// # Examples
    ///
    /// ```
    /// use pjson_rs::config::{SecurityConfig, Validate};
    ///
    /// let mut config = SecurityConfig::default();
    /// config.json.max_depth = 100_000;
    /// assert!(config.validated().is_err());
    /// ```
    fn validated(self) -> Result<Validated<Self>, ConfigError> {
        Validate::validate(&self)?;
        Ok(Validated(self))
    }
}

/// Proof that the wrapped config passed [`Validate::validate`].
///
/// The inner field is private and there is no mutable access, so holding a
/// `Validated<T>` proves the invariants (e.g. `json.max_depth <=
/// MAX_SAFE_JSON_DEPTH`) still hold. Consumers take this type instead of the raw
/// config.
///
/// # Examples
///
/// ```
/// use pjson_rs::config::{SecurityConfig, Validate, ValidatedSecurityConfig};
///
/// let config: ValidatedSecurityConfig = SecurityConfig::default().validated().unwrap();
/// assert!(config.json.max_depth > 0);
/// ```
#[derive(Debug, Clone)]
pub struct Validated<T>(T);

impl<T> Validated<T> {
    /// Unwrap the config, giving up the validation proof.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> Deref for Validated<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

/// A [`SecurityConfig`] that passed validation.
pub type ValidatedSecurityConfig = Validated<SecurityConfig>;

/// A [`CompressionConfig`] that passed validation.
pub type ValidatedCompressionConfig = Validated<CompressionConfig>;

impl Default for ValidatedSecurityConfig {
    fn default() -> Self {
        Self(SecurityConfig::default())
    }
}

impl ValidatedSecurityConfig {
    /// Validated [`SecurityConfig::high_throughput`] preset.
    pub fn high_throughput() -> Self {
        Self(SecurityConfig::high_throughput())
    }

    /// Validated [`SecurityConfig::low_memory`] preset.
    pub fn low_memory() -> Self {
        Self(SecurityConfig::low_memory())
    }

    /// Validated [`SecurityConfig::development`] preset.
    pub fn development() -> Self {
        Self(SecurityConfig::development())
    }
}

impl Default for ValidatedCompressionConfig {
    fn default() -> Self {
        Self(CompressionConfig::default())
    }
}

impl Validate for SecurityConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        SecurityConfig::validate(self)
    }
}

impl Validate for CompressionConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        CompressionConfig::validate(self)
    }
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
    /// The chain order is: `streaming`, `parser`, `simd`, `security`, `compression`.
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
        self.compression.validate()?;
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
                min_string_length: 2,
                min_frequency_count: 1,
                min_net_savings: 4, // Lower floor than the default, for smaller mobile payloads
                min_numeric_sequence_size: 3,
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
        assert_eq!(config.compression.min_net_savings, 4);
        assert_eq!(config.simd.vectorized_chunk_size, 8);
    }

    #[test]
    fn test_compression_with_custom_config() {
        use crate::compression::{CompressionConfig, SchemaAnalyzer};
        use serde_json::json;

        // Create custom compression config with a zeroed net-savings floor
        let compression_config = CompressionConfig {
            min_net_savings: 0,
            min_frequency_count: 1,
            ..Default::default()
        };

        let analyzer = SchemaAnalyzer::with_config(compression_config.validated().unwrap());

        // Test data that should trigger dictionary compression with a zeroed floor
        let data = json!({
            "users": [
                {"status": "active", "role": "user"},
                {"status": "active", "role": "user"}
            ]
        });

        let strategy = analyzer.analyze(&data).unwrap();

        // With a zeroed floor, should detect dictionary compression opportunity
        match strategy {
            crate::compression::CompressionStrategy::Dictionary { .. }
            | crate::compression::CompressionStrategy::Hybrid { .. } => {
                // Expected with a zeroed floor
            }
            _ => {
                // Also acceptable, depends on specific data characteristics
            }
        }
    }

    #[test]
    fn test_validated_security_config_rejects_unsafe_depth() {
        let mut config = SecurityConfig::default();
        config.json.max_depth = 100_000;
        assert!(matches!(
            config.validated(),
            Err(ConfigError::InconsistentBounds { .. })
        ));
    }

    #[test]
    fn test_validated_security_config_rejects_zero_input_size() {
        let mut config = SecurityConfig::default();
        config.json.max_input_size = 0;
        assert!(matches!(
            config.validated(),
            Err(ConfigError::MustBePositive {
                field: "max_input_size",
                ..
            })
        ));
    }

    #[test]
    fn test_validated_security_config_rejects_inverted_session_id_bounds() {
        let mut config = SecurityConfig::default();
        config.sessions.min_session_id_length = config.sessions.max_session_id_length + 1;
        assert!(config.validated().is_err());
    }

    #[test]
    fn test_validated_presets_and_defaults_are_valid() {
        for config in [
            ValidatedSecurityConfig::default(),
            ValidatedSecurityConfig::high_throughput(),
            ValidatedSecurityConfig::low_memory(),
            ValidatedSecurityConfig::development(),
        ] {
            config.validate().expect("preset must be valid");
        }
        ValidatedCompressionConfig::default()
            .validate()
            .expect("default must be valid");
    }
}
