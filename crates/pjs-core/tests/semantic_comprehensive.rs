//! Comprehensive tests for semantic.rs module
//!
//! This test suite aims to achieve 70%+ coverage by testing:
//! - All SemanticType variants and their methods
//! - NumericDType properties (size, float detection, signed detection)
//! - ColumnMeta and ColumnType
//! - SemanticMeta creation and processing strategies
//! - ProcessingHints and their defaults
//! - AccessPattern and CompressionHint
//! - Edge cases and boundary conditions

use pjson_rs::semantic::{
    AccessPattern, ColumnMeta, ColumnType, CompressionHint, NumericDType, ProcessingHints,
    ProcessingStrategy, SemanticMeta, SemanticType,
};
use smallvec::SmallVec;

// === SemanticType Tests ===

mod semantic_type_tests {
    use super::*;

    #[test]
    fn test_numeric_array_creation() {
        let numeric_array = SemanticType::NumericArray {
            dtype: NumericDType::F64,
            length: Some(1000),
        };

        assert!(numeric_array.is_simd_friendly());
        assert_eq!(numeric_array.numeric_dtype(), Some(NumericDType::F64));
        assert_eq!(numeric_array.size_hint(), Some(8000)); // 1000 * 8 bytes
    }

    #[test]
    fn test_numeric_array_without_length() {
        let numeric_array = SemanticType::NumericArray {
            dtype: NumericDType::I32,
            length: None,
        };

        assert!(numeric_array.is_simd_friendly());
        assert_eq!(numeric_array.numeric_dtype(), Some(NumericDType::I32));
        assert_eq!(numeric_array.size_hint(), None);
    }

    #[test]
    fn test_time_series_creation() {
        let time_series = SemanticType::TimeSeries {
            timestamp_field: "timestamp".to_string(),
            value_fields: SmallVec::from_vec(vec!["value1".to_string(), "value2".to_string()]),
            interval_ms: Some(1000),
        };

        assert!(!time_series.is_simd_friendly());
        assert!(time_series.is_columnar());
        assert_eq!(time_series.numeric_dtype(), None);
        assert_eq!(time_series.size_hint(), None);
    }

    #[test]
    fn test_time_series_without_interval() {
        let time_series = SemanticType::TimeSeries {
            timestamp_field: "time".to_string(),
            value_fields: SmallVec::from_vec(vec!["temp".to_string()]),
            interval_ms: None,
        };

        assert!(time_series.is_columnar());
    }

    #[test]
    fn test_table_creation() {
        let columns = Box::new(SmallVec::from_vec(vec![
            ColumnMeta {
                name: "id".to_string(),
                dtype: ColumnType::Numeric(NumericDType::I64),
                nullable: false,
            },
            ColumnMeta {
                name: "name".to_string(),
                dtype: ColumnType::String,
                nullable: true,
            },
        ]));

        let table = SemanticType::Table {
            columns,
            row_count: Some(100),
        };

        assert!(!table.is_simd_friendly());
        assert!(table.is_columnar());
        assert_eq!(table.numeric_dtype(), None);
        assert_eq!(table.size_hint(), Some(1600)); // 100 rows * 2 columns * 8 bytes
    }

    #[test]
    fn test_table_without_row_count() {
        let columns = Box::new(SmallVec::from_vec(vec![ColumnMeta {
            name: "value".to_string(),
            dtype: ColumnType::Numeric(NumericDType::F32),
            nullable: false,
        }]));

        let table = SemanticType::Table {
            columns,
            row_count: None,
        };

        assert_eq!(table.size_hint(), None);
    }

    #[test]
    fn test_graph_creation() {
        let graph = SemanticType::Graph {
            node_type: "User".to_string(),
            edge_type: "follows".to_string(),
            node_count: Some(1000),
        };

        assert!(!graph.is_simd_friendly());
        assert!(!graph.is_columnar());
        assert_eq!(graph.numeric_dtype(), None);
    }

    #[test]
    fn test_graph_without_count() {
        let graph = SemanticType::Graph {
            node_type: "Node".to_string(),
            edge_type: "Edge".to_string(),
            node_count: None,
        };

        assert!(!graph.is_simd_friendly());
    }

