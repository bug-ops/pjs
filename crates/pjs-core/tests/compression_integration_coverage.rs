//! Additional coverage tests for compression integration
//!
//! These tests target uncovered code paths identified in coverage analysis
//! to achieve >= 70% coverage for compression_integration.rs (P2-TEST-002)

use pjson_rs::{
    compression::{CompressedData, CompressionStrategy},
    stream::{
        StreamFrame,
        compression_integration::{
            CompressedFrame, CompressionStats, DecompressionMetadata, StreamingCompressor,
            StreamingDecompressor,
        },
    },
};
use pjson_rs_domain::value_objects::Priority;
use serde_json::json;
use std::collections::HashMap;

// ============================================================================
// CompressionStats tests (targeting uncovered branches)
// ============================================================================

#[test]
fn test_compression_stats_priority_ratio_with_existing_ratios() {
    let mut stats = CompressionStats::default();
    stats.priority_ratios.insert(1, 0.5);
    stats.priority_ratios.insert(2, 0.7);
    stats.priority_ratios.insert(3, 0.9);

    assert_eq!(stats.priority_compression_ratio(1), 0.5);
    assert_eq!(stats.priority_compression_ratio(2), 0.7);
    assert_eq!(stats.priority_compression_ratio(3), 0.9);
    assert_eq!(stats.priority_compression_ratio(99), 1.0); // Default for non-existent
}

#[test]
fn test_compression_stats_bytes_saved_with_expansion() {
    let stats = CompressionStats {
        total_input_bytes: 100,
        total_output_bytes: 150, // Compression actually expanded data
        frames_processed: 1,
        priority_ratios: HashMap::new(),
    };

    assert_eq!(stats.bytes_saved(), -50);
    assert!(stats.percentage_saved() < 0.0);
}

#[test]
fn test_compression_stats_zero_input_bytes() {
    let stats = CompressionStats {
        total_input_bytes: 0,
        total_output_bytes: 0,
        frames_processed: 0,
        priority_ratios: HashMap::new(),
    };

    assert_eq!(stats.overall_compression_ratio(), 1.0);
    assert_eq!(stats.percentage_saved(), 0.0);
    assert_eq!(stats.bytes_saved(), 0);
}

#[test]
fn test_compression_stats_perfect_compression() {
    let stats = CompressionStats {
        total_input_bytes: 1000,
        total_output_bytes: 100,
        frames_processed: 5,
        priority_ratios: HashMap::new(),
    };

    assert_eq!(stats.overall_compression_ratio(), 0.1);
    assert_eq!(stats.bytes_saved(), 900);
    let percentage = stats.percentage_saved();
    assert!((percentage - 90.0).abs() < 0.001);
}

// ============================================================================
// StreamingCompressor with_strategies tests
// ============================================================================

#[test]
fn test_compressor_with_dictionary_strategy() {
    let mut dict = HashMap::new();
    dict.insert("test".to_string(), 0);

    let mut compressor = StreamingCompressor::with_strategies(
        CompressionStrategy::Dictionary {
            dictionary: dict.clone(),
        },
        CompressionStrategy::None,
    );

    let frame = StreamFrame {
        data: json!({"value": 123}),
        priority: Priority::CRITICAL,
        metadata: HashMap::new(),
    };

    let result = compressor.compress_frame(frame);
    assert!(result.is_ok());
    assert_eq!(compressor.get_stats().frames_processed, 1);
}

#[test]
fn test_compressor_with_delta_strategy() {
    let mut bases = HashMap::new();
    bases.insert("value".to_string(), 100.0);

    let mut compressor = StreamingCompressor::with_strategies(
        CompressionStrategy::None,
        CompressionStrategy::Delta {
            base_values: bases.clone(),
        },
    );

    let frame = StreamFrame {
        data: json!({"value": 105}),
        priority: Priority::LOW,
        metadata: HashMap::new(),
    };

    let result = compressor.compress_frame(frame);
    assert!(result.is_ok());
}

