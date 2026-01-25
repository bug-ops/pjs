//! Phase 2 comprehensive coverage tests for compression_integration.rs
//!
//! These tests target uncovered code paths to achieve >= 70% coverage

use pjson_rs::{
    compression::{CompressedData, CompressionStrategy},
    stream::{
        StreamFrame,
        compression_integration::{
            CompressedFrame, DecompressionMetadata, StreamingCompressor, StreamingDecompressor,
        },
    },
};
use pjson_rs_domain::value_objects::Priority;
use serde_json::json;
use std::collections::HashMap;

// ============================================================================
// Decompression frame tests with different strategies
// ============================================================================

#[test]
fn test_decompress_frame_with_dictionary_strategy() {
    let mut decompressor = StreamingDecompressor::new();

    let mut dict_map = HashMap::new();
    dict_map.insert(0, "testKey".to_string());

    let frame = CompressedFrame {
        frame: StreamFrame {
            data: json!({"value": "data"}),
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        },
        compressed_data: CompressedData {
            strategy: CompressionStrategy::Dictionary {
                dictionary: HashMap::new(),
            },
            compressed_size: 10,
            data: json!({"value": "data"}),
            compression_metadata: HashMap::new(),
        },
        decompression_metadata: DecompressionMetadata {
            strategy: CompressionStrategy::Dictionary {
                dictionary: HashMap::new(),
            },
            dictionary_map: dict_map,
            delta_bases: HashMap::new(),
            priority_hints: HashMap::new(),
        },
    };

    let result = decompressor.decompress_frame(frame).unwrap();
    assert_eq!(result.priority, Priority::MEDIUM);
}

#[test]
fn test_decompress_frame_with_delta_strategy() {
    let mut decompressor = StreamingDecompressor::new();

    let frame = CompressedFrame {
        frame: StreamFrame {
            data: json!([{"delta_base": 10.0, "delta_type": "numeric"}, 1.0, 2.0]),
            priority: Priority::HIGH,
            metadata: HashMap::new(),
        },
        compressed_data: CompressedData {
            strategy: CompressionStrategy::Delta {
                base_values: HashMap::new(),
            },
            compressed_size: 20,
            data: json!([{"delta_base": 10.0, "delta_type": "numeric"}, 1.0, 2.0]),
            compression_metadata: HashMap::new(),
        },
        decompression_metadata: DecompressionMetadata {
            strategy: CompressionStrategy::Delta {
                base_values: HashMap::new(),
            },
            dictionary_map: HashMap::new(),
            delta_bases: HashMap::new(),
            priority_hints: HashMap::new(),
        },
    };

    let result = decompressor.decompress_frame(frame).unwrap();
    assert_eq!(result.priority, Priority::HIGH);
}

#[test]
fn test_decompress_frame_with_runlength_strategy() {
    let mut decompressor = StreamingDecompressor::new();

    let frame = CompressedFrame {
        frame: StreamFrame {
            data: json!([{"rle_value": "x", "rle_count": 3}]),
            priority: Priority::LOW,
            metadata: HashMap::new(),
        },
        compressed_data: CompressedData {
            strategy: CompressionStrategy::RunLength,
            compressed_size: 15,
            data: json!([{"rle_value": "x", "rle_count": 3}]),
            compression_metadata: HashMap::new(),
        },
        decompression_metadata: DecompressionMetadata {
            strategy: CompressionStrategy::RunLength,
            dictionary_map: HashMap::new(),
            delta_bases: HashMap::new(),
            priority_hints: HashMap::new(),
        },
    };

    let result = decompressor.decompress_frame(frame).unwrap();
    let arr = result.data.as_array().unwrap();
    assert_eq!(arr.len(), 3);
}

#[test]
fn test_decompress_frame_with_hybrid_strategy() {
    let mut decompressor = StreamingDecompressor::new();

    let frame = CompressedFrame {
        frame: StreamFrame {
            data: json!({"test": "data"}),
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        },
        compressed_data: CompressedData {
            strategy: CompressionStrategy::Hybrid {
                string_dict: HashMap::new(),
                numeric_deltas: HashMap::new(),
            },
            compressed_size: 10,
            data: json!({"test": "data"}),
            compression_metadata: HashMap::new(),
        },
        decompression_metadata: DecompressionMetadata {
            strategy: CompressionStrategy::Hybrid {
                string_dict: HashMap::new(),
                numeric_deltas: HashMap::new(),
            },
            dictionary_map: HashMap::new(),
            delta_bases: HashMap::new(),
            priority_hints: HashMap::new(),
        },
    };

    let result = decompressor.decompress_frame(frame);
    assert!(result.is_ok());
}

// ============================================================================
// Decompression delta tests via frame decompression
// ============================================================================

#[test]
fn test_decompress_delta_via_frame() {
    let mut decompressor = StreamingDecompressor::new();

    let frame = CompressedFrame {
        frame: StreamFrame {
            data: json!([{"delta_base": 100.0, "delta_type": "numeric"}, -10.0, 5.0]),
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        },
        compressed_data: CompressedData {
            strategy: CompressionStrategy::Delta {
                base_values: HashMap::new(),
            },
            compressed_size: 20,
            data: json!([{"delta_base": 100.0, "delta_type": "numeric"}, -10.0, 5.0]),
            compression_metadata: HashMap::new(),
        },
        decompression_metadata: DecompressionMetadata {
            strategy: CompressionStrategy::Delta {
                base_values: HashMap::new(),
            },
            dictionary_map: HashMap::new(),
            delta_bases: HashMap::new(),
            priority_hints: HashMap::new(),
        },
    };

    let result = decompressor.decompress_frame(frame).unwrap();
    let arr = result.data.as_array().unwrap();
    assert_eq!(arr[0].as_f64().unwrap(), 90.0); // 100 + (-10)
    assert_eq!(arr[1].as_f64().unwrap(), 105.0); // 100 + 5
}

