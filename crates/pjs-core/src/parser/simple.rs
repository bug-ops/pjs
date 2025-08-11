//! Simplified serde-based parser for PJS MVP
//!
//! This parser uses serde_json as the foundation and focuses on PJS's
//! unique features: semantic analysis, chunking, and streaming.

use crate::semantic::{SemanticMeta, SemanticType};
use crate::{Error, Frame, Result};
use bytes::Bytes;
use serde_json::{self, Map, Value};
use smallvec::SmallVec;

/// Simple parser based on serde_json
pub struct SimpleParser {
    config: ParseConfig,
}

/// Parser configuration
#[derive(Debug, Clone)]
pub struct ParseConfig {
    /// Enable automatic semantic type detection
    pub detect_semantics: bool,
    /// Maximum JSON size to parse at once (MB)
    pub max_size_mb: usize,
    /// Enable streaming for large arrays
    pub stream_large_arrays: bool,
    /// Array size threshold for streaming
    pub stream_threshold: usize,
}

impl Default for ParseConfig {
    fn default() -> Self {
        Self {
            detect_semantics: true,
            max_size_mb: 100,
            stream_large_arrays: true,
            stream_threshold: 1000,
        }
    }
}

impl SimpleParser {
    /// Create new simple parser with default config
    pub fn new() -> Self {
        Self {
            config: ParseConfig::default(),
        }
    }

    /// Create parser with custom config
    pub fn with_config(config: ParseConfig) -> Self {
        Self { config }
    }

    /// Parse JSON bytes into PJS Frame
    pub fn parse(&self, input: &[u8]) -> Result<Frame> {
        // Basic size check
        if input.len() > self.config.max_size_mb * 1024 * 1024 {
            let input_mb = input.len() / (1024 * 1024);
            let max_mb = self.config.max_size_mb;
            return Err(Error::buffer(format!(
                "Input too large: {input_mb} MB, max: {max_mb} MB"
            )));
        }

        // Parse with serde_json
        let value: Value = serde_json::from_slice(input)
            .map_err(|e| Error::invalid_json(0, format!("serde_json error: {e}")))?;

        // Detect semantic type
        let semantic_type = if self.config.detect_semantics {
            self.detect_semantic_type(&value)
        } else {
            SemanticType::Generic
        };

        // Create semantic metadata
        let semantics = Some(SemanticMeta::new(semantic_type));

        // Create frame
        let mut frame = Frame::new(Bytes::copy_from_slice(input));
        frame.semantics = semantics;

        Ok(frame)
    }

    /// Parse with explicit semantic hints
    pub fn parse_with_semantics(&self, input: &[u8], semantics: &SemanticMeta) -> Result<Frame> {
        let mut frame = self.parse(input)?;
        frame.semantics = Some(semantics.clone());
        Ok(frame)
    }

    /// Detect semantic type from JSON structure
    fn detect_semantic_type(&self, value: &Value) -> SemanticType {
        match value {
            Value::Array(arr) => self.detect_array_semantics(arr),
            Value::Object(obj) => self.detect_object_semantics(obj),
            _ => SemanticType::Generic,
        }
    }

    /// Detect semantic type for JSON arrays
    fn detect_array_semantics(&self, arr: &[Value]) -> SemanticType {
        if arr.is_empty() {
            return SemanticType::Generic;
        }

        // Check if it's a numeric array
        if self.is_numeric_array(arr) {
            let dtype = self.detect_numeric_dtype(&arr[0]);
            return SemanticType::NumericArray {
                dtype,
                length: Some(arr.len()),
            };
        }

        // Check if it's time series data
        if self.is_time_series_array(arr) {
            return SemanticType::TimeSeries {
                timestamp_field: "timestamp".to_string(),
                value_fields: SmallVec::from_vec(vec!["value".to_string()]),
                interval_ms: None,
            };
        }

        // Check if it's tabular data
        if self.is_tabular_data(arr) {
            let columns = self.extract_table_columns(&arr[0]);
            return SemanticType::Table {
                columns: Box::new(columns),
                row_count: Some(arr.len()),
            };
        }

        SemanticType::Generic
    }