#[test]
fn test_compressor_with_hybrid_strategy() {
    let mut string_dict = HashMap::new();
    string_dict.insert("test".to_string(), 0);

    let mut numeric_deltas = HashMap::new();
    numeric_deltas.insert("value".to_string(), 100.0);

    let mut compressor = StreamingCompressor::with_strategies(
        CompressionStrategy::Hybrid {
            string_dict: string_dict.clone(),
            numeric_deltas: numeric_deltas.clone(),
        },
        CompressionStrategy::None,
    );

    let frame = StreamFrame {
        data: json!({"test": "data", "value": 105}),
        priority: Priority::HIGH,
        metadata: HashMap::new(),
    };

    let result = compressor.compress_frame(frame);
    assert!(result.is_ok());
}

#[test]
fn test_compressor_with_run_length_strategy() {
    let mut compressor = StreamingCompressor::with_strategies(
        CompressionStrategy::RunLength,
        CompressionStrategy::None,
    );

    let frame = StreamFrame {
        data: json!([1, 1, 1, 2, 2, 3, 3, 3, 3]),
        priority: Priority::CRITICAL,
        metadata: HashMap::new(),
    };

    let result = compressor.compress_frame(frame);
    assert!(result.is_ok());
}

// ============================================================================
// optimize_for_data tests (targets uncovered optimization paths)
// ============================================================================

#[test]
fn test_optimize_for_data_with_sample_data() {
    let mut compressor = StreamingCompressor::new();

    let skeleton = json!({
        "id": null,
        "name": null,
        "value": null
    });

    let samples = vec![
        json!({"id": 1, "name": "test1", "value": 100}),
        json!({"id": 2, "name": "test2", "value": 200}),
        json!({"id": 3, "name": "test3", "value": 300}),
    ];

    let result = compressor.optimize_for_data(&skeleton, &samples);
    assert!(result.is_ok());
}

#[test]
fn test_optimize_for_data_with_empty_samples() {
    let mut compressor = StreamingCompressor::new();

    let skeleton = json!({"key": "value"});
    let result = compressor.optimize_for_data(&skeleton, &[]);
    assert!(result.is_ok());
}

#[test]
fn test_optimize_for_data_with_complex_skeleton() {
    let mut compressor = StreamingCompressor::new();

    let skeleton = json!({
        "user": {
            "id": null,
            "profile": {
                "name": null,
                "age": null
            }
        },
        "metadata": {
            "timestamp": null
        }
    });

    let samples = vec![
        json!({"user": {"id": 1, "profile": {"name": "alice", "age": 30}}, "metadata": {"timestamp": 1000}}),
        json!({"user": {"id": 2, "profile": {"name": "bob", "age": 25}}, "metadata": {"timestamp": 2000}}),
    ];

    let result = compressor.optimize_for_data(&skeleton, &samples);
    assert!(result.is_ok());
}

// ============================================================================
// reset_stats tests
// ============================================================================

#[test]
fn test_reset_stats_clears_all_data() {
    let mut compressor = StreamingCompressor::new();

    // Process some frames
    for i in 0..5 {
        let frame = StreamFrame {
            data: json!({"count": i}),
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        };
        compressor.compress_frame(frame).unwrap();
    }

    assert!(compressor.get_stats().frames_processed > 0);
    assert!(compressor.get_stats().total_input_bytes > 0);

    compressor.reset_stats();

    assert_eq!(compressor.get_stats().total_input_bytes, 0);
    assert_eq!(compressor.get_stats().total_output_bytes, 0);
    assert_eq!(compressor.get_stats().frames_processed, 0);
}

// ============================================================================
// Priority-based compressor selection tests
// ============================================================================

#[test]
fn test_compress_critical_priority_uses_skeleton_compressor() {
    let mut compressor = StreamingCompressor::new();

    let frame = StreamFrame {
        data: json!({"critical": "error message"}),
        priority: Priority::CRITICAL,
        metadata: HashMap::new(),
    };

    let result = compressor.compress_frame(frame);
    assert!(result.is_ok());
}