#[test]
fn test_decompress_delta_missing_base_treated_as_regular_array() {
    let mut decompressor = StreamingDecompressor::new();

    // Array with delta_type but no delta_base is treated as regular array
    let frame = CompressedFrame {
        frame: StreamFrame {
            data: json!([{"delta_type": "numeric"}, 1.0]),
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        },
        compressed_data: CompressedData {
            strategy: CompressionStrategy::Delta {
                base_values: HashMap::new(),
            },
            compressed_size: 20,
            data: json!([{"delta_type": "numeric"}, 1.0]),
            compression_metadata: HashMap::new(),
        },
        decompression_metadata: DecompressionMetadata {
            strategy: CompressionStrategy::Delta {
                base_values: HashMap::new(),
            },
            dictionary_map: HashMap::new(),
            delta_bases: HashMap::new(),
            priority_hints: HashMap::new(),
        },
    };

    // Should succeed, treating as regular array
    let result = decompressor.decompress_frame(frame);
    assert!(result.is_ok());
}

#[test]
fn test_decompress_delta_invalid_delta_value_error() {
    let mut decompressor = StreamingDecompressor::new();

    let frame = CompressedFrame {
        frame: StreamFrame {
            data: json!([{"delta_base": 100.0, "delta_type": "numeric"}, "not_a_number"]),
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        },
        compressed_data: CompressedData {
            strategy: CompressionStrategy::Delta {
                base_values: HashMap::new(),
            },
            compressed_size: 20,
            data: json!([{"delta_base": 100.0, "delta_type": "numeric"}, "not_a_number"]),
            compression_metadata: HashMap::new(),
        },
        decompression_metadata: DecompressionMetadata {
            strategy: CompressionStrategy::Delta {
                base_values: HashMap::new(),
            },
            dictionary_map: HashMap::new(),
            delta_bases: HashMap::new(),
            priority_hints: HashMap::new(),
        },
    };

    let result = decompressor.decompress_frame(frame);
    assert!(result.is_err());
}

// ============================================================================
// Decompression RLE tests via frame decompression
// ============================================================================

#[test]
fn test_decompress_rle_malformed_rle_value_only() {
    let mut decompressor = StreamingDecompressor::new();

    let frame = CompressedFrame {
        frame: StreamFrame {
            data: json!([{"rle_value": "x"}]),
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        },
        compressed_data: CompressedData {
            strategy: CompressionStrategy::RunLength,
            compressed_size: 10,
            data: json!([{"rle_value": "x"}]),
            compression_metadata: HashMap::new(),
        },
        decompression_metadata: DecompressionMetadata {
            strategy: CompressionStrategy::RunLength,
            dictionary_map: HashMap::new(),
            delta_bases: HashMap::new(),
            priority_hints: HashMap::new(),
        },
    };

    let result = decompressor.decompress_frame(frame);
    assert!(result.is_err());
}

#[test]
fn test_decompress_rle_malformed_count_only() {
    let mut decompressor = StreamingDecompressor::new();

    let frame = CompressedFrame {
        frame: StreamFrame {
            data: json!([{"rle_count": 5}]),
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        },
        compressed_data: CompressedData {
            strategy: CompressionStrategy::RunLength,
            compressed_size: 10,
            data: json!([{"rle_count": 5}]),
            compression_metadata: HashMap::new(),
        },
        decompression_metadata: DecompressionMetadata {
            strategy: CompressionStrategy::RunLength,
            dictionary_map: HashMap::new(),
            delta_bases: HashMap::new(),
            priority_hints: HashMap::new(),
        },
    };

    let result = decompressor.decompress_frame(frame);
    assert!(result.is_err());
}

#[test]
fn test_decompress_rle_invalid_count_type() {
    let mut decompressor = StreamingDecompressor::new();

    let frame = CompressedFrame {
        frame: StreamFrame {
            data: json!([{"rle_value": "x", "rle_count": "five"}]),
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        },
        compressed_data: CompressedData {
            strategy: CompressionStrategy::RunLength,
            compressed_size: 10,
            data: json!([{"rle_value": "x", "rle_count": "five"}]),
            compression_metadata: HashMap::new(),
        },
        decompression_metadata: DecompressionMetadata {
            strategy: CompressionStrategy::RunLength,
            dictionary_map: HashMap::new(),
            delta_bases: HashMap::new(),
            priority_hints: HashMap::new(),
        },
    };

    let result = decompressor.decompress_frame(frame);
    assert!(result.is_err());
}

#[test]
fn test_decompress_rle_exceeds_max_count() {
    let mut decompressor = StreamingDecompressor::new();

    let frame = CompressedFrame {
        frame: StreamFrame {
            data: json!([{"rle_value": "x", "rle_count": 100_001}]),
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        },
        compressed_data: CompressedData {
            strategy: CompressionStrategy::RunLength,
            compressed_size: 10,
            data: json!([{"rle_value": "x", "rle_count": 100_001}]),
            compression_metadata: HashMap::new(),
        },
        decompression_metadata: DecompressionMetadata {
            strategy: CompressionStrategy::RunLength,
            dictionary_map: HashMap::new(),
            delta_bases: HashMap::new(),
            priority_hints: HashMap::new(),
        },
    };

    let result = decompressor.decompress_frame(frame);
    assert!(result.is_err());
}

