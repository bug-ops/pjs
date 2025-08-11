//! Semantic type hints for automatic optimization

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

/// Semantic type hints that enable automatic optimization
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SemanticType {
    /// Array of homogeneous numeric data (SIMD-friendly)
    NumericArray {
        /// Data type of array elements
        dtype: NumericDType,
        /// Number of elements (if known)
        length: Option<usize>,
    },

    /// Time series data with timestamp and values
    TimeSeries {
        /// Field name containing timestamps
        timestamp_field: String,
        /// Field names containing values
        value_fields: SmallVec<[String; 4]>,
        /// Optional sampling interval hint
        interval_ms: Option<u64>,
    },

    /// Tabular data (columnar processing friendly)
    Table {
        /// Column metadata
        columns: Box<SmallVec<[ColumnMeta; 16]>>,
        /// Estimated row count
        row_count: Option<usize>,
    },

    /// Graph/tree structure
    Graph {
        /// Node type identifier
        node_type: String,
        /// Edge type identifier
        edge_type: String,
        /// Estimated node count
        node_count: Option<usize>,
    },

    /// Geospatial data
    Geospatial {
        /// Coordinate system (e.g., "WGS84", "UTM")
        coordinate_system: String,
        /// Geometry type (Point, LineString, Polygon, etc.)
        geometry_type: String,
    },

    /// Image/matrix data
    Matrix {
        /// Matrix dimensions
        dimensions: SmallVec<[usize; 4]>,
        /// Element data type
        dtype: NumericDType,
    },

    /// Generic JSON (no specific optimization)
    Generic,
}

/// Numeric data types for type-specific optimization
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NumericDType {
    /// 64-bit float
    F64,
    /// 32-bit float
    F32,
    /// 64-bit signed integer
    I64,
    /// 32-bit signed integer
    I32,
    /// 16-bit signed integer
    I16,
    /// 8-bit signed integer
    I8,
    /// 64-bit unsigned integer
    U64,
    /// 32-bit unsigned integer
    U32,
    /// 16-bit unsigned integer
    U16,
    /// 8-bit unsigned integer
    U8,
}

/// Column metadata for tabular data
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColumnMeta {
    /// Column name
    pub name: String,
    /// Column data type
    pub dtype: ColumnType,
    /// Whether column allows null values
    pub nullable: bool,
}

/// Column data types
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ColumnType {
    /// Numeric column
    Numeric(NumericDType),
    /// String/text column
    String,
    /// Boolean column
    Boolean,
    /// Timestamp column
    Timestamp,
    /// JSON object column
    Json,
    /// Array column with element type
    Array(Box<ColumnType>),
}

/// Complete semantic metadata for a frame
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SemanticMeta {
    /// Primary semantic type
    pub semantic_type: SemanticType,
    /// Optional secondary types for mixed data
    pub secondary_types: SmallVec<[SemanticType; 2]>,
    /// Processing hints
    pub hints: ProcessingHints,
}

/// Processing hints for optimization
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessingHints {
    /// Prefer SIMD processing
    pub prefer_simd: bool,
    /// Prefer GPU processing
    pub prefer_gpu: bool,
    /// Prefer parallel processing
    pub prefer_parallel: bool,
    /// Memory access pattern hint
    pub access_pattern: AccessPattern,
    /// Compression hint
    pub compression_hint: CompressionHint,
}

/// Memory access pattern hints
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum AccessPattern {
    /// Sequential access
    Sequential,
    /// Random access
    Random,
    /// Streaming (read-once)
    Streaming,
}

/// Compression strategy hints
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum CompressionHint {
    /// No compression preferred
    None,
    /// Fast compression (LZ4)
    Fast,
    /// Balanced compression
    Balanced,
    /// Maximum compression
    Maximum,
}

impl SemanticType {
    /// Get the primary numeric data type if applicable
    pub fn numeric_dtype(&self) -> Option<NumericDType> {
        match self {
            Self::NumericArray { dtype, .. } => Some(*dtype),
            Self::Matrix { dtype, .. } => Some(*dtype),
            _ => None,
        }
    }

    /// Check if type is suitable for SIMD processing
    pub fn is_simd_friendly(&self) -> bool {
        matches!(self, Self::NumericArray { .. } | Self::Matrix { .. })
    }

    /// Check if type is suitable for columnar processing
    pub fn is_columnar(&self) -> bool {
        matches!(self, Self::Table { .. } | Self::TimeSeries { .. })
    }

