//! High-performance JSON parsing module with hybrid approach
//!
//! This module provides both SIMD-optimized parsing and serde fallback,
//! allowing rapid MVP development while building towards maximum performance.

#[cfg(feature = "partial-parse")]
pub mod partial;

#[cfg(feature = "partial-parse")]
pub use partial::{
    JiterConfig, JiterPartialParser, ParseDiagnostic, PartialJsonParser, PartialParseResult,
    StreamingHint,
};

pub mod aligned_alloc;
pub mod buffer_pool;
pub mod scanner;
pub mod simd;
pub mod simple;
pub mod sonic;
pub mod value;
pub mod zero_copy;

pub use aligned_alloc::{AlignedAllocator, aligned_allocator};
pub use buffer_pool::{
    BufferPool, BufferSize, PoolConfig, PooledBuffer, SimdType, global_buffer_pool,
};
pub use scanner::{JsonScanner, ScanResult, StringLocation};
pub use simple::{ParseConfig, ParseStats, SimpleParser};
pub use sonic::{SonicConfig, SonicParser};
pub use value::{JsonValue, LazyArray, LazyObject};
pub use zero_copy::{IncrementalParser, LazyJsonValue, LazyParser, MemoryUsage, ZeroCopyParser};

use crate::{Result, SemanticMeta};

/// High-performance hybrid parser with SIMD acceleration
pub struct Parser {
    sonic: SonicParser,
    simple: SimpleParser,
    use_sonic: bool,
}

impl Parser {
    /// Create new parser with default configuration.
    ///
    /// Selects the sonic-rs SIMD backend when any `simd-*` Cargo feature is
    /// enabled (which is the default via `simd-auto`). Without any `simd-*`
    /// feature, falls back to the portable serde-based parser.
    pub fn new() -> Self {
        Self {
            sonic: SonicParser::new(),
            simple: SimpleParser::new(),
            use_sonic: cfg!(pjs_simd),
        }
    }

    /// Create parser with custom configuration
    pub fn with_config(config: ParseConfig) -> Self {
        let sonic_config = SonicConfig {
            detect_semantics: config.detect_semantics,
            max_input_size: config.max_size_mb * 1024 * 1024,
        };

        Self {
            sonic: SonicParser::with_config(sonic_config),
            simple: SimpleParser::with_config(config),
            use_sonic: cfg!(pjs_simd),
        }
    }

    /// Create parser with serde fallback (for compatibility)
    pub fn with_serde_fallback() -> Self {
        Self {
            sonic: SonicParser::new(),
            simple: SimpleParser::new(),
            use_sonic: false,
        }
    }

    /// Create parser optimized for zero-copy performance
    pub fn zero_copy_optimized() -> Self {
        Self {
            sonic: SonicParser::new(),
            simple: SimpleParser::new(),
            use_sonic: false,
        }
    }

    /// Parse JSON bytes into PJS Frame using optimal strategy
    pub fn parse(&self, input: &[u8]) -> Result<crate::Frame> {
        if self.use_sonic {
            // Try sonic-rs first for performance
            match self.sonic.parse(input) {
                Ok(frame) => Ok(frame),
                Err(_) => {
                    // Fallback to serde for compatibility
                    self.simple.parse(input)
                }
            }
        } else {
            self.simple.parse(input)
        }
    }

    /// Parse with explicit semantic hints
    pub fn parse_with_semantics(
        &self,
        input: &[u8],
        semantics: &SemanticMeta,
    ) -> Result<crate::Frame> {
        if self.use_sonic {
            // Sonic parser doesn't support explicit semantics yet
            // Use simple parser for this case
            self.simple.parse_with_semantics(input, semantics)
        } else {
            self.simple.parse_with_semantics(input, semantics)
        }
    }

    /// Parse the largest valid JSON prefix from `input`, tolerating truncation.
    ///
    /// Delegates to [`JiterPartialParser`] with default configuration.
    ///
    /// Returns `Ok(None)` when `consumed == 0` (no structurally complete prefix
    /// could be recovered — e.g. input `[` or `-`). Returns `Ok(Some(_))` when
    /// at least one byte was committed.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::InvalidJson`] for syntactically invalid
    /// input (e.g. stray `}`). Returns [`crate::error::Error::Buffer`] when the
    /// input exceeds the default `max_input_size` (100 MiB).
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use pjson_rs::parser::Parser;
    ///
    /// let parser = Parser::new();
    /// let result = parser.parse_partial(b"{\"a\":1,\"b\":[2,3").unwrap();
    /// assert!(result.is_some());
    /// ```
    #[cfg(feature = "partial-parse")]
    pub fn parse_partial(&self, input: &[u8]) -> crate::Result<Option<PartialParseResult>> {
        use partial::PartialJsonParser as _;
        let result = JiterPartialParser::default().parse_partial(input)?;
        if result.consumed == 0 {
            Ok(None)
        } else {
            Ok(Some(result))
        }
    }

    /// Get parser statistics
    pub fn stats(&self) -> ParseStats {
        if self.use_sonic {
            let sonic_stats = self.sonic.get_stats();
            ParseStats {
                total_parses: sonic_stats.total_parses,
                semantic_detections: sonic_stats.sonic_successes,
                avg_parse_time_ms: sonic_stats.avg_parse_time_ns as f64 / 1_000_000.0,
            }
        } else {
            self.simple.stats()
        }
    }
}

impl Default for Parser {
    fn default() -> Self {
        Self::new()
    }
}

/// JSON value types for initial classification
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ValueType {
    Object,
    Array,
    String,
    Number,
    Boolean,
    Null,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parser_creation() {
        let parser = Parser::new();
        assert_eq!(parser.stats().total_parses, 0);
    }

    #[test]
    fn test_simple_parsing() {
        let parser = Parser::new();
        let input = br#"{"hello": "world"}"#;
        let result = parser.parse(input);
        assert!(result.is_ok());

        let frame = result.unwrap();
        // Simple JSON may not have semantic metadata
        assert_eq!(frame.payload.len(), input.len());
    }

    #[test]
    fn test_numeric_array_parsing() {
        let parser = Parser::new();
        let input = b"[1.0, 2.0, 3.0, 4.0]";
        let result = parser.parse(input);
        assert!(result.is_ok());
    }

    #[test]
    fn test_semantic_parsing() {
        let parser = Parser::new();
        let input = b"[1, 2, 3, 4]";

        let semantics = crate::SemanticMeta::new(crate::semantic::SemanticType::NumericArray {
            dtype: crate::semantic::NumericDType::I32,
            length: Some(4),
        });

        let result = parser.parse_with_semantics(input, &semantics);
        assert!(result.is_ok());
    }

    #[test]
    fn test_custom_config() {
        let config = ParseConfig {
            detect_semantics: false,
            max_size_mb: 50,
            stream_large_arrays: false,
            stream_threshold: 500,
        };

        let parser = Parser::with_config(config);
        let input = br#"{"test": "data"}"#;
        let result = parser.parse(input);
        assert!(result.is_ok());
    }
}