#[test]
fn test_decompress_rle_total_size_exceeds_limit() {
    let mut decompressor = StreamingDecompressor::new();

    let frame = CompressedFrame {
        frame: StreamFrame {
            data: json!([
                {"rle_value": "x", "rle_count": 5_000_000},
                {"rle_value": "y", "rle_count": 6_000_000}
            ]),
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        },
        compressed_data: CompressedData {
            strategy: CompressionStrategy::RunLength,
            compressed_size: 20,
            data: json!([
                {"rle_value": "x", "rle_count": 5_000_000},
                {"rle_value": "y", "rle_count": 6_000_000}
            ]),
            compression_metadata: HashMap::new(),
        },
        decompression_metadata: DecompressionMetadata {
            strategy: CompressionStrategy::RunLength,
            dictionary_map: HashMap::new(),
            delta_bases: HashMap::new(),
            priority_hints: HashMap::new(),
        },
    };

    let result = decompressor.decompress_frame(frame);
    assert!(result.is_err());
}

#[test]
fn test_decompress_rle_zero_count() {
    let mut decompressor = StreamingDecompressor::new();

    let frame = CompressedFrame {
        frame: StreamFrame {
            data: json!([{"rle_value": "x", "rle_count": 0}]),
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        },
        compressed_data: CompressedData {
            strategy: CompressionStrategy::RunLength,
            compressed_size: 10,
            data: json!([{"rle_value": "x", "rle_count": 0}]),
            compression_metadata: HashMap::new(),
        },
        decompression_metadata: DecompressionMetadata {
            strategy: CompressionStrategy::RunLength,
            dictionary_map: HashMap::new(),
            delta_bases: HashMap::new(),
            priority_hints: HashMap::new(),
        },
    };

    let result = decompressor.decompress_frame(frame).unwrap();
    let arr = result.data.as_array().unwrap();
    assert_eq!(arr.len(), 0);
}

// ============================================================================
// Compression tests
// ============================================================================

#[test]
fn test_compress_frame_critical_priority() {
    let mut compressor = StreamingCompressor::new();

    let frame = StreamFrame {
        data: json!({"critical": "data"}),
        priority: Priority::CRITICAL,
        metadata: HashMap::new(),
    };

    let result = compressor.compress_frame(frame);
    assert!(result.is_ok());
    assert_eq!(compressor.get_stats().frames_processed, 1);
}

#[test]
fn test_compress_frame_high_priority() {
    let mut compressor = StreamingCompressor::new();

    let frame = StreamFrame {
        data: json!({"high": "priority"}),
        priority: Priority::HIGH,
        metadata: HashMap::new(),
    };

    let result = compressor.compress_frame(frame);
    assert!(result.is_ok());
}

#[test]
fn test_compress_frame_low_priority() {
    let mut compressor = StreamingCompressor::new();

    let frame = StreamFrame {
        data: json!({"low": "priority"}),
        priority: Priority::LOW,
        metadata: HashMap::new(),
    };

    let result = compressor.compress_frame(frame);
    assert!(result.is_ok());
}

#[test]
fn test_compress_frame_multiple_frames() {
    let mut compressor = StreamingCompressor::new();

    for i in 0..5 {
        let frame = StreamFrame {
            data: json!({"frame": i}),
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        };
        compressor.compress_frame(frame).unwrap();
    }

    let stats = compressor.get_stats();
    assert_eq!(stats.frames_processed, 5);
}

#[test]
fn test_optimize_for_data_with_samples() {
    let mut compressor = StreamingCompressor::new();

    let skeleton = json!({"type": "user", "version": 1});
    let samples = vec![
        json!({"name": "Alice", "age": 30}),
        json!({"name": "Bob", "age": 25}),
    ];

    let result = compressor.optimize_for_data(&skeleton, &samples);
    assert!(result.is_ok());
}

#[test]
fn test_reset_stats() {
    let mut compressor = StreamingCompressor::new();

    let frame = StreamFrame {
        data: json!({"test": "data"}),
        priority: Priority::MEDIUM,
        metadata: HashMap::new(),
    };

    compressor.compress_frame(frame).unwrap();
    assert!(compressor.get_stats().frames_processed > 0);

    compressor.reset_stats();
    assert_eq!(compressor.get_stats().frames_processed, 0);
}

#[test]
fn test_compressor_with_custom_strategies() {
    let dict = HashMap::new();
    let skeleton_strategy = CompressionStrategy::Dictionary {
        dictionary: dict.clone(),
    };
    let content_strategy = CompressionStrategy::Delta {
        base_values: HashMap::new(),
    };

    let mut compressor = StreamingCompressor::with_strategies(skeleton_strategy, content_strategy);

    let frame = StreamFrame {
        data: json!({"test": "data"}),
        priority: Priority::HIGH,
        metadata: HashMap::new(),
    };

    let result = compressor.compress_frame(frame);
    assert!(result.is_ok());
}

// ============================================================================
// Decompressor stats tests
// ============================================================================