    #[test]
    fn test_geospatial_creation() {
        let geospatial = SemanticType::Geospatial {
            coordinate_system: "WGS84".to_string(),
            geometry_type: "Point".to_string(),
        };

        assert!(!geospatial.is_simd_friendly());
        assert!(!geospatial.is_columnar());
        assert_eq!(geospatial.numeric_dtype(), None);
    }

    #[test]
    fn test_geospatial_polygon() {
        let geospatial = SemanticType::Geospatial {
            coordinate_system: "UTM".to_string(),
            geometry_type: "Polygon".to_string(),
        };

        assert!(!geospatial.is_simd_friendly());
    }

    #[test]
    fn test_matrix_creation() {
        let matrix = SemanticType::Matrix {
            dimensions: SmallVec::from_vec(vec![100, 100]),
            dtype: NumericDType::F32,
        };

        assert!(matrix.is_simd_friendly());
        assert!(!matrix.is_columnar());
        assert_eq!(matrix.numeric_dtype(), Some(NumericDType::F32));
        assert_eq!(matrix.size_hint(), Some(40000)); // 100 * 100 * 4 bytes
    }

    #[test]
    fn test_matrix_3d() {
        let matrix = SemanticType::Matrix {
            dimensions: SmallVec::from_vec(vec![10, 10, 10]),
            dtype: NumericDType::U8,
        };

        assert!(matrix.is_simd_friendly());
        assert_eq!(matrix.size_hint(), Some(1000)); // 10 * 10 * 10 * 1 byte
    }

    #[test]
    fn test_generic_type() {
        let generic = SemanticType::Generic;

        assert!(!generic.is_simd_friendly());
        assert!(!generic.is_columnar());
        assert_eq!(generic.numeric_dtype(), None);
        assert_eq!(generic.size_hint(), None);
    }

    #[test]
    fn test_semantic_type_clone() {
        let original = SemanticType::NumericArray {
            dtype: NumericDType::F64,
            length: Some(500),
        };
        let cloned = original.clone();

        assert_eq!(original, cloned);
    }
}

// === NumericDType Tests ===

mod numeric_dtype_tests {
    use super::*;

    #[test]
    fn test_f64_properties() {
        let dtype = NumericDType::F64;
        assert_eq!(dtype.size(), 8);
        assert!(dtype.is_float());
        assert!(dtype.is_signed());
    }

    #[test]
    fn test_f32_properties() {
        let dtype = NumericDType::F32;
        assert_eq!(dtype.size(), 4);
        assert!(dtype.is_float());
        assert!(dtype.is_signed());
    }

    #[test]
    fn test_i64_properties() {
        let dtype = NumericDType::I64;
        assert_eq!(dtype.size(), 8);
        assert!(!dtype.is_float());
        assert!(dtype.is_signed());
    }

    #[test]
    fn test_i32_properties() {
        let dtype = NumericDType::I32;
        assert_eq!(dtype.size(), 4);
        assert!(!dtype.is_float());
        assert!(dtype.is_signed());
    }

    #[test]
    fn test_i16_properties() {
        let dtype = NumericDType::I16;
        assert_eq!(dtype.size(), 2);
        assert!(!dtype.is_float());
        assert!(dtype.is_signed());
    }

    #[test]
    fn test_i8_properties() {
        let dtype = NumericDType::I8;
        assert_eq!(dtype.size(), 1);
        assert!(!dtype.is_float());
        assert!(dtype.is_signed());
    }

    #[test]
    fn test_u64_properties() {
        let dtype = NumericDType::U64;
        assert_eq!(dtype.size(), 8);
        assert!(!dtype.is_float());
        assert!(!dtype.is_signed());
    }

    #[test]
    fn test_u32_properties() {
        let dtype = NumericDType::U32;
        assert_eq!(dtype.size(), 4);
        assert!(!dtype.is_float());
        assert!(!dtype.is_signed());
    }

    #[test]
    fn test_u16_properties() {
        let dtype = NumericDType::U16;
        assert_eq!(dtype.size(), 2);
        assert!(!dtype.is_float());
        assert!(!dtype.is_signed());
    }

