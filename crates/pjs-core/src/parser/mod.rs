//! High-performance JSON parsing module with hybrid approach
//!
//! This module provides both SIMD-optimized parsing and serde fallback,
//! allowing rapid MVP development while building towards maximum performance.

pub mod simple;
pub mod simd;
pub mod scanner;
pub mod sonic;
pub mod value;

pub use simple::{SimpleParser, ParseConfig, ParseStats};
pub use scanner::{JsonScanner, ScanResult, StringLocation};
pub use sonic::{SonicParser, SonicConfig, LazyFrame};
pub use value::{JsonValue, LazyArray, LazyObject};

use crate::{Result, SemanticMeta};

/// Main parser interface using serde as foundation
pub struct Parser {
    simple: SimpleParser,
}

impl Parser {
    /// Create new parser with default configuration
    pub fn new() -> Self {
        Self {
            simple: SimpleParser::new(),
        }
    }

    /// Create parser with custom configuration
    pub fn with_config(config: ParseConfig) -> Self {
        Self {
            simple: SimpleParser::with_config(config),
        }
    }

    /// Parse JSON bytes into PJS Frame
    pub fn parse(&self, input: &[u8]) -> Result<crate::Frame> {
        self.simple.parse(input)
    }

    /// Parse with explicit semantic hints
    pub fn parse_with_semantics(&self, input: &[u8], semantics: &SemanticMeta) -> Result<crate::Frame> {
        self.simple.parse_with_semantics(input, semantics)
    }

    /// Get parser statistics
    pub fn stats(&self) -> ParseStats {
        self.simple.stats()
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
        assert!(frame.semantics.is_some());
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
        
        let semantics = crate::SemanticMeta::new(
            crate::semantic::SemanticType::NumericArray {
                dtype: crate::semantic::NumericDType::I32,
                length: Some(4),
            }
        );
        
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