#[test]
fn test_decompressor_stats_increments() {
    let mut decompressor = StreamingDecompressor::new();

    assert_eq!(decompressor.get_stats().frames_decompressed, 0);

    let frame = CompressedFrame {
        frame: StreamFrame {
            data: json!({"key": "value"}),
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        },
        compressed_data: CompressedData {
            strategy: CompressionStrategy::None,
            compressed_size: 15,
            data: json!({"key": "value"}),
            compression_metadata: HashMap::new(),
        },
        decompression_metadata: DecompressionMetadata {
            strategy: CompressionStrategy::None,
            dictionary_map: HashMap::new(),
            delta_bases: HashMap::new(),
            priority_hints: HashMap::new(),
        },
    };

    decompressor.decompress_frame(frame).unwrap();

    let stats = decompressor.get_stats();
    assert_eq!(stats.frames_decompressed, 1);
    assert!(stats.total_decompressed_bytes > 0);
}

#[test]
fn test_decompressor_running_average() {
    let mut decompressor = StreamingDecompressor::new();

    for i in 0..3 {
        let frame = CompressedFrame {
            frame: StreamFrame {
                data: json!({"frame": i}),
                priority: Priority::MEDIUM,
                metadata: HashMap::new(),
            },
            compressed_data: CompressedData {
                strategy: CompressionStrategy::None,
                compressed_size: 15,
                data: json!({"frame": i}),
                compression_metadata: HashMap::new(),
            },
            decompression_metadata: DecompressionMetadata {
                strategy: CompressionStrategy::None,
                dictionary_map: HashMap::new(),
                delta_bases: HashMap::new(),
                priority_hints: HashMap::new(),
            },
        };

        decompressor.decompress_frame(frame).unwrap();
    }

    let stats = decompressor.get_stats();
    assert_eq!(stats.frames_decompressed, 3);
}

// ============================================================================
// Default trait tests
// ============================================================================

#[test]
fn test_compressor_default() {
    let compressor = StreamingCompressor::default();
    assert_eq!(compressor.get_stats().frames_processed, 0);
}

#[test]
fn test_decompressor_default() {
    let decompressor = StreamingDecompressor::default();
    assert_eq!(decompressor.get_stats().frames_decompressed, 0);
}

// ============================================================================
// Direct testing of public decompress_delta and decompress_run_length
// ============================================================================

#[test]
fn test_decompress_delta_empty_array() {
    let decompressor = StreamingDecompressor::new();
    let result = decompressor.decompress_delta(&json!([]));
    assert_eq!(result.unwrap(), json!([]));
}

#[test]
fn test_decompress_delta_non_delta_array() {
    let decompressor = StreamingDecompressor::new();
    let result = decompressor.decompress_delta(&json!([1, 2, 3]));
    assert_eq!(result.unwrap(), json!([1, 2, 3]));
}

#[test]
fn test_decompress_delta_nested_objects() {
    let decompressor = StreamingDecompressor::new();
    let data = json!({"outer": {"inner": 123}});
    let result = decompressor.decompress_delta(&data);
    assert_eq!(result.unwrap(), data);
}

#[test]
fn test_decompress_delta_valid_delta_array() {
    let decompressor = StreamingDecompressor::new();
    let data = json!([
        {"delta_base": 100.0, "delta_type": "numeric"},
        1.0,
        2.0,
        3.0
    ]);
    let result = decompressor.decompress_delta(&data).unwrap();
    let arr = result.as_array().unwrap();
    assert_eq!(arr[0].as_f64().unwrap(), 101.0);
    assert_eq!(arr[1].as_f64().unwrap(), 102.0);
    assert_eq!(arr[2].as_f64().unwrap(), 103.0);
}

#[test]
fn test_decompress_delta_negative_deltas() {
    let decompressor = StreamingDecompressor::new();
    let data = json!([
        {"delta_base": 100.0, "delta_type": "numeric"},
        -10.0,
        -5.0,
        5.0
    ]);
    let result = decompressor.decompress_delta(&data).unwrap();
    let arr = result.as_array().unwrap();
    assert_eq!(arr[0].as_f64().unwrap(), 90.0);
    assert_eq!(arr[1].as_f64().unwrap(), 95.0);
    assert_eq!(arr[2].as_f64().unwrap(), 105.0);
}

#[test]
fn test_decompress_delta_missing_delta_base() {
    let decompressor = StreamingDecompressor::new();
    let data = json!([
        {"delta_type": "numeric"},
        1.0
    ]);
    let result = decompressor.decompress_delta(&data);
    // Should be treated as regular array, not error
    assert!(result.is_ok());
}

#[test]
fn test_decompress_delta_invalid_delta_value() {
    let decompressor = StreamingDecompressor::new();
    let data = json!([
        {"delta_base": 100.0, "delta_type": "numeric"},
        "not_a_number"
    ]);
    let result = decompressor.decompress_delta(&data);
    assert!(result.is_err());
}

#[test]
fn test_decompress_delta_string_values() {
    let decompressor = StreamingDecompressor::new();
    let data = json!("string value");
    let result = decompressor.decompress_delta(&data);
    assert_eq!(result.unwrap(), json!("string value"));
}

#[test]
fn test_decompress_delta_number_values() {
    let decompressor = StreamingDecompressor::new();
    let data = json!(42);
    let result = decompressor.decompress_delta(&data);
    assert_eq!(result.unwrap(), json!(42));
}

#[test]
fn test_decompress_delta_bool_values() {
    let decompressor = StreamingDecompressor::new();
    assert_eq!(
        decompressor.decompress_delta(&json!(true)).unwrap(),
        json!(true)
    );
    assert_eq!(
        decompressor.decompress_delta(&json!(false)).unwrap(),
        json!(false)
    );
}