    #[test]
    fn test_u8_properties() {
        let dtype = NumericDType::U8;
        assert_eq!(dtype.size(), 1);
        assert!(!dtype.is_float());
        assert!(!dtype.is_signed());
    }

    #[test]
    fn test_numeric_dtype_equality() {
        assert_eq!(NumericDType::F64, NumericDType::F64);
        assert_ne!(NumericDType::F64, NumericDType::F32);
        assert_ne!(NumericDType::I32, NumericDType::U32);
    }

    #[test]
    fn test_numeric_dtype_clone() {
        let dtype1 = NumericDType::F64;
        let dtype2 = dtype1;
        assert_eq!(dtype1, dtype2);
    }
}

// === ColumnMeta and ColumnType Tests ===

mod column_tests {
    use super::*;

    #[test]
    fn test_column_meta_creation() {
        let column = ColumnMeta {
            name: "id".to_string(),
            dtype: ColumnType::Numeric(NumericDType::I64),
            nullable: false,
        };

        assert_eq!(column.name, "id");
        assert!(!column.nullable);
    }

    #[test]
    fn test_column_meta_nullable() {
        let column = ColumnMeta {
            name: "optional_field".to_string(),
            dtype: ColumnType::String,
            nullable: true,
        };

        assert!(column.nullable);
    }

    #[test]
    fn test_column_type_numeric() {
        let col_type = ColumnType::Numeric(NumericDType::F32);
        assert!(matches!(col_type, ColumnType::Numeric(_)));
    }

    #[test]
    fn test_column_type_string() {
        let col_type = ColumnType::String;
        assert!(matches!(col_type, ColumnType::String));
    }

    #[test]
    fn test_column_type_boolean() {
        let col_type = ColumnType::Boolean;
        assert!(matches!(col_type, ColumnType::Boolean));
    }

    #[test]
    fn test_column_type_timestamp() {
        let col_type = ColumnType::Timestamp;
        assert!(matches!(col_type, ColumnType::Timestamp));
    }

    #[test]
    fn test_column_type_json() {
        let col_type = ColumnType::Json;
        assert!(matches!(col_type, ColumnType::Json));
    }

    #[test]
    fn test_column_type_array() {
        let col_type = ColumnType::Array(Box::new(ColumnType::Numeric(NumericDType::I32)));
        assert!(matches!(col_type, ColumnType::Array(_)));
    }

    #[test]
    fn test_column_type_nested_array() {
        let col_type = ColumnType::Array(Box::new(ColumnType::Array(Box::new(ColumnType::String))));
        assert!(matches!(col_type, ColumnType::Array(_)));
    }

    #[test]
    fn test_column_meta_clone() {
        let column = ColumnMeta {
            name: "test".to_string(),
            dtype: ColumnType::Boolean,
            nullable: true,
        };
        let cloned = column.clone();
        assert_eq!(column, cloned);
    }
}

// === ProcessingHints Tests ===

mod processing_hints_tests {
    use super::*;

    #[test]
    fn test_processing_hints_default() {
        let hints = ProcessingHints::default();
        assert!(!hints.prefer_simd);
        assert!(!hints.prefer_gpu);
        assert!(hints.prefer_parallel);
        assert_eq!(hints.access_pattern, AccessPattern::Sequential);
        assert_eq!(hints.compression_hint, CompressionHint::Balanced);
    }

    #[test]
    fn test_processing_hints_custom() {
        let hints = ProcessingHints {
            prefer_simd: true,
            prefer_gpu: false,
            prefer_parallel: true,
            access_pattern: AccessPattern::Random,
            compression_hint: CompressionHint::Fast,
        };

        assert!(hints.prefer_simd);
        assert!(!hints.prefer_gpu);
        assert_eq!(hints.access_pattern, AccessPattern::Random);
        assert_eq!(hints.compression_hint, CompressionHint::Fast);
    }

    #[test]
    fn test_access_pattern_sequential() {
        let pattern = AccessPattern::Sequential;
        assert_eq!(pattern, AccessPattern::Sequential);
    }

    #[test]
    fn test_access_pattern_random() {
        let pattern = AccessPattern::Random;
        assert_eq!(pattern, AccessPattern::Random);
    }