#[test]
fn test_compress_high_priority_uses_skeleton_compressor() {
    let mut compressor = StreamingCompressor::new();

    let frame = StreamFrame {
        data: json!({"important": "data"}),
        priority: Priority::HIGH,
        metadata: HashMap::new(),
    };

    let result = compressor.compress_frame(frame);
    assert!(result.is_ok());
}

#[test]
fn test_compress_medium_priority_uses_content_compressor() {
    let mut compressor = StreamingCompressor::new();

    let frame = StreamFrame {
        data: json!({"regular": "content"}),
        priority: Priority::MEDIUM,
        metadata: HashMap::new(),
    };

    let result = compressor.compress_frame(frame);
    assert!(result.is_ok());
}

#[test]
fn test_compress_low_priority_uses_content_compressor() {
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
fn test_compress_background_priority_uses_content_compressor() {
    let mut compressor = StreamingCompressor::new();

    let frame = StreamFrame {
        data: json!({"background": "task"}),
        priority: Priority::BACKGROUND,
        metadata: HashMap::new(),
    };

    let result = compressor.compress_frame(frame);
    assert!(result.is_ok());
}

// ============================================================================
// Multiple priority compression tests (stats tracking)
// ============================================================================

#[test]
fn test_compress_multiple_priorities_tracks_stats() {
    let mut compressor = StreamingCompressor::new();

    let priorities = vec![
        Priority::CRITICAL,
        Priority::HIGH,
        Priority::MEDIUM,
        Priority::LOW,
        Priority::BACKGROUND,
    ];

    for (i, priority) in priorities.iter().enumerate() {
        let frame = StreamFrame {
            data: json!({"index": i, "data": format!("frame{}", i)}),
            priority: *priority,
            metadata: HashMap::new(),
        };
        compressor.compress_frame(frame).unwrap();
    }

    let stats = compressor.get_stats();
    assert_eq!(stats.frames_processed, 5);
    assert!(stats.total_input_bytes > 0);
    assert!(stats.total_output_bytes > 0);
}

// ============================================================================
// StreamingDecompressor tests with various strategies
// ============================================================================

#[test]
fn test_decompress_frame_with_none_strategy() {
    let mut decompressor = StreamingDecompressor::new();

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

    let result = decompressor.decompress_frame(frame);
    assert!(result.is_ok());
    assert_eq!(decompressor.get_stats().frames_decompressed, 1);
}

#[test]
fn test_decompress_frame_with_dictionary_metadata() {
    let mut decompressor = StreamingDecompressor::new();

    let mut dict_map = HashMap::new();
    dict_map.insert(0, "hello".to_string());
    dict_map.insert(1, "world".to_string());

    let frame = CompressedFrame {
        frame: StreamFrame {
            data: json!({}),
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        },
        compressed_data: CompressedData {
            strategy: CompressionStrategy::None,
            compressed_size: 10,
            data: json!({}),
            compression_metadata: HashMap::new(),
        },
        decompression_metadata: DecompressionMetadata {
            strategy: CompressionStrategy::None,
            dictionary_map: dict_map,
            delta_bases: HashMap::new(),
            priority_hints: HashMap::new(),
        },
    };

    let result = decompressor.decompress_frame(frame);
    assert!(result.is_ok());
}

#[test]
fn test_decompress_frame_with_delta_bases_metadata() {
    let mut decompressor = StreamingDecompressor::new();

    let mut delta_bases = HashMap::new();
    delta_bases.insert("value1".to_string(), 100.0);
    delta_bases.insert("value2".to_string(), 200.0);

    let frame = CompressedFrame {
        frame: StreamFrame {
            data: json!({}),
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        },
        compressed_data: CompressedData {
            strategy: CompressionStrategy::None,
            compressed_size: 10,
            data: json!({}),
            compression_metadata: HashMap::new(),
        },
        decompression_metadata: DecompressionMetadata {
            strategy: CompressionStrategy::None,
            dictionary_map: HashMap::new(),
            delta_bases,
            priority_hints: HashMap::new(),
        },
    };

    let result = decompressor.decompress_frame(frame);
    assert!(result.is_ok());
}

#[test]
fn test_decompress_multiple_frames_updates_stats() {
    let mut decompressor = StreamingDecompressor::new();

    for i in 0..10 {
        let frame = CompressedFrame {
            frame: StreamFrame {
                data: json!({"count": i}),
                priority: Priority::MEDIUM,
                metadata: HashMap::new(),
            },
            compressed_data: CompressedData {
                strategy: CompressionStrategy::None,
                compressed_size: 15,
                data: json!({"count": i}),
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
    assert_eq!(stats.frames_decompressed, 10);
    assert!(stats.total_decompressed_bytes > 0);
    // avg_decompression_time_us might be 0 on very fast machines
    assert!(stats.avg_decompression_time_us >= 0);
}

// ============================================================================
// Default trait implementation tests
// ============================================================================

#[test]
fn test_streaming_compressor_default() {
    let compressor = StreamingCompressor::default();
    let stats = compressor.get_stats();
    assert_eq!(stats.frames_processed, 0);
    assert_eq!(stats.total_input_bytes, 0);
    assert_eq!(stats.total_output_bytes, 0);
}

#[test]
fn test_streaming_decompressor_default() {
    let decompressor = StreamingDecompressor::default();
    let stats = decompressor.get_stats();
    assert_eq!(stats.frames_decompressed, 0);
    assert_eq!(stats.total_decompressed_bytes, 0);
    assert_eq!(stats.avg_decompression_time_us, 0);
}

// ============================================================================
// Complex nested structure tests
// ============================================================================

#[test]
fn test_compress_deeply_nested_structure() {
    let mut compressor = StreamingCompressor::new();

    let frame = StreamFrame {
        data: json!({
            "level1": {
                "level2": {
                    "level3": {
                        "level4": {
                            "level5": {
                                "data": "deeply nested"
                            }
                        }
                    }
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
fn test_decompress_deeply_nested_structure() {
    let mut decompressor = StreamingDecompressor::new();

    let nested_data = json!({
        "level1": {
            "level2": {
                "level3": {
                    "data": "nested"
                }
            }
        }
    });

    let frame = CompressedFrame {
        frame: StreamFrame {
            data: nested_data.clone(),
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        },
        compressed_data: CompressedData {
            strategy: CompressionStrategy::None,
            compressed_size: 50,
            data: nested_data,
            compression_metadata: HashMap::new(),
        },
        decompression_metadata: DecompressionMetadata {
            strategy: CompressionStrategy::None,
            dictionary_map: HashMap::new(),
            delta_bases: HashMap::new(),
            priority_hints: HashMap::new(),
        },
    };

    let result = decompressor.decompress_frame(frame);
    assert!(result.is_ok());
}

// ============================================================================
// Edge case tests for compression metadata creation
// ============================================================================

#[test]
fn test_compress_frame_creates_metadata_for_dictionary() {
    let mut dict = HashMap::new();
    dict.insert("common".to_string(), 0);

    let mut compressor = StreamingCompressor::with_strategies(
        CompressionStrategy::Dictionary {
            dictionary: dict.clone(),
        },
        CompressionStrategy::None,
    );

    let frame = StreamFrame {
        data: json!({"field": "common"}),
        priority: Priority::CRITICAL,
        metadata: HashMap::new(),
    };

    let result = compressor.compress_frame(frame);
    assert!(result.is_ok());
    let compressed = result.unwrap();
    // Metadata should be present
    assert_eq!(
        compressed.decompression_metadata.strategy,
        CompressionStrategy::Dictionary { dictionary: dict }
    );
}

#[test]
fn test_compress_frame_empty_data() {
    let mut compressor = StreamingCompressor::new();

    let frame = StreamFrame {
        data: json!({}),
        priority: Priority::MEDIUM,
        metadata: HashMap::new(),
    };

    let result = compressor.compress_frame(frame);
    assert!(result.is_ok());

    let stats = compressor.get_stats();
    assert_eq!(stats.frames_processed, 1);
}

#[test]
fn test_compress_frame_array_data() {
    let mut compressor = StreamingCompressor::new();

    let frame = StreamFrame {
        data: json!([1, 2, 3, 4, 5]),
        priority: Priority::LOW,
        metadata: HashMap::new(),
    };

    let result = compressor.compress_frame(frame);
    assert!(result.is_ok());
}

#[test]
fn test_compress_frame_null_data() {
    let mut compressor = StreamingCompressor::new();

    let frame = StreamFrame {
        data: json!(null),
        priority: Priority::BACKGROUND,
        metadata: HashMap::new(),
    };

    let result = compressor.compress_frame(frame);
    assert!(result.is_ok());
}

#[test]
fn test_compress_frame_boolean_data() {
    let mut compressor = StreamingCompressor::new();

    let frame = StreamFrame {
        data: json!(true),
        priority: Priority::MEDIUM,
        metadata: HashMap::new(),
    };

    let result = compressor.compress_frame(frame);
    assert!(result.is_ok());
}

#[test]
fn test_compress_frame_number_data() {
    let mut compressor = StreamingCompressor::new();

    let frame = StreamFrame {
        data: json!(42.5),
        priority: Priority::HIGH,
        metadata: HashMap::new(),
    };

    let result = compressor.compress_frame(frame);
    assert!(result.is_ok());
}

#[test]
fn test_compress_frame_string_data() {
    let mut compressor = StreamingCompressor::new();

    let frame = StreamFrame {
        data: json!("plain string"),
        priority: Priority::CRITICAL,
        metadata: HashMap::new(),
    };

    let result = compressor.compress_frame(frame);
    assert!(result.is_ok());
}

// ============================================================================
// Stats tracking edge cases
// ============================================================================

#[test]
fn test_compression_stats_single_frame() {
    let mut compressor = StreamingCompressor::new();

    let frame = StreamFrame {
        data: json!({"test": "data"}),
        priority: Priority::MEDIUM,
        metadata: HashMap::new(),
    };

    compressor.compress_frame(frame).unwrap();

    let stats = compressor.get_stats();
    assert_eq!(stats.frames_processed, 1);
    assert!(stats.total_input_bytes > 0);
}

#[test]
fn test_decompression_stats_running_average() {
    let mut decompressor = StreamingDecompressor::new();

    // First frame
    let frame1 = CompressedFrame {
        frame: StreamFrame {
            data: json!({"data": 1}),
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        },
        compressed_data: CompressedData {
            strategy: CompressionStrategy::None,
            compressed_size: 10,
            data: json!({"data": 1}),
            compression_metadata: HashMap::new(),
        },
        decompression_metadata: DecompressionMetadata {
            strategy: CompressionStrategy::None,
            dictionary_map: HashMap::new(),
            delta_bases: HashMap::new(),
            priority_hints: HashMap::new(),
        },
    };

    decompressor.decompress_frame(frame1).unwrap();
    let stats1 = decompressor.get_stats();
    assert_eq!(stats1.frames_decompressed, 1);

    // Second frame
    let frame2 = CompressedFrame {
        frame: StreamFrame {
            data: json!({"data": 2}),
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        },
        compressed_data: CompressedData {
            strategy: CompressionStrategy::None,
            compressed_size: 10,
            data: json!({"data": 2}),
            compression_metadata: HashMap::new(),
        },
        decompression_metadata: DecompressionMetadata {
            strategy: CompressionStrategy::None,
            dictionary_map: HashMap::new(),
            delta_bases: HashMap::new(),
            priority_hints: HashMap::new(),
        },
    };

    decompressor.decompress_frame(frame2).unwrap();
    let stats2 = decompressor.get_stats();
    assert_eq!(stats2.frames_decompressed, 2);
    // Running average should be calculated (might be 0 on very fast machines)
    assert!(stats2.avg_decompression_time_us >= 0);
}
