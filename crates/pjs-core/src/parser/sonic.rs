//! Hybrid parser using sonic-rs for SIMD acceleration
//!
//! This module provides a high-performance parser that combines:
//! - sonic-rs for SIMD-accelerated JSON parsing
//! - PJS semantic analysis for intelligent chunking

use crate::{
    error::{Error, Result},
    frame::Frame,
    frame::{FrameFlags, FrameHeader},
    semantic::{NumericDType, SemanticMeta, SemanticType},
};
use bytes::Bytes;
use smallvec::SmallVec;
use sonic_rs::{JsonContainerTrait, JsonNumberTrait, JsonValueTrait, Value as SonicValue};

/// Configuration for the sonic hybrid parser
#[derive(Debug, Clone)]
pub struct SonicConfig {
    /// Enable semantic type detection
    pub detect_semantics: bool,
    /// Maximum input size in bytes
    pub max_input_size: usize,
}

impl Default for SonicConfig {
    fn default() -> Self {
        Self {
            detect_semantics: true,
            max_input_size: 100 * 1024 * 1024, // 100MB
        }
    }
}

/// High-performance parser using sonic-rs with PJS semantic analysis
pub struct SonicParser {
    config: SonicConfig,
}

impl SonicParser {
    /// Create a new SonicParser with default configuration
    pub fn new() -> Self {
        Self {
            config: SonicConfig::default(),
        }
    }

    /// Create a new SonicParser with custom configuration
    pub fn with_config(config: SonicConfig) -> Self {
        Self { config }
    }

    /// Parse JSON input using sonic-rs with PJS semantics
    pub fn parse(&self, input: &[u8]) -> Result<Frame> {
        // Check input size
        if input.len() > self.config.max_input_size {
            return Err(Error::Other(format!("Input too large: {}", input.len())));
        }

        // Convert to string for sonic-rs
        let json_str = std::str::from_utf8(input).map_err(|e| Error::Utf8(e.to_string()))?;

        // Parse with sonic-rs
        let value: SonicValue =
            sonic_rs::from_str(json_str).map_err(|e| Error::invalid_json(0, e.to_string()))?;

        // Detect semantic type if enabled
        let semantic_type = if self.config.detect_semantics {
            self.detect_semantic_type_sonic(&value)
        } else {
            SemanticType::Generic
        };

        // Build frame
        let header = FrameHeader {
            version: 1,
            flags: FrameFlags::empty(),
            sequence: 0,
            length: input.len() as u32,
            schema_id: 0,
            checksum: 0, // No checksum for now
        };

        let semantics = if semantic_type != SemanticType::Generic {
            Some(SemanticMeta::new(semantic_type))
        } else {
            None
        };

        Ok(Frame {
            header,
            payload: Bytes::copy_from_slice(input),
            semantics,
        })
    }

    /// Detect semantic type using sonic-rs Value (simplified)
    fn detect_semantic_type_sonic(&self, value: &SonicValue) -> SemanticType {
        if value.is_array() {
            // Try to get as array and analyze
            if let Some(arr) = value.as_array() {
                return self.analyze_array_semantics(arr);
            }
        }

        if value.is_object() {
            // Try to get as object and analyze
            if let Some(obj) = value.as_object() {
                // Simple GeoJSON detection
                if obj.contains_key(&"type") && obj.contains_key(&"coordinates") {
                    return SemanticType::Geospatial {
                        coordinate_system: "WGS84".to_string(),
                        geometry_type: "Point".to_string(),
                    };
                }
            }
        }

        SemanticType::Generic
    }

    /// Analyze array semantics (simplified)
    fn analyze_array_semantics(&self, arr: &sonic_rs::Array) -> SemanticType {
        let len = arr.len();
        if len == 0 {
            return SemanticType::Generic;
        }

        // Check for numeric array
        if len > 2 {
            let mut all_numeric = true;
            let mut dtype = NumericDType::F64;

            for value in arr.iter() {
                if !value.is_number() {
                    all_numeric = false;
                    break;
                }

                // Detect specific numeric type from first element
                if let Some(num) = value.as_number() {
                    if num.is_i64() {
                        dtype = NumericDType::I64;
                    } else if num.is_u64() {
                        dtype = NumericDType::U64;
                    }
                    // else keep F64 as default
                }
            }

            if all_numeric {
                return SemanticType::NumericArray {
                    dtype,
                    length: Some(len),
                };
            }
        }

        // Check for time series (simplified)
        if len >= 2 {
            let mut is_time_series = true;

            for value in arr.iter() {
                if let Some(obj) = value.as_object() {
                    if !obj.contains_key(&"timestamp") && !obj.contains_key(&"time") {
                        is_time_series = false;
                        break;
                    }
                } else {
                    is_time_series = false;
                    break;
                }
            }

            if is_time_series {
                return SemanticType::TimeSeries {
                    timestamp_field: "timestamp".to_string(),
                    value_fields: SmallVec::from_vec(vec!["value".to_string()]),
                    interval_ms: None,
                };
            }
        }

        SemanticType::Generic
    }
}

/// Simplified lazy frame for future implementation
pub struct LazyFrame<'a> {
    frame: Frame,
    parser: &'a SonicParser,
}

impl<'a> LazyFrame<'a> {
    /// Get the parsed frame
    pub fn frame(&self) -> &Frame {
        &self.frame
    }
}

impl Default for SonicParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sonic_parser_creation() {
        let parser = SonicParser::new();
        assert!(parser.config.detect_semantics);
        assert_eq!(parser.config.max_input_size, 100 * 1024 * 1024);
    }

    #[test]
    fn test_sonic_basic_parsing() {
        let parser = SonicParser::new();
        let json = br#"{"name": "test", "value": 42}"#;

        let result = parser.parse(json);
        assert!(result.is_ok());

        let frame = result.unwrap();
        assert_eq!(frame.header.version, 1);
        assert_eq!(frame.payload.len(), json.len());
    }

    #[test]
    fn test_sonic_numeric_array_detection() {
        let parser = SonicParser::new();
        let json = br#"[1.5, 2.7, 3.14, 4.2, 5.1]"#;

        let result = parser.parse(json).unwrap();
        if let Some(semantics) = result.semantics {
            assert!(matches!(
                semantics.semantic_type,
                SemanticType::NumericArray { .. }
            ));
        } else {
            panic!("Expected semantic metadata");
        }
    }

    #[test]
    fn test_sonic_time_series_detection() {
        let parser = SonicParser::new();
        let json = br#"[
            {"timestamp": "2023-01-01T00:00:00Z", "value": 1.5},
            {"timestamp": "2023-01-01T00:01:00Z", "value": 2.3}
        ]"#;

        let result = parser.parse(json).unwrap();
        if let Some(semantics) = result.semantics {
            assert!(matches!(
                semantics.semantic_type,
                SemanticType::TimeSeries { .. }
            ));
        } else {
            panic!("Expected semantic metadata");
        }
    }

    #[test]
    fn test_sonic_performance_config() {
        let config = SonicConfig {
            detect_semantics: false,
            max_input_size: 1024,
        };

        let parser = SonicParser::with_config(config);
        assert!(!parser.config.detect_semantics);
        assert_eq!(parser.config.max_input_size, 1024);
    }
}