    #[test]
    fn test_access_pattern_streaming() {
        let pattern = AccessPattern::Streaming;
        assert_eq!(pattern, AccessPattern::Streaming);
    }

    #[test]
    fn test_compression_hint_none() {
        let hint = CompressionHint::None;
        assert_eq!(hint, CompressionHint::None);
    }

    #[test]
    fn test_compression_hint_fast() {
        let hint = CompressionHint::Fast;
        assert_eq!(hint, CompressionHint::Fast);
    }

    #[test]
    fn test_compression_hint_balanced() {
        let hint = CompressionHint::Balanced;
        assert_eq!(hint, CompressionHint::Balanced);
    }

    #[test]
    fn test_compression_hint_maximum() {
        let hint = CompressionHint::Maximum;
        assert_eq!(hint, CompressionHint::Maximum);
    }

    #[test]
    fn test_processing_hints_clone() {
        let hints1 = ProcessingHints::default();
        let hints2 = hints1.clone();
        assert_eq!(hints1, hints2);
    }
}

// === SemanticMeta Tests ===

mod semantic_meta_tests {
    use super::*;

    #[test]
    fn test_semantic_meta_new() {
        let meta = SemanticMeta::new(SemanticType::Generic);
        assert!(matches!(meta.semantic_type, SemanticType::Generic));
        assert_eq!(meta.secondary_types.len(), 0);
    }

    #[test]
    fn test_semantic_meta_with_hints() {
        let hints = ProcessingHints {
            prefer_simd: true,
            prefer_gpu: false,
            prefer_parallel: true,
            access_pattern: AccessPattern::Sequential,
            compression_hint: CompressionHint::Fast,
        };

        let meta = SemanticMeta::with_hints(SemanticType::Generic, hints.clone());
        assert_eq!(meta.hints, hints);
    }

    #[test]
    fn test_semantic_meta_with_secondary() {
        let meta =
            SemanticMeta::new(SemanticType::Generic).with_secondary(SemanticType::NumericArray {
                dtype: NumericDType::F64,
                length: Some(100),
            });

        assert_eq!(meta.secondary_types.len(), 1);
    }

    #[test]
    fn test_semantic_meta_multiple_secondary() {
        let meta = SemanticMeta::new(SemanticType::Generic)
            .with_secondary(SemanticType::NumericArray {
                dtype: NumericDType::F64,
                length: Some(100),
            })
            .with_secondary(SemanticType::TimeSeries {
                timestamp_field: "time".to_string(),
                value_fields: SmallVec::new(),
                interval_ms: None,
            });

        assert_eq!(meta.secondary_types.len(), 2);
    }

    #[test]
    fn test_processing_strategy_gpu_preference() {
        let mut hints = ProcessingHints::default();
        hints.prefer_gpu = true;

        let meta = SemanticMeta::with_hints(SemanticType::Generic, hints);
        assert_eq!(meta.processing_strategy(), ProcessingStrategy::Gpu);
    }

    #[test]
    fn test_processing_strategy_simd_preference() {
        let mut hints = ProcessingHints::default();
        hints.prefer_simd = true;

        let meta = SemanticMeta::with_hints(
            SemanticType::NumericArray {
                dtype: NumericDType::F32,
                length: Some(100),
            },
            hints,
        );
        assert_eq!(meta.processing_strategy(), ProcessingStrategy::Simd);
    }

    #[test]
    fn test_processing_strategy_large_numeric_array() {
        let meta = SemanticMeta::new(SemanticType::NumericArray {
            dtype: NumericDType::F32,
            length: Some(2000),
        });

        assert_eq!(meta.processing_strategy(), ProcessingStrategy::Simd);
    }

    #[test]
    fn test_processing_strategy_small_numeric_array() {
        let meta = SemanticMeta::new(SemanticType::NumericArray {
            dtype: NumericDType::F32,
            length: Some(500),
        });

        // Small array doesn't trigger SIMD automatically
        assert_eq!(meta.processing_strategy(), ProcessingStrategy::Generic);
    }