    /// Detect semantic type for JSON objects
    fn detect_object_semantics(&self, obj: &Map<String, Value>) -> SemanticType {
        // GeoJSON detection
        if obj.contains_key("type") && obj.contains_key("coordinates") {
            return SemanticType::Geospatial {
                coordinate_system: "WGS84".to_string(),
                geometry_type: obj
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Point")
                    .to_string(),
            };
        }

        // Time series single point
        if obj.contains_key("timestamp") || obj.contains_key("time") {
            let timestamp_field = if obj.contains_key("timestamp") {
                "timestamp"
            } else {
                "time"
            };

            let value_fields: SmallVec<[String; 4]> = obj
                .keys()
                .filter(|k| {
                    // TODO: Handle unwrap() - add proper error handling for object field access
                    *k != timestamp_field && self.looks_like_numeric_value(obj.get(*k).unwrap())
                })
                .map(|k| k.clone())
                .collect();

            if !value_fields.is_empty() {
                return SemanticType::TimeSeries {
                    timestamp_field: timestamp_field.to_string(),
                    value_fields,
                    interval_ms: None,
                };
            }
        }

        // Matrix/image data detection
        if obj.contains_key("data") && obj.contains_key("shape") {
            if let (Some(Value::Array(_)), Some(Value::Array(shape))) =
                (obj.get("data"), obj.get("shape"))
            {
                let dimensions: SmallVec<[usize; 4]> = shape
                    .iter()
                    .filter_map(|v| v.as_u64().map(|n| n as usize))
                    .collect();

                if !dimensions.is_empty() {
                    return SemanticType::Matrix {
                        dimensions,
                        dtype: crate::semantic::NumericDType::F64, // Default
                    };
                }
            }
        }

        SemanticType::Generic
    }

    /// Check if array contains only numeric values
    fn is_numeric_array(&self, arr: &[Value]) -> bool {
        arr.len() > 2 && arr.iter().all(|v| v.is_number())
    }

    /// Check if array looks like time series data
    fn is_time_series_array(&self, arr: &[Value]) -> bool {
        arr.len() >= 2
            && arr.iter().all(|v| {
                if let Value::Object(obj) = v {
                    obj.contains_key("timestamp") || obj.contains_key("time")
                } else {
                    false
                }
            })
    }

    /// Check if array looks like tabular data (array of similar objects)
    fn is_tabular_data(&self, arr: &[Value]) -> bool {
        if arr.len() < 2 {
            return false;
        }

        // Check if all elements are objects with similar structure
        let first_keys: std::collections::HashSet<_> = if let Value::Object(first) = &arr[0] {
            first.keys().collect()
        } else {
            return false;
        };

        arr.iter().all(|v| {
            if let Value::Object(obj) = v {
                let keys: std::collections::HashSet<_> = obj.keys().collect();
                // Allow some variation in keys (80% similarity)
                let intersection = first_keys.intersection(&keys).count();
                let union = first_keys.union(&keys).count();
                intersection as f64 / union as f64 > 0.8
            } else {
                false
            }
        })
    }

    /// Detect numeric data type from Value
    fn detect_numeric_dtype(&self, value: &Value) -> crate::semantic::NumericDType {
        match value {
            Value::Number(n) => {
                if n.is_i64() {
                    crate::semantic::NumericDType::I64
                } else if n.is_u64() {
                    crate::semantic::NumericDType::U64
                } else {
                    crate::semantic::NumericDType::F64
                }
            }
            _ => crate::semantic::NumericDType::F64,
        }
    }

    /// Extract table columns from first object
    fn extract_table_columns(
        &self,
        first_obj: &Value,
    ) -> SmallVec<[crate::semantic::ColumnMeta; 16]> {
        let mut columns = SmallVec::new();

        if let Value::Object(obj) = first_obj {
            for (key, value) in obj {
                let column_type = self.detect_column_type(value);
                columns.push(crate::semantic::ColumnMeta {
                    name: key.clone(),
                    dtype: column_type,
                    nullable: false, // Simplified - would need more analysis
                });
            }
        }

        columns
    }