#[test]
fn test_decompress_delta_null() {
    let decompressor = StreamingDecompressor::new();
    let result = decompressor.decompress_delta(&json!(null));
    assert_eq!(result.unwrap(), json!(null));
}

// Run-length decompress tests
#[test]
fn test_decompress_run_length_empty_array() {
    let decompressor = StreamingDecompressor::new();
    let result = decompressor.decompress_run_length(&json!([]));
    assert_eq!(result.unwrap(), json!([]));
}

#[test]
fn test_decompress_run_length_no_rle_objects() {
    let decompressor = StreamingDecompressor::new();
    let result = decompressor.decompress_run_length(&json!([1, 2, 3]));
    assert_eq!(result.unwrap(), json!([1, 2, 3]));
}

#[test]
fn test_decompress_run_length_simple_expansion() {
    let decompressor = StreamingDecompressor::new();
    let data = json!([{"rle_value": "x", "rle_count": 5}]);
    let result = decompressor.decompress_run_length(&data).unwrap();
    let arr = result.as_array().unwrap();
    assert_eq!(arr.len(), 5);
    for item in arr {
        assert_eq!(item.as_str().unwrap(), "x");
    }
}

#[test]
fn test_decompress_run_length_mixed_content() {
    let decompressor = StreamingDecompressor::new();
    let data = json!([
        {"rle_value": "a", "rle_count": 2},
        "regular",
        {"rle_value": "b", "rle_count": 3}
    ]);
    let result = decompressor.decompress_run_length(&data).unwrap();
    let arr = result.as_array().unwrap();
    assert_eq!(arr.len(), 6); // 2 + 1 + 3
}

#[test]
fn test_decompress_run_length_zero_count() {
    let decompressor = StreamingDecompressor::new();
    let data = json!([{"rle_value": "x", "rle_count": 0}]);
    let result = decompressor.decompress_run_length(&data).unwrap();
    let arr = result.as_array().unwrap();
    assert_eq!(arr.len(), 0);
}

#[test]
fn test_decompress_run_length_nested_objects() {
    let decompressor = StreamingDecompressor::new();
    let data = json!({
        "outer": [{"rle_value": 1, "rle_count": 3}]
    });
    let result = decompressor.decompress_run_length(&data).unwrap();
    let obj = result.as_object().unwrap();
    let arr = obj.get("outer").unwrap().as_array().unwrap();
    assert_eq!(arr.len(), 3);
}

#[test]
fn test_decompress_run_length_preserves_non_rle_objects() {
    let decompressor = StreamingDecompressor::new();
    let data = json!([{"other": "data"}, 1, "string"]);
    let result = decompressor.decompress_run_length(&data).unwrap();
    let arr = result.as_array().unwrap();
    assert_eq!(arr.len(), 3);
}

#[test]
fn test_decompress_run_length_string_value() {
    let decompressor = StreamingDecompressor::new();
    let result = decompressor.decompress_run_length(&json!("text"));
    assert_eq!(result.unwrap(), json!("text"));
}

#[test]
fn test_decompress_run_length_number_value() {
    let decompressor = StreamingDecompressor::new();
    let result = decompressor.decompress_run_length(&json!(123));
    assert_eq!(result.unwrap(), json!(123));
}

#[test]
fn test_decompress_run_length_bool_values() {
    let decompressor = StreamingDecompressor::new();
    assert_eq!(
        decompressor.decompress_run_length(&json!(true)).unwrap(),
        json!(true)
    );
    assert_eq!(
        decompressor.decompress_run_length(&json!(false)).unwrap(),
        json!(false)
    );
}

#[test]
fn test_decompress_run_length_null() {
    let decompressor = StreamingDecompressor::new();
    let result = decompressor.decompress_run_length(&json!(null));
    assert_eq!(result.unwrap(), json!(null));
}

#[test]
fn test_decompress_run_length_large_count_within_limit() {
    let decompressor = StreamingDecompressor::new();
    let data = json!([{"rle_value": "x", "rle_count": 1000}]);
    let result = decompressor.decompress_run_length(&data).unwrap();
    let arr = result.as_array().unwrap();
    assert_eq!(arr.len(), 1000);
}

#[test]
fn test_decompress_run_length_number_rle() {
    let decompressor = StreamingDecompressor::new();
    let data = json!([{"rle_value": 42, "rle_count": 3}]);
    let result = decompressor.decompress_run_length(&data).unwrap();
    let arr = result.as_array().unwrap();
    assert_eq!(arr.len(), 3);
    for item in arr {
        assert_eq!(item.as_i64().unwrap(), 42);
    }
}

#[test]
fn test_decompress_run_length_object_rle() {
    let decompressor = StreamingDecompressor::new();
    let data = json!([{"rle_value": {"key": "val"}, "rle_count": 2}]);
    let result = decompressor.decompress_run_length(&data).unwrap();
    let arr = result.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0].get("key").unwrap().as_str().unwrap(), "val");
}

#[test]
fn test_decompress_run_length_array_rle() {
    let decompressor = StreamingDecompressor::new();
    let data = json!([{"rle_value": [1, 2, 3], "rle_count": 2}]);
    let result = decompressor.decompress_run_length(&data).unwrap();
    let arr = result.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0].as_array().unwrap().len(), 3);
}

// ============================================================================
// Additional compressor tests
// ============================================================================

