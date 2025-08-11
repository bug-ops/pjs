//! High-performance JSON parsing module with hybrid approach
//!
//! This module provides both SIMD-optimized parsing and serde fallback,
//! allowing rapid MVP development while building towards maximum performance.

pub mod buffer_pool;
pub mod scanner;
pub mod simd;
pub mod simd_zero_copy;
pub mod simple;
pub mod sonic;
pub mod value;
pub mod zero_copy;

pub use buffer_pool::{BufferPool, PooledBuffer, BufferSize, PoolConfig, global_buffer_pool};
pub use scanner::{JsonScanner, ScanResult, StringLocation};
pub use simd_zero_copy::{SimdZeroCopyParser, SimdZeroCopyConfig, SimdParseResult, SimdParsingStats};
pub use simple::{ParseConfig, ParseStats, SimpleParser};
pub use sonic::{LazyFrame, SonicConfig, SonicParser};
pub use value::{JsonValue, LazyArray, LazyObject};
pub use zero_copy::{LazyParser, ZeroCopyParser, LazyJsonValue, MemoryUsage, IncrementalParser};

use crate::{Result, SemanticMeta};

/// High-performance hybrid parser with SIMD acceleration
pub struct Parser {
    sonic: SonicParser,
    simple: SimpleParser,
    zero_copy_simd: Option<SimdZeroCopyParser<'static>>,
    use_sonic: bool,
    use_zero_copy: bool,
}

impl Parser {
    /// Create new parser with default configuration (sonic-rs enabled)
    pub fn new() -> Self {
        Self {
            sonic: SonicParser::new(),
            simple: SimpleParser::new(),
            zero_copy_simd: None, // Lazy initialization
            use_sonic: true,
            use_zero_copy: true, // Enable zero-copy by default
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
            zero_copy_simd: None, // Lazy initialization
            use_sonic: true,
            use_zero_copy: true,
        }
    }

    /// Create parser with serde fallback (for compatibility)
    pub fn with_serde_fallback() -> Self {
        Self {
            sonic: SonicParser::new(),
            simple: SimpleParser::new(),
            zero_copy_simd: None,
            use_sonic: false,
            use_zero_copy: false, // Disable zero-copy for compatibility
        }
    }

    /// Create parser optimized for zero-copy performance
    pub fn zero_copy_optimized() -> Self {
        Self {
            sonic: SonicParser::new(),
            simple: SimpleParser::new(),
            zero_copy_simd: None, // Will be initialized on first use
            use_sonic: false, // Use zero-copy instead
            use_zero_copy: true,
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

    /// Get parser statistics (simplified for now)
    pub fn stats(&self) -> ParseStats {
        if self.use_sonic {
            // TODO: Implement stats collection for sonic parser
            ParseStats::default()
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
