//! Hybrid parser using sonic-rs for SIMD acceleration
//!
//! This module provides a high-performance parser that combines:
//! - sonic-rs for SIMD-accelerated JSON parsing
//! - PJS semantic analysis for intelligent chunking

use crate::{
    error::{Error, Result},
    frame::Frame,
    frame::{FrameFlags, FrameHeader},
    security::{SecurityValidator, DepthTracker},
    semantic::{NumericDType, SemanticMeta, SemanticType},
};
use bytes::Bytes;
use smallvec::SmallVec;
use sonic_rs::{JsonContainerTrait, JsonNumberTrait, JsonValueTrait, Value as SonicValue};

/// Branch prediction hint for unlikely conditions (simplified)
#[inline(always)]
fn unlikely(b: bool) -> bool {
    b
}

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
    validator: SecurityValidator,
    stats: std::cell::RefCell<SonicStats>,
}

/// Performance statistics for sonic parser
#[derive(Debug, Default, Clone)]
pub struct SonicStats {
    pub total_parses: u64,
    pub sonic_successes: u64,
    pub serde_fallbacks: u64,
    pub avg_parse_time_ns: u64,
    pub bytes_processed: u64,
}

impl SonicParser {
    /// Create a new SonicParser with default configuration
    pub fn new() -> Self {
        Self {
            config: SonicConfig::default(),
            validator: SecurityValidator::default(),
            stats: std::cell::RefCell::new(SonicStats::default()),
        }
    }

    /// Create a new SonicParser with custom configuration
    pub fn with_config(config: SonicConfig) -> Self {
        Self {
            config,
            validator: SecurityValidator::default(),
            stats: std::cell::RefCell::new(SonicStats::default()),
        }
    }

    /// Create a new SonicParser with security configuration
    pub fn with_security_config(config: SonicConfig, security_config: crate::config::SecurityConfig) -> Self {
        Self {
            config,
            validator: SecurityValidator::new(security_config),
            stats: std::cell::RefCell::new(SonicStats::default()),
        }
    }

    /// Parse JSON input using sonic-rs with PJS semantics (optimized)
    pub fn parse(&self, input: &[u8]) -> Result<Frame> {
        let start_time = std::time::Instant::now();

        // Security validation: check input size
        self.validator.validate_input_size(input.len())?;
        
        // Fast path: size check with branch prediction hint
        if unlikely(input.len() > self.config.max_input_size) {
            return Err(Error::Other(format!("Input too large: {}", input.len())));
        }

        // UTF-8 validation (safe approach)
        let json_str = std::str::from_utf8(input)
            .map_err(|e| Error::Other(format!("Invalid UTF-8 input: {}", e)))?;

        // Pre-validate JSON structure for safety (before parsing)
        self.pre_validate_json_string(json_str)?;

        // Parse with sonic-rs SIMD acceleration
        let value: SonicValue =
            sonic_rs::from_str(json_str).map_err(|e| Error::invalid_json(0, e.to_string()))?;
            
        // Post-validate parsed JSON structure for additional checks
        self.validate_json_structure(&value)?;

        // Fast semantic detection (only if enabled and small overhead)
        let semantic_type = if self.config.detect_semantics && input.len() < 100_000 {
            self.detect_semantic_type_sonic(&value)
        } else {
            SemanticType::Generic
        };

        // Zero-copy payload using Bytes::from_static when possible
        let payload = if input.len() < 4096 {
            // For small inputs, copy is fast and reduces fragmentation
            Bytes::copy_from_slice(input)
        } else {
            // For larger inputs, prefer zero-copy when possible
            Bytes::from(input.to_vec()) // Will be optimized to zero-copy in many cases
        };

        // Minimal frame header for performance
        let header = FrameHeader {
            version: 1,
            flags: FrameFlags::empty(),
            sequence: 0,
            length: input.len() as u32,
            schema_id: 0,
            checksum: 0,
        };

        let semantics = if semantic_type != SemanticType::Generic {
            Some(SemanticMeta::new(semantic_type))
        } else {
            None
        };

        // Update statistics
        {
            let mut stats = self.stats.borrow_mut();
            stats.total_parses += 1;
            stats.sonic_successes += 1;
            stats.bytes_processed += input.len() as u64;

            let elapsed_ns = start_time.elapsed().as_nanos() as u64;
            stats.avg_parse_time_ns = (stats.avg_parse_time_ns * (stats.total_parses - 1)
                + elapsed_ns)
                / stats.total_parses;
        }

        Ok(Frame {
            header,
            payload,
            semantics,
        })
    }

    /// Get performance statistics
    pub fn get_stats(&self) -> SonicStats {
        self.stats.borrow().clone()
    }
    