#[test]
fn test_compress_frame_with_background_priority() {
    let mut compressor = StreamingCompressor::new();
    let frame = StreamFrame {
        data: json!({"bg": "task"}),
        priority: Priority::BACKGROUND,
        metadata: HashMap::new(),
    };
    let result = compressor.compress_frame(frame);
    assert!(result.is_ok());
}

#[test]
fn test_compress_frame_with_empty_object() {
    let mut compressor = StreamingCompressor::new();
    let frame = StreamFrame {
        data: json!({}),
        priority: Priority::MEDIUM,
        metadata: HashMap::new(),
    };
    let result = compressor.compress_frame(frame);
    assert!(result.is_ok());
}

#[test]
fn test_compress_frame_with_empty_array() {
    let mut compressor = StreamingCompressor::new();
    let frame = StreamFrame {
        data: json!([]),
        priority: Priority::MEDIUM,
        metadata: HashMap::new(),
    };
    let result = compressor.compress_frame(frame);
    assert!(result.is_ok());
}

#[test]
fn test_compress_frame_with_nested_data() {
    let mut compressor = StreamingCompressor::new();
    let frame = StreamFrame {
        data: json!({
            "level1": {
                "level2": {
                    "level3": "deep"
                }
            }
        }),
        priority: Priority::MEDIUM,
        metadata: HashMap::new(),
    };
    let result = compressor.compress_frame(frame);
    assert!(result.is_ok());
}

#[test]
fn test_compress_frame_with_large_array() {
    let mut compressor = StreamingCompressor::new();
    let large_array: Vec<i32> = (0..100).collect();
    let frame = StreamFrame {
        data: json!(large_array),
        priority: Priority::LOW,
        metadata: HashMap::new(),
    };
    let result = compressor.compress_frame(frame);
    assert!(result.is_ok());
}

// ============================================================================
// Stats methods tests
// ============================================================================

#[test]
fn test_compression_stats_overall_ratio() {
    let stats = pjson_rs::stream::compression_integration::CompressionStats {
        total_input_bytes: 1000,
        total_output_bytes: 500,
        frames_processed: 1,
        priority_ratios: HashMap::new(),
    };
    assert_eq!(stats.overall_compression_ratio(), 0.5);
}

#[test]
fn test_compression_stats_priority_ratio_default() {
    let stats = pjson_rs::stream::compression_integration::CompressionStats {
        total_input_bytes: 100,
        total_output_bytes: 50,
        frames_processed: 1,
        priority_ratios: HashMap::new(),
    };
    assert_eq!(stats.priority_compression_ratio(255), 1.0);
}

#[test]
fn test_compression_stats_bytes_saved_positive() {
    let stats = pjson_rs::stream::compression_integration::CompressionStats {
        total_input_bytes: 1000,
        total_output_bytes: 600,
        frames_processed: 1,
        priority_ratios: HashMap::new(),
    };
    assert_eq!(stats.bytes_saved(), 400);
}

#[test]
fn test_compression_stats_percentage_saved() {
    let stats = pjson_rs::stream::compression_integration::CompressionStats {
        total_input_bytes: 1000,
        total_output_bytes: 700,
        frames_processed: 1,
        priority_ratios: HashMap::new(),
    };
    let percentage = stats.percentage_saved();
    assert!((percentage - 30.0).abs() < 0.001);
}

// ============================================================================
// Deep recursion tests for decompress_delta
// ============================================================================

#[test]
fn test_decompress_delta_deeply_nested_objects() {
    let decompressor = StreamingDecompressor::new();
    let data = json!({
        "l1": {
            "l2": {
                "l3": {
                    "l4": {
                        "value": 42
                    }
                }
            }
        }
    });
    let result = decompressor.decompress_delta(&data).unwrap();
    assert_eq!(
        result
            .get("l1")
            .unwrap()
            .get("l2")
            .unwrap()
            .get("l3")
            .unwrap()
            .get("l4")
            .unwrap()
            .get("value")
            .unwrap()
            .as_i64()
            .unwrap(),
        42
    );
}

#[test]
fn test_decompress_delta_deeply_nested_arrays() {
    let decompressor = StreamingDecompressor::new();
    let data = json!([[[[1, 2, 3]]]]);
    let result = decompressor.decompress_delta(&data).unwrap();
    assert_eq!(result[0][0][0].as_array().unwrap().len(), 3);
}

#[test]
fn test_decompress_delta_mixed_nested_structures() {
    let decompressor = StreamingDecompressor::new();
    let data = json!({
        "array": [
            {"nested": [1, 2]},
            [{"more_nesting": "value"}]
        ]
    });
    let result = decompressor.decompress_delta(&data);
    assert!(result.is_ok());
}

#[test]
fn test_decompress_delta_array_with_objects_recursive() {
    let decompressor = StreamingDecompressor::new();
    let data = json!([
        {"a": 1},
        {"b": 2},
        {"c": [3, 4, 5]}
    ]);
    let result = decompressor.decompress_delta(&data).unwrap();
    let arr = result.as_array().unwrap();
    assert_eq!(arr.len(), 3);
}

#[test]
fn test_decompress_delta_object_with_array_values() {
    let decompressor = StreamingDecompressor::new();
    let data = json!({
        "arrays": [
            [1, 2],
            [3, 4],
            [5, 6]
        ]
    });
    let result = decompressor.decompress_delta(&data).unwrap();
    let arr = result.get("arrays").unwrap().as_array().unwrap();
    assert_eq!(arr.len(), 3);
}