    /// Get estimated data size hint
    pub fn size_hint(&self) -> Option<usize> {
        match self {
            Self::NumericArray {
                dtype,
                length: Some(len),
            } => Some(len * dtype.size()),
            Self::Table {
                row_count: Some(rows),
                columns,
            } => {
                Some(rows * columns.len() * 8) // Rough estimate
            }
            Self::Matrix { dimensions, dtype } => {
                Some(dimensions.iter().product::<usize>() * dtype.size())
            }
            _ => None,
        }
    }
}

impl NumericDType {
    /// Get size in bytes
    pub fn size(self) -> usize {
        match self {
            Self::F64 | Self::I64 | Self::U64 => 8,
            Self::F32 | Self::I32 | Self::U32 => 4,
            Self::I16 | Self::U16 => 2,
            Self::I8 | Self::U8 => 1,
        }
    }

    /// Check if type is floating-point
    pub fn is_float(self) -> bool {
        matches!(self, Self::F32 | Self::F64)
    }

    /// Check if type is signed
    pub fn is_signed(self) -> bool {
        matches!(
            self,
            Self::I8 | Self::I16 | Self::I32 | Self::I64 | Self::F32 | Self::F64
        )
    }
}

impl Default for ProcessingHints {
    fn default() -> Self {
        Self {
            prefer_simd: false,
            prefer_gpu: false,
            prefer_parallel: true,
            access_pattern: AccessPattern::Sequential,
            compression_hint: CompressionHint::Balanced,
        }
    }
}

impl SemanticMeta {
    /// Create new semantic metadata
    pub fn new(semantic_type: SemanticType) -> Self {
        Self {
            semantic_type,
            secondary_types: SmallVec::new(),
            hints: ProcessingHints::default(),
        }
    }

    /// Create with explicit hints
    pub fn with_hints(semantic_type: SemanticType, hints: ProcessingHints) -> Self {
        Self {
            semantic_type,
            secondary_types: SmallVec::new(),
            hints,
        }
    }

    /// Add secondary semantic type
    pub fn with_secondary(mut self, secondary_type: SemanticType) -> Self {
        self.secondary_types.push(secondary_type);
        self
    }

    /// Get the best processing strategy based on semantics
    pub fn processing_strategy(&self) -> ProcessingStrategy {
        // Prefer explicit hints first
        if self.hints.prefer_gpu {
            return ProcessingStrategy::Gpu;
        }

        if self.hints.prefer_simd && self.semantic_type.is_simd_friendly() {
            return ProcessingStrategy::Simd;
        }

        // Auto-select based on semantic type
        match &self.semantic_type {
            SemanticType::NumericArray {
                length: Some(len), ..
            } if *len > 1000 => ProcessingStrategy::Simd,
            SemanticType::Table {
                row_count: Some(rows),
                ..
            } if *rows > 10000 => ProcessingStrategy::Columnar,
            SemanticType::TimeSeries { .. } => ProcessingStrategy::Streaming,
            _ => ProcessingStrategy::Generic,
        }
    }
}

/// Processing strategy recommendation
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProcessingStrategy {
    /// Use SIMD-optimized parsing
    Simd,
    /// Use GPU acceleration
    Gpu,
    /// Use columnar processing
    Columnar,
    /// Use streaming processing
    Streaming,
    /// Use generic processing
    Generic,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_semantic_type_creation() {
        let numeric_array = SemanticType::NumericArray {
            dtype: NumericDType::F64,
            length: Some(1000),
        };

        assert!(numeric_array.is_simd_friendly());
        assert_eq!(numeric_array.numeric_dtype(), Some(NumericDType::F64));
        assert_eq!(numeric_array.size_hint(), Some(8000)); // 1000 * 8 bytes
    }

    #[test]
    fn test_processing_strategy() {
        let meta = SemanticMeta::new(SemanticType::NumericArray {
            dtype: NumericDType::F32,
            length: Some(2000),
        });

        assert_eq!(meta.processing_strategy(), ProcessingStrategy::Simd);
    }

    #[test]
    fn test_column_meta() {
        let column = ColumnMeta {
            name: "value".to_string(),
            dtype: ColumnType::Numeric(NumericDType::F64),
            nullable: false,
        };

        assert_eq!(column.name, "value");
        assert!(!column.nullable);
    }
}