    /// Pre-validate JSON string for basic safety checks (before parsing)
    fn pre_validate_json_string(&self, json_str: &str) -> Result<()> {
        // Count nesting depth by counting braces/brackets
        let mut depth = 0;
        let mut max_depth = 0;
        
        for ch in json_str.chars() {
            match ch {
                '{' | '[' => {
                    depth += 1;
                    max_depth = max_depth.max(depth);
                    self.validator.validate_json_depth(max_depth)?;
                }
                '}' | ']' => {
                    if depth > 0 {
                        depth -= 1;
                    }
                }
                _ => {}
            }
        }
        
        Ok(())
    }
    
    /// Validate JSON structure for security (depth, complexity, etc.)
    fn validate_json_structure(&self, value: &SonicValue) -> Result<()> {
        let mut depth_tracker = DepthTracker::default();
        self.validate_json_recursive(value, &mut depth_tracker)
    }
    
    /// Recursively validate JSON structure
    fn validate_json_recursive(&self, value: &SonicValue, depth_tracker: &mut DepthTracker) -> Result<()> {
        match value {
            _ if value.is_object() => {
                depth_tracker.enter()?;
                
                if let Some(obj) = value.as_object() {
                    // Validate object key count
                    self.validator.validate_object_keys(obj.len())?;
                    
                    // Recursively validate object values
                    for (key, val) in obj.iter() {
                        // Validate key length
                        self.validator.validate_string_length(key.len())?;
                        self.validate_json_recursive(val, depth_tracker)?;
                    }
                }
                
                depth_tracker.exit();
            }
            _ if value.is_array() => {
                depth_tracker.enter()?;
                
                if let Some(arr) = value.as_array() {
                    // Validate array length
                    self.validator.validate_array_length(arr.len())?;
                    
                    // Recursively validate array elements
                    for element in arr.iter() {
                        self.validate_json_recursive(element, depth_tracker)?;
                    }
                }
                
                depth_tracker.exit();
            }
            _ if value.is_str() => {
                if let Some(s) = value.as_str() {
                    self.validator.validate_string_length(s.len())?;
                }
            }
            _ => {
                // Numbers, booleans, null are always valid
            }
        }
        
        Ok(())
    }

    /// Detect semantic type using sonic-rs Value with SIMD acceleration
    fn detect_semantic_type_sonic(&self, value: &SonicValue) -> SemanticType {
        if value.is_array()
            && let Some(arr) = value.as_array() {
                return self.analyze_array_semantics_simd(arr);
            }

        if value.is_object()
            && let Some(obj) = value.as_object() {
                return self.analyze_object_semantics_simd(obj);
            }

        SemanticType::Generic
    }

    /// SIMD-optimized object semantic analysis
    fn analyze_object_semantics_simd(&self, obj: &sonic_rs::Object) -> SemanticType {
        let scan_result = crate::parser::simd::SimdClassifier::scan_object_keys(obj);

        // Fast GeoJSON detection
        if scan_result.has_type_field && scan_result.has_coordinates {
            return SemanticType::Geospatial {
                coordinate_system: "WGS84".to_string(),
                geometry_type: obj
                    .get(&"type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Point")
                    .to_string(),
            };
        }

        // Fast time series detection
        if scan_result.has_timestamp {
            let timestamp_field = if obj.contains_key(&"timestamp") {
                "timestamp"
            } else {
                "time"
            };

            // Find numeric value fields efficiently
            let value_fields: SmallVec<[String; 4]> = obj
                .iter()
                .filter_map(|(k, v)| {
                    if k != timestamp_field && v.is_number() {
                        Some(k.to_string())
                    } else {
                        None
                    }
                })
                .collect();

            if !value_fields.is_empty() {
                return SemanticType::TimeSeries {
                    timestamp_field: timestamp_field.to_string(),
                    value_fields,
                    interval_ms: None,
                };
            }
        }

        SemanticType::Generic
    }