    #[test]
    fn test_processing_strategy_large_table() {
        let columns = Box::new(SmallVec::from_vec(vec![ColumnMeta {
            name: "value".to_string(),
            dtype: ColumnType::Numeric(NumericDType::F64),
            nullable: false,
        }]));

        let meta = SemanticMeta::new(SemanticType::Table {
            columns,
            row_count: Some(15000),
        });

        assert_eq!(meta.processing_strategy(), ProcessingStrategy::Columnar);
    }

    #[test]
    fn test_processing_strategy_small_table() {
        let columns = Box::new(SmallVec::from_vec(vec![ColumnMeta {
            name: "value".to_string(),
            dtype: ColumnType::Numeric(NumericDType::F64),
            nullable: false,
        }]));

        let meta = SemanticMeta::new(SemanticType::Table {
            columns,
            row_count: Some(100),
        });

        assert_eq!(meta.processing_strategy(), ProcessingStrategy::Generic);
    }

    #[test]
    fn test_processing_strategy_time_series() {
        let meta = SemanticMeta::new(SemanticType::TimeSeries {
            timestamp_field: "time".to_string(),
            value_fields: SmallVec::new(),
            interval_ms: Some(1000),
        });

        assert_eq!(meta.processing_strategy(), ProcessingStrategy::Streaming);
    }

    #[test]
    fn test_processing_strategy_generic() {
        let meta = SemanticMeta::new(SemanticType::Generic);
        assert_eq!(meta.processing_strategy(), ProcessingStrategy::Generic);
    }

    #[test]
    fn test_processing_strategy_boundary_1000() {
        let meta = SemanticMeta::new(SemanticType::NumericArray {
            dtype: NumericDType::F64,
            length: Some(1000),
        });
        // Exactly 1000 doesn't trigger SIMD (needs > 1000)
        assert_eq!(meta.processing_strategy(), ProcessingStrategy::Generic);
    }

    #[test]
    fn test_processing_strategy_boundary_1001() {
        let meta = SemanticMeta::new(SemanticType::NumericArray {
            dtype: NumericDType::F64,
            length: Some(1001),
        });
        assert_eq!(meta.processing_strategy(), ProcessingStrategy::Simd);
    }

    #[test]
    fn test_processing_strategy_boundary_10000_rows() {
        let columns = Box::new(SmallVec::from_vec(vec![ColumnMeta {
            name: "value".to_string(),
            dtype: ColumnType::String,
            nullable: false,
        }]));

        let meta = SemanticMeta::new(SemanticType::Table {
            columns,
            row_count: Some(10000),
        });
        // Exactly 10000 doesn't trigger columnar (needs > 10000)
        assert_eq!(meta.processing_strategy(), ProcessingStrategy::Generic);
    }

    #[test]
    fn test_processing_strategy_boundary_10001_rows() {
        let columns = Box::new(SmallVec::from_vec(vec![ColumnMeta {
            name: "value".to_string(),
            dtype: ColumnType::String,
            nullable: false,
        }]));

        let meta = SemanticMeta::new(SemanticType::Table {
            columns,
            row_count: Some(10001),
        });
        assert_eq!(meta.processing_strategy(), ProcessingStrategy::Columnar);
    }

    #[test]
    fn test_semantic_meta_clone() {
        let meta = SemanticMeta::new(SemanticType::Generic);
        let cloned = meta.clone();
        assert_eq!(meta, cloned);
    }
}

// === ProcessingStrategy Tests ===

mod processing_strategy_tests {
    use super::*;

    #[test]
    fn test_processing_strategy_equality() {
        assert_eq!(ProcessingStrategy::Simd, ProcessingStrategy::Simd);
        assert_ne!(ProcessingStrategy::Simd, ProcessingStrategy::Gpu);
    }

    #[test]
    fn test_processing_strategy_clone() {
        let strategy1 = ProcessingStrategy::Columnar;
        let strategy2 = strategy1;
        assert_eq!(strategy1, strategy2);
    }

    #[test]
    fn test_processing_strategy_debug() {
        let strategy = ProcessingStrategy::Streaming;
        let debug_str = format!("{:?}", strategy);
        assert!(debug_str.contains("Streaming"));
    }
}