    /// Detect column type from JSON value
    fn detect_column_type(&self, value: &Value) -> crate::semantic::ColumnType {
        match value {
            Value::Number(n) => {
                if n.is_i64() {
                    crate::semantic::ColumnType::Numeric(crate::semantic::NumericDType::I64)
                } else if n.is_u64() {
                    crate::semantic::ColumnType::Numeric(crate::semantic::NumericDType::U64)
                } else {
                    crate::semantic::ColumnType::Numeric(crate::semantic::NumericDType::F64)
                }
            }
            Value::String(_) => crate::semantic::ColumnType::String,
            Value::Bool(_) => crate::semantic::ColumnType::Boolean,
            Value::Array(_) => {
                crate::semantic::ColumnType::Array(Box::new(crate::semantic::ColumnType::Json))
            }
            _ => crate::semantic::ColumnType::Json,
        }
    }

    /// Check if value looks like a numeric measurement
    fn looks_like_numeric_value(&self, value: &Value) -> bool {
        value.is_number()
    }

    /// Get parser statistics (placeholder for metrics)
    pub fn stats(&self) -> ParseStats {
        ParseStats {
            total_parses: 0,
            semantic_detections: 0,
            avg_parse_time_ms: 0.0,
        }
    }
}

/// Parser statistics
#[derive(Debug, Default)]
pub struct ParseStats {
    /// Total number of parse operations
    pub total_parses: u64,
    /// Number of successful semantic type detections
    pub semantic_detections: u64,
    /// Average parse time in milliseconds
    pub avg_parse_time_ms: f64,
}

impl Default for SimpleParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_parser_creation() {
        let parser = SimpleParser::new();
        assert!(parser.config.detect_semantics);
    }

    #[test]
    fn test_basic_json_parsing() {
        let parser = SimpleParser::new();
        let json = br#"{"hello": "world", "count": 42}"#;

        let result = parser.parse(json);
        assert!(result.is_ok());

        let frame = result.unwrap();
        assert!(frame.semantics.is_some());
    }

    #[test]
    fn test_numeric_array_detection() {
        let parser = SimpleParser::new();
        let json = b"[1, 2, 3, 4, 5]";

        let result = parser.parse(json).unwrap();
        if let Some(semantics) = result.semantics {
            assert!(matches!(
                semantics.semantic_type,
                SemanticType::NumericArray { .. }
            ));
        }
    }

    #[test]
    fn test_time_series_detection() {
        let parser = SimpleParser::new();
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
        }
    }

    #[test]
    fn test_geospatial_detection() {
        let parser = SimpleParser::new();
        let json = br#"{"type": "Point", "coordinates": [125.6, 10.1]}"#;

        let result = parser.parse(json).unwrap();
        if let Some(semantics) = result.semantics {
            assert!(matches!(
                semantics.semantic_type,
                SemanticType::Geospatial { .. }
            ));
        }
    }

    #[test]
    fn test_tabular_data_detection() {
        let parser = SimpleParser::new();
        let json = br#"[
            {"name": "John", "age": 30, "city": "New York"},
            {"name": "Jane", "age": 25, "city": "Boston"},
            {"name": "Bob", "age": 35, "city": "Chicago"}
        ]"#;

        let result = parser.parse(json).unwrap();
        if let Some(semantics) = result.semantics {
            assert!(matches!(
                semantics.semantic_type,
                SemanticType::Table { .. }
            ));
        }
    }

    #[test]
    fn test_large_input_rejection() {
        let mut parser = SimpleParser::new();
        parser.config.max_size_mb = 1; // 1MB limit

        let large_json = vec![b'a'; 2 * 1024 * 1024]; // 2MB
        let result = parser.parse(&large_json);

        assert!(result.is_err());
    }
}