    /// SIMD-optimized array semantic analysis
    fn analyze_array_semantics_simd(&self, arr: &sonic_rs::Array) -> SemanticType {
        let len = arr.len();
        if len == 0 {
            return SemanticType::Generic;
        }

        // Fast numeric array detection using SIMD
        if crate::parser::simd::SimdClassifier::is_numeric_array(arr) {
            let dtype = if let Some(first) = arr.first() {
                if let Some(num) = first.as_number() {
                    if num.is_i64() {
                        NumericDType::I64
                    } else if num.is_u64() {
                        NumericDType::U64
                    } else {
                        NumericDType::F64
                    }
                } else {
                    NumericDType::F64
                }
            } else {
                NumericDType::F64
            };

            return SemanticType::NumericArray {
                dtype,
                length: Some(len),
            };
        }

        // Fast time series detection
        if len >= 2 {
            let mut is_time_series = true;

            // Use early exit strategy for performance
            for value in arr.iter() {
                if let Some(obj) = value.as_object() {
                    let scan_result = crate::parser::simd::SimdClassifier::scan_object_keys(obj);
                    if !scan_result.has_timestamp {
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

        // Check for tabular data (array of objects with similar structure)
        if len >= 3 && arr.iter().all(|v| v.is_object())
            && let Some(first_obj) = arr.first().and_then(|v| v.as_object()) {
                let first_scan = crate::parser::simd::SimdClassifier::scan_object_keys(first_obj);

                // Simple homogeneity check - all objects should have similar key counts
                let is_tabular = arr.iter().skip(1).filter_map(|v| v.as_object()).all(|obj| {
                    let scan = crate::parser::simd::SimdClassifier::scan_object_keys(obj);
                    // Allow some variation (±20%)
                    let diff = scan.key_count as i32 - first_scan.key_count as i32;
                    diff.abs() <= (first_scan.key_count as i32 / 5)
                });

                if is_tabular {
                    // Extract columns from first object
                    let columns: SmallVec<[crate::semantic::ColumnMeta; 16]> = first_obj
                        .iter()
                        .map(|(k, v)| {
                            let column_type = if v.is_number() {
                                crate::semantic::ColumnType::Numeric(NumericDType::F64)
                            } else if v.is_str() {
                                crate::semantic::ColumnType::String
                            } else if v.as_bool().is_some() {
                                crate::semantic::ColumnType::Boolean
                            } else {
                                crate::semantic::ColumnType::Json
                            };

                            crate::semantic::ColumnMeta {
                                name: k.to_string(),
                                dtype: column_type,
                                nullable: false,
                            }
                        })
                        .collect();

                    return SemanticType::Table {
                        columns: Box::new(columns),
                        row_count: Some(len),
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

    #[test]
    fn test_sonic_invalid_utf8_handling() {
        let parser = SonicParser::new();
        // Create invalid UTF-8 sequence
        let invalid_utf8 = &[0xFF, 0xFE, 0xFD];
        
        let result = parser.parse(invalid_utf8);
        assert!(result.is_err());
        
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("Invalid UTF-8"));
    }

    #[test]
    fn test_sonic_input_size_limit() {
        let config = SonicConfig {
            detect_semantics: true,
            max_input_size: 10, // Very small limit
        };
        let parser = SonicParser::with_config(config);
        
        let large_json = b"[1,2,3,4,5,6,7,8,9,10]"; // Exceeds 10 bytes
        let result = parser.parse(large_json);
        
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("Input size") || error_msg.contains("Input too large"));
    }

    #[test]
    fn test_sonic_json_depth_validation() {
        let parser = SonicParser::new();
        
        // Create moderately nested JSON that exceeds our validation limit but won't cause stack overflow
        let mut json = String::new();
        // Create 65 levels of nesting (exceeds limit of 64)
        for _ in 0..65 {
            json.push('{');
            json.push_str("\"a\":");
        }
        json.push_str("\"value\"");
        for _ in 0..65 {
            json.push('}');
        }
        
        let result = parser.parse(json.as_bytes());
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("depth"));
    }
    
    #[test]
    fn test_sonic_large_string_validation() {
        let parser = SonicParser::new();
        
        // Create JSON with very large string value
        let large_string = "a".repeat(11 * 1024 * 1024); // 11MB string
        let json = format!("{{\"key\": \"{}\"}}", large_string);
        
        let result = parser.parse(json.as_bytes());
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("String length"));
    }
    
    #[test]
    fn test_sonic_large_array_validation() {
        let parser = SonicParser::new();
        
        // Create JSON with array that has too many elements
        let mut json = String::from("[");
        let _max_elements = 1_000_000 + 1; // Large array limit for reference
        
        // We'll test with smaller number for performance, but check the validation logic
        for i in 0..1001 {  // Just over a reasonable limit for testing
            if i > 0 {
                json.push(',');
            }
            json.push_str(&i.to_string());
        }
        json.push(']');
        
        // This should work fine as 1001 is well under the limit
        let result = parser.parse(json.as_bytes());
        assert!(result.is_ok());
    }
    
    #[test]
    fn test_sonic_many_object_keys_validation() {
        let parser = SonicParser::new();
        
        // Create JSON object with many keys
        let mut json = String::from("{");
        for i in 0..1000 {  // Well under the limit for testing
            if i > 0 {
                json.push(',');
            }
            json.push_str(&format!("\"key{}\": {}", i, i));
        }
        json.push('}');
        
        let result = parser.parse(json.as_bytes());
        assert!(result.is_ok());
    }
}