// ============================================================================
// Deep recursion tests for decompress_run_length
// ============================================================================

#[test]
fn test_decompress_run_length_deeply_nested_objects() {
    let decompressor = StreamingDecompressor::new();
    let data = json!({
        "l1": {
            "l2": {
                "l3": [
                    {"rle_value": "x", "rle_count": 2}
                ]
            }
        }
    });
    let result = decompressor.decompress_run_length(&data).unwrap();
    let arr = result
        .get("l1")
        .unwrap()
        .get("l2")
        .unwrap()
        .get("l3")
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(arr.len(), 2);
}

#[test]
fn test_decompress_run_length_arrays_in_arrays() {
    let decompressor = StreamingDecompressor::new();
    let data = json!([
        [{"rle_value": 1, "rle_count": 2}],
        [{"rle_value": 2, "rle_count": 3}]
    ]);
    let result = decompressor.decompress_run_length(&data).unwrap();
    let arr = result.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0].as_array().unwrap().len(), 2);
    assert_eq!(arr[1].as_array().unwrap().len(), 3);
}

#[test]
fn test_decompress_run_length_complex_nested_structure() {
    let decompressor = StreamingDecompressor::new();
    let data = json!({
        "top": [
            {
                "nested": [
                    {"rle_value": "a", "rle_count": 1}
                ]
            },
            [{"rle_value": "b", "rle_count": 2}]
        ]
    });
    let result = decompressor.decompress_run_length(&data);
    assert!(result.is_ok());
}

// ============================================================================
// Mixed testing - combining decompress_delta recursive paths
// ============================================================================

#[test]
fn test_decompress_delta_array_of_mixed_types() {
    let decompressor = StreamingDecompressor::new();
    let data = json!([null, true, 42, "str", {"obj": "val"}, [1, 2]]);
    let result = decompressor.decompress_delta(&data).unwrap();
    let arr = result.as_array().unwrap();
    assert_eq!(arr.len(), 6);
}

#[test]
fn test_decompress_delta_object_with_mixed_value_types() {
    let decompressor = StreamingDecompressor::new();
    let data = json!({
        "null_val": null,
        "bool_val": true,
        "num_val": 123,
        "str_val": "text",
        "arr_val": [1, 2],
        "obj_val": {"nested": "data"}
    });
    let result = decompressor.decompress_delta(&data).unwrap();
    assert!(result.is_object());
}

// ============================================================================
// Boundary tests for RLE
// ============================================================================

#[test]
fn test_decompress_run_length_single_element() {
    let decompressor = StreamingDecompressor::new();
    let data = json!([{"rle_value": "x", "rle_count": 1}]);
    let result = decompressor.decompress_run_length(&data).unwrap();
    let arr = result.as_array().unwrap();
    assert_eq!(arr.len(), 1);
}

#[test]
fn test_decompress_run_length_max_safe_count() {
    let decompressor = StreamingDecompressor::new();
    // Just under the MAX_RLE_COUNT limit (100_000)
    let data = json!([{"rle_value": "x", "rle_count": 100_000}]);
    let result = decompressor.decompress_run_length(&data).unwrap();
    let arr = result.as_array().unwrap();
    assert_eq!(arr.len(), 100_000);
}

#[test]
fn test_decompress_run_length_multiple_small_runs() {
    let decompressor = StreamingDecompressor::new();
    let data = json!([
        {"rle_value": 1, "rle_count": 10},
        {"rle_value": 2, "rle_count": 10},
        {"rle_value": 3, "rle_count": 10}
    ]);
    let result = decompressor.decompress_run_length(&data).unwrap();
    let arr = result.as_array().unwrap();
    assert_eq!(arr.len(), 30);
}

// ============================================================================
// Delta array edge cases
// ============================================================================

#[test]
fn test_decompress_delta_array_single_element() {
    let decompressor = StreamingDecompressor::new();
    let data = json!([
        {"delta_base": 10.0, "delta_type": "numeric"},
        5.0
    ]);
    let result = decompressor.decompress_delta(&data).unwrap();
    let arr = result.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0].as_f64().unwrap(), 15.0);
}

#[test]
fn test_decompress_delta_array_zero_deltas() {
    let decompressor = StreamingDecompressor::new();
    let data = json!([
        {"delta_base": 100.0, "delta_type": "numeric"},
        0.0,
        0.0,
        0.0
    ]);
    let result = decompressor.decompress_delta(&data).unwrap();
    let arr = result.as_array().unwrap();
    for item in arr {
        assert_eq!(item.as_f64().unwrap(), 100.0);
    }
}

#[test]
fn test_decompress_delta_array_large_positive_deltas() {
    let decompressor = StreamingDecompressor::new();
    let data = json!([
        {"delta_base": 0.0, "delta_type": "numeric"},
        1000.0,
        2000.0,
        3000.0
    ]);
    let result = decompressor.decompress_delta(&data).unwrap();
    let arr = result.as_array().unwrap();
    assert_eq!(arr[0].as_f64().unwrap(), 1000.0);
    assert_eq!(arr[1].as_f64().unwrap(), 2000.0);
    assert_eq!(arr[2].as_f64().unwrap(), 3000.0);
}