// === Edge Cases and Complex Scenarios ===

mod edge_cases_tests {
    use super::*;

    #[test]
    fn test_matrix_1d() {
        let matrix = SemanticType::Matrix {
            dimensions: SmallVec::from_vec(vec![1000]),
            dtype: NumericDType::F64,
        };
        assert_eq!(matrix.size_hint(), Some(8000));
    }

    #[test]
    fn test_matrix_4d() {
        let matrix = SemanticType::Matrix {
            dimensions: SmallVec::from_vec(vec![5, 5, 5, 5]),
            dtype: NumericDType::U8,
        };
        assert_eq!(matrix.size_hint(), Some(625)); // 5^4 * 1
    }

    #[test]
    fn test_time_series_no_value_fields() {
        let time_series = SemanticType::TimeSeries {
            timestamp_field: "ts".to_string(),
            value_fields: SmallVec::new(),
            interval_ms: None,
        };
        assert!(time_series.is_columnar());
    }

    #[test]
    fn test_time_series_many_value_fields() {
        let fields: Vec<String> = (0..10).map(|i| format!("value{}", i)).collect();
        let time_series = SemanticType::TimeSeries {
            timestamp_field: "timestamp".to_string(),
            value_fields: SmallVec::from_vec(fields),
            interval_ms: Some(100),
        };
        assert!(time_series.is_columnar());
    }

    #[test]
    fn test_table_many_columns() {
        let columns: Vec<ColumnMeta> = (0..50)
            .map(|i| ColumnMeta {
                name: format!("col{}", i),
                dtype: ColumnType::Numeric(NumericDType::F32),
                nullable: false,
            })
            .collect();

        let table = SemanticType::Table {
            columns: Box::new(SmallVec::from_vec(columns)),
            row_count: Some(1000),
        };

        assert_eq!(table.size_hint(), Some(400000)); // 1000 * 50 * 8
    }

    #[test]
    fn test_graph_empty_strings() {
        let graph = SemanticType::Graph {
            node_type: String::new(),
            edge_type: String::new(),
            node_count: Some(0),
        };
        assert!(!graph.is_simd_friendly());
    }

    #[test]
    fn test_geospatial_all_geometry_types() {
        for geometry_type in &["Point", "LineString", "Polygon", "MultiPoint"] {
            let geo = SemanticType::Geospatial {
                coordinate_system: "WGS84".to_string(),
                geometry_type: geometry_type.to_string(),
            };
            assert!(!geo.is_simd_friendly());
        }
    }

    #[test]
    fn test_column_array_of_arrays() {
        let col_type = ColumnType::Array(Box::new(ColumnType::Array(Box::new(ColumnType::Array(
            Box::new(ColumnType::Numeric(NumericDType::I32)),
        )))));
        assert!(matches!(col_type, ColumnType::Array(_)));
    }

    #[test]
    fn test_semantic_meta_max_secondary_types() {
        let mut meta = SemanticMeta::new(SemanticType::Generic);
        // Add many secondary types (SmallVec limit test)
        for i in 0..5 {
            meta = meta.with_secondary(SemanticType::NumericArray {
                dtype: NumericDType::F64,
                length: Some(i * 100),
            });
        }
        assert_eq!(meta.secondary_types.len(), 5);
    }

    #[test]
    fn test_processing_hints_all_preferences_enabled() {
        let hints = ProcessingHints {
            prefer_simd: true,
            prefer_gpu: true,
            prefer_parallel: true,
            access_pattern: AccessPattern::Streaming,
            compression_hint: CompressionHint::Maximum,
        };

        // GPU takes precedence
        let meta = SemanticMeta::with_hints(SemanticType::Generic, hints);
        assert_eq!(meta.processing_strategy(), ProcessingStrategy::Gpu);
    }

    #[test]
    fn test_simd_preference_non_simd_friendly_type() {
        let mut hints = ProcessingHints::default();
        hints.prefer_simd = true;

        // Generic is not SIMD-friendly, so preference is ignored
        let meta = SemanticMeta::with_hints(SemanticType::Generic, hints);
        assert_eq!(meta.processing_strategy(), ProcessingStrategy::Generic);
    }
}