#[test]
fn test_decompress_delta_array_fractional_deltas() {
    let decompressor = StreamingDecompressor::new();
    let data = json!([
        {"delta_base": 10.5, "delta_type": "numeric"},
        0.1,
        0.2,
        0.3
    ]);
    let result = decompressor.decompress_delta(&data).unwrap();
    let arr = result.as_array().unwrap();
    assert!((arr[0].as_f64().unwrap() - 10.6).abs() < 0.0001);
    assert!((arr[1].as_f64().unwrap() - 10.7).abs() < 0.0001);
    assert!((arr[2].as_f64().unwrap() - 10.8).abs() < 0.0001);
}

// ============================================================================
// Metadata update tests via frames
// ============================================================================

#[test]
fn test_metadata_update_with_dictionary() {
    let mut decompressor = StreamingDecompressor::new();

    let mut dict_map = HashMap::new();
    dict_map.insert(0, "first".to_string());
    dict_map.insert(1, "second".to_string());
    dict_map.insert(2, "third".to_string());

    let frame = CompressedFrame {
        frame: StreamFrame {
            data: json!({"data": "test"}),
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        },
        compressed_data: CompressedData {
            strategy: CompressionStrategy::Dictionary {
                dictionary: HashMap::new(),
            },
            compressed_size: 10,
            data: json!({"data": "test"}),
            compression_metadata: HashMap::new(),
        },
        decompression_metadata: DecompressionMetadata {
            strategy: CompressionStrategy::Dictionary {
                dictionary: HashMap::new(),
            },
            dictionary_map: dict_map,
            delta_bases: HashMap::new(),
            priority_hints: HashMap::new(),
        },
    };

    // Dictionary should be updated during decompression
    let result = decompressor.decompress_frame(frame);
    assert!(result.is_ok());
}

#[test]
fn test_metadata_update_with_delta_bases() {
    let mut decompressor = StreamingDecompressor::new();

    let mut delta_bases = HashMap::new();
    delta_bases.insert("path1".to_string(), 100.0);
    delta_bases.insert("path2".to_string(), 200.0);

    let frame = CompressedFrame {
        frame: StreamFrame {
            data: json!({"data": "test"}),
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        },
        compressed_data: CompressedData {
            strategy: CompressionStrategy::Delta {
                base_values: HashMap::new(),
            },
            compressed_size: 10,
            data: json!({"data": "test"}),
            compression_metadata: HashMap::new(),
        },
        decompression_metadata: DecompressionMetadata {
            strategy: CompressionStrategy::Delta {
                base_values: HashMap::new(),
            },
            dictionary_map: HashMap::new(),
            delta_bases,
            priority_hints: HashMap::new(),
        },
    };

    // Delta bases should be updated
    let result = decompressor.decompress_frame(frame);
    assert!(result.is_ok());
}

// ============================================================================
// Additional compressor tests for coverage
// ============================================================================

#[test]
fn test_compress_frame_preserves_metadata() {
    let mut compressor = StreamingCompressor::new();

    let mut metadata = HashMap::new();
    metadata.insert("key".to_string(), "value".to_string());

    let frame = StreamFrame {
        data: json!({"test": "data"}),
        priority: Priority::MEDIUM,
        metadata: metadata.clone(),
    };

    let compressed = compressor.compress_frame(frame).unwrap();
    assert_eq!(compressed.frame.metadata, metadata);
}

#[test]
fn test_compress_frame_preserves_priority() {
    let mut compressor = StreamingCompressor::new();

    let frame = StreamFrame {
        data: json!({"test": "data"}),
        priority: Priority::CRITICAL,
        metadata: HashMap::new(),
    };

    let compressed = compressor.compress_frame(frame).unwrap();
    assert_eq!(compressed.frame.priority, Priority::CRITICAL);
}

#[test]
fn test_optimize_for_data_with_complex_skeleton() {
    let mut compressor = StreamingCompressor::new();

    let skeleton = json!({
        "type": "complex",
        "version": 2,
        "schema": {
            "fields": ["a", "b", "c"]
        }
    });
    let samples = vec![
        json!({"name": "test1", "value": 100}),
        json!({"name": "test2", "value": 200}),
    ];

    let result = compressor.optimize_for_data(&skeleton, &samples);
    assert!(result.is_ok());
}

#[test]
fn test_compression_stats_with_different_priorities() {
    let mut compressor = StreamingCompressor::new();

    let priorities = vec![
        Priority::CRITICAL,
        Priority::HIGH,
        Priority::MEDIUM,
        Priority::LOW,
        Priority::BACKGROUND,
    ];

    for priority in priorities {
        let frame = StreamFrame {
            data: json!({"test": "data"}),
            priority,
            metadata: HashMap::new(),
        };
        compressor.compress_frame(frame).unwrap();
    }

    let stats = compressor.get_stats();
    assert_eq!(stats.frames_processed, 5);
}

#[test]
fn test_decompress_frame_preserves_original_frame_data() {
    let mut decompressor = StreamingDecompressor::new();

    let original_data = json!({"original": "data"});

    let frame = CompressedFrame {
        frame: StreamFrame {
            data: original_data.clone(),
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        },
        compressed_data: CompressedData {
            strategy: CompressionStrategy::None,
            compressed_size: 20,
            data: original_data.clone(),
            compression_metadata: HashMap::new(),
        },
        decompression_metadata: DecompressionMetadata {
            strategy: CompressionStrategy::None,
            dictionary_map: HashMap::new(),
            delta_bases: HashMap::new(),
            priority_hints: HashMap::new(),
        },
    };

    let result = decompressor.decompress_frame(frame).unwrap();
    assert_eq!(result.data, original_data);
}
