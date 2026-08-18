//! Integration tests for compression + streaming functionality
//!
//! Tests the interaction between schema-based compression and priority streaming

use pjson_rs::compression::CompressionStrategy;
use pjson_rs::domain::value_objects::Priority;
use pjson_rs::stream::StreamFrame;
use pjson_rs::stream::compression_integration::{
    CompressedFrame, CompressionStats, DecompressionMetadata, StreamingCompressor,
    StreamingDecompressor,
};
use serde_json::json;
use std::collections::HashMap;

#[test]
fn test_streaming_compressor_creation() {
    let compressor = StreamingCompressor::new();
    let stats = compressor.stats();
    assert_eq!(stats.total_input_bytes, 0);
    assert_eq!(stats.total_output_bytes, 0);
    assert_eq!(stats.frames_processed, 0);
}

#[test]
fn test_streaming_compressor_with_custom_strategies() {
    let mut dictionary = HashMap::new();
    dictionary.insert("test".to_string(), 1);

    let skeleton_strategy = CompressionStrategy::Dictionary {
        dictionary: dictionary.clone(),
    };

    let mut base_values = HashMap::new();
    base_values.insert("value".to_string(), 100.0);

    let content_strategy = CompressionStrategy::Delta { base_values };

    let compressor =
        StreamingCompressor::with_strategies(skeleton_strategy, content_strategy.clone());

    // Verify compressor was created successfully
    let stats = compressor.stats();
    assert_eq!(stats.frames_processed, 0);
}

#[test]
fn test_compress_critical_priority_frame() {
    let mut compressor = StreamingCompressor::new();

    let frame = StreamFrame {
        data: json!({
            "error": "critical failure",
            "timestamp": 1234567890,
            "severity": "critical"
        }),
        priority: Priority::CRITICAL,
        metadata: HashMap::new(),
    };

    let result = compressor.compress_frame(frame.clone());
    assert!(result.is_ok());

    let compressed = result.unwrap();
    assert_eq!(compressed.frame.priority, Priority::CRITICAL);
    assert_eq!(compressed.frame.data, frame.data);

    // Verify stats were updated
    let stats = compressor.stats();
    assert_eq!(stats.frames_processed, 1);
    assert!(stats.total_input_bytes > 0);
    assert!(stats.total_output_bytes > 0);
}

#[test]
fn test_compress_multiple_frames_with_different_priorities() {
    let mut compressor = StreamingCompressor::new();

    let critical_frame = StreamFrame {
        data: json!({"error": "critical"}),
        priority: Priority::CRITICAL,
        metadata: HashMap::new(),
    };

    let low_frame = StreamFrame {
        data: json!({"debug": "info"}),
        priority: Priority::LOW,
        metadata: HashMap::new(),
    };

    let medium_frame = StreamFrame {
        data: json!({"data": "content"}),
        priority: Priority::MEDIUM,
        metadata: HashMap::new(),
    };

    // Compress all frames
    let _r1 = compressor.compress_frame(critical_frame).unwrap();
    let _r2 = compressor.compress_frame(low_frame).unwrap();
    let _r3 = compressor.compress_frame(medium_frame).unwrap();

    // Verify all frames were processed
    let stats = compressor.stats();
    assert_eq!(stats.frames_processed, 3);

    // Verify different priority levels were tracked
    assert!(
        stats
            .priority_ratios
            .contains_key(&Priority::CRITICAL.value())
    );
    assert!(stats.priority_ratios.contains_key(&Priority::LOW.value()));
    assert!(
        stats
            .priority_ratios
            .contains_key(&Priority::MEDIUM.value())
    );
}

#[test]
fn test_optimize_for_data() {
    let mut compressor = StreamingCompressor::new();

    let skeleton = json!({
        "type": "object",
        "properties": {
            "id": {"type": "number"},
            "name": {"type": "string"}
        }
    });

    let sample_data = vec![
        json!({"id": 1, "name": "Alice"}),
        json!({"id": 2, "name": "Bob"}),
        json!({"id": 3, "name": "Charlie"}),
    ];

    let result = compressor.optimize_for_data(&skeleton, &sample_data);
    assert!(result.is_ok());
}

#[test]
fn test_optimize_for_empty_samples() {
    let mut compressor = StreamingCompressor::new();

    let skeleton = json!({"type": "object"});
    let empty_samples: Vec<serde_json::Value> = vec![];

    let result = compressor.optimize_for_data(&skeleton, &empty_samples);
    assert!(result.is_ok());
}

#[test]
fn test_compression_stats_overall_ratio() {
    let stats = CompressionStats {
        total_input_bytes: 1000,
        total_output_bytes: 600,
        frames_processed: 5,
        priority_ratios: HashMap::new(),
    };

    assert_eq!(stats.overall_compression_ratio(), 0.6);
}

#[test]
fn test_compression_stats_with_zero_input() {
    let stats = CompressionStats::default();
    assert_eq!(stats.overall_compression_ratio(), 1.0);
    assert_eq!(stats.bytes_saved(), 0);
    assert_eq!(stats.percentage_saved(), 0.0);
}

#[test]
fn test_compression_stats_bytes_saved() {
    let stats = CompressionStats {
        total_input_bytes: 2000,
        total_output_bytes: 1200,
        frames_processed: 10,
        priority_ratios: HashMap::new(),
    };

    assert_eq!(stats.bytes_saved(), 800);
}

#[test]
fn test_compression_stats_percentage_saved() {
    let stats = CompressionStats {
        total_input_bytes: 1000,
        total_output_bytes: 300,
        frames_processed: 5,
        priority_ratios: HashMap::new(),
    };

    let percentage = stats.percentage_saved();
    assert!((percentage - 70.0).abs() < 0.001);
}

#[test]
fn test_compression_stats_priority_ratio() {
    let mut priority_ratios = HashMap::new();
    priority_ratios.insert(Priority::HIGH.value(), 0.5);
    priority_ratios.insert(Priority::LOW.value(), 0.8);

    let stats = CompressionStats {
        total_input_bytes: 1000,
        total_output_bytes: 600,
        frames_processed: 2,
        priority_ratios,
    };

    assert_eq!(
        stats.priority_compression_ratio(Priority::HIGH.value()),
        0.5
    );
    assert_eq!(stats.priority_compression_ratio(Priority::LOW.value()), 0.8);
    assert_eq!(stats.priority_compression_ratio(99), 1.0); // Non-existent priority
}

#[test]
fn test_reset_stats() {
    let mut compressor = StreamingCompressor::new();

    let frame = StreamFrame {
        data: json!({"test": "data"}),
        priority: Priority::MEDIUM,
        metadata: HashMap::new(),
    };

    let _compressed = compressor.compress_frame(frame).unwrap();

    // Verify stats were recorded
    assert_eq!(compressor.stats().frames_processed, 1);

    // Reset stats
    compressor.reset_stats();

    // Verify stats were cleared
    let stats = compressor.stats();
    assert_eq!(stats.total_input_bytes, 0);
    assert_eq!(stats.total_output_bytes, 0);
    assert_eq!(stats.frames_processed, 0);
    assert!(stats.priority_ratios.is_empty());
}

#[test]
fn test_streaming_decompressor_creation() {
    let decompressor = StreamingDecompressor::new();
    let stats = decompressor.stats();
    assert_eq!(stats.frames_decompressed, 0);
    assert_eq!(stats.total_decompressed_bytes, 0);
}

#[test]
fn test_decompressor_default_trait() {
    let decompressor = StreamingDecompressor::default();
    assert_eq!(decompressor.stats().frames_decompressed, 0);
}

#[test]
fn test_compressor_default_trait() {
    let compressor = StreamingCompressor::default();
    assert_eq!(compressor.stats().frames_processed, 0);
}

#[test]
fn test_decompress_frame_with_no_compression() {
    let mut decompressor = StreamingDecompressor::new();

    let test_data = json!({"test": "data", "value": 42});

    let compressed_frame = CompressedFrame {
        frame: StreamFrame {
            data: test_data.clone(),
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        },
        compressed_data: pjson_rs::compression::CompressedData {
            strategy: CompressionStrategy::None,
            compressed_size: 30,
            data: test_data.clone(),
            compression_metadata: HashMap::new(),
        },
        decompression_metadata: DecompressionMetadata {
            strategy: CompressionStrategy::None,
            dictionary_map: HashMap::new(),
            delta_bases: HashMap::new(),
        },
    };

    let result = decompressor.decompress_frame(compressed_frame);
    assert!(result.is_ok());

    let decompressed = result.unwrap();
    assert_eq!(decompressed.data, test_data);
    assert_eq!(decompressed.priority, Priority::MEDIUM);

    // Verify stats were updated
    let stats = decompressor.stats();
    assert_eq!(stats.frames_decompressed, 1);
    assert!(stats.total_decompressed_bytes > 0);
}

#[test]
fn test_decompress_with_dictionary_metadata() {
    let mut decompressor = StreamingDecompressor::new();

    let mut dictionary_map = HashMap::new();
    dictionary_map.insert(0, "hello".to_string());
    dictionary_map.insert(1, "world".to_string());

    let compressed_data = json!({
        "greeting": "\u{7F}0",
        "target": "\u{7F}1"
    });

    let compressed_frame = CompressedFrame {
        frame: StreamFrame {
            data: compressed_data.clone(),
            priority: Priority::HIGH,
            metadata: HashMap::new(),
        },
        compressed_data: pjson_rs::compression::CompressedData {
            strategy: CompressionStrategy::Dictionary {
                dictionary: HashMap::new(),
            },
            compressed_size: 20,
            data: compressed_data,
            compression_metadata: HashMap::new(),
        },
        decompression_metadata: DecompressionMetadata {
            strategy: CompressionStrategy::Dictionary {
                dictionary: HashMap::new(),
            },
            dictionary_map: dictionary_map.clone(),
            delta_bases: HashMap::new(),
        },
    };

    let result = decompressor.decompress_frame(compressed_frame);
    assert!(result.is_ok());

    let decompressed = result.unwrap();
    assert_eq!(
        decompressed.data,
        json!({
            "greeting": "hello",
            "target": "world"
        })
    );
}

#[test]
fn test_decompress_nested_dictionary_values() {
    let mut decompressor = StreamingDecompressor::new();

    let mut dictionary_map = HashMap::new();
    dictionary_map.insert(0, "status".to_string());
    dictionary_map.insert(1, "active".to_string());

    // "value" happens to hold the same raw number (1) that "active"'s dictionary index
    // encodes as, but it stays a number: only JSON strings are ever candidates for
    // dictionary substitution, so there is no value-based ambiguity to guard against.
    let compressed_data = json!({
        "items": [
            {"field": "\u{7F}0", "value": 1},
            {"field": "\u{7F}0", "value": 1}
        ]
    });

    let compressed_frame = CompressedFrame {
        frame: StreamFrame {
            data: compressed_data.clone(),
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        },
        compressed_data: pjson_rs::compression::CompressedData {
            strategy: CompressionStrategy::Dictionary {
                dictionary: HashMap::new(),
            },
            compressed_size: 50,
            data: compressed_data,
            compression_metadata: HashMap::new(),
        },
        decompression_metadata: DecompressionMetadata {
            strategy: CompressionStrategy::Dictionary {
                dictionary: HashMap::new(),
            },
            dictionary_map,
            delta_bases: HashMap::new(),
        },
    };

    let result = decompressor.decompress_frame(compressed_frame);
    assert!(result.is_ok());

    let decompressed = result.unwrap();
    assert_eq!(
        decompressed.data,
        json!({
            "items": [
                {"field": "status", "value": 1},
                {"field": "status", "value": 1}
            ]
        })
    );
}

#[test]
fn test_decompress_delta_strategy() {
    let mut decompressor = StreamingDecompressor::new();

    // Delta-compressed format: first element is metadata, rest are deltas
    let compressed_data = json!({
        "values": [
            {"delta_base": 100.0, "delta_type": "numeric_sequence"},
            0.0,
            1.0,
            2.0,
            3.0,
            4.0
        ]
    });

    let compressed_frame = CompressedFrame {
        frame: StreamFrame {
            data: compressed_data.clone(),
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        },
        compressed_data: pjson_rs::compression::CompressedData {
            strategy: CompressionStrategy::Delta {
                base_values: HashMap::new(),
            },
            compressed_size: 30,
            data: compressed_data,
            compression_metadata: HashMap::new(),
        },
        decompression_metadata: DecompressionMetadata {
            strategy: CompressionStrategy::Delta {
                base_values: HashMap::new(),
            },
            dictionary_map: HashMap::new(),
            delta_bases: HashMap::new(),
        },
    };

    let result = decompressor.decompress_frame(compressed_frame);
    assert!(result.is_ok());

    let decompressed = result.unwrap();
    assert_eq!(
        decompressed.data,
        json!({"values": [100.0, 101.0, 102.0, 103.0, 104.0]})
    );
}

#[test]
fn test_decompress_run_length_strategy() {
    let mut decompressor = StreamingDecompressor::new();

    // RLE-compressed format
    let compressed_data = json!({
        "data": [
            {"rle_value": 1, "rle_count": 3},
            {"rle_value": 2, "rle_count": 2},
            3
        ]
    });

    let compressed_frame = CompressedFrame {
        frame: StreamFrame {
            data: compressed_data.clone(),
            priority: Priority::LOW,
            metadata: HashMap::new(),
        },
        compressed_data: pjson_rs::compression::CompressedData {
            strategy: CompressionStrategy::RunLength,
            compressed_size: 25,
            data: compressed_data,
            compression_metadata: HashMap::new(),
        },
        decompression_metadata: DecompressionMetadata {
            strategy: CompressionStrategy::RunLength,
            dictionary_map: HashMap::new(),
            delta_bases: HashMap::new(),
        },
    };

    let result = decompressor.decompress_frame(compressed_frame);
    assert!(result.is_ok());

    let decompressed = result.unwrap();
    assert_eq!(decompressed.data, json!({"data": [1, 1, 1, 2, 2, 3]}));
}

#[test]
fn test_decompress_hybrid_strategy() {
    let mut decompressor = StreamingDecompressor::new();

    let mut dictionary_map = HashMap::new();
    dictionary_map.insert(0, "test".to_string());

    let compressed_data = json!({"field": "\u{7F}0"});

    let compressed_frame = CompressedFrame {
        frame: StreamFrame {
            data: compressed_data.clone(),
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        },
        compressed_data: pjson_rs::compression::CompressedData {
            strategy: CompressionStrategy::Hybrid {
                string_dict: HashMap::new(),
                numeric_deltas: HashMap::new(),
            },
            compressed_size: 15,
            data: compressed_data,
            compression_metadata: HashMap::new(),
        },
        decompression_metadata: DecompressionMetadata {
            strategy: CompressionStrategy::Hybrid {
                string_dict: HashMap::new(),
                numeric_deltas: HashMap::new(),
            },
            dictionary_map,
            delta_bases: HashMap::new(),
        },
    };

    let result = decompressor.decompress_frame(compressed_frame);
    assert!(result.is_ok());

    let decompressed = result.unwrap();
    assert_eq!(decompressed.data, json!({"field": "test"}));
}

#[test]
fn test_decompressor_stats_accumulation() {
    let mut decompressor = StreamingDecompressor::new();

    // Decompress multiple frames
    for i in 0..5 {
        let compressed_frame = CompressedFrame {
            frame: StreamFrame {
                data: json!({"iteration": i}),
                priority: Priority::MEDIUM,
                metadata: HashMap::new(),
            },
            compressed_data: pjson_rs::compression::CompressedData {
                strategy: CompressionStrategy::None,
                compressed_size: 20,
                data: json!({"iteration": i}),
                compression_metadata: HashMap::new(),
            },
            decompression_metadata: DecompressionMetadata {
                strategy: CompressionStrategy::None,
                dictionary_map: HashMap::new(),
                delta_bases: HashMap::new(),
            },
        };

        let _result = decompressor.decompress_frame(compressed_frame).unwrap();
    }

    let stats = decompressor.stats();
    assert_eq!(stats.frames_decompressed, 5);
    assert!(stats.total_decompressed_bytes > 0);
    // Note: avg_decompression_time_us may be 0 for very fast operations
}

#[test]
fn test_end_to_end_compression_decompression() {
    let mut compressor = StreamingCompressor::new();
    let mut decompressor = StreamingDecompressor::new();

    // Create test frame
    let original_data = json!({
        "user": "alice",
        "action": "login",
        "timestamp": 1234567890
    });

    let frame = StreamFrame {
        data: original_data.clone(),
        priority: Priority::HIGH,
        metadata: HashMap::new(),
    };

    // Compress
    let compressed_frame = compressor.compress_frame(frame).unwrap();

    // Verify compression occurred
    assert!(compressor.stats().frames_processed == 1);

    // Decompress
    let decompressed_frame = decompressor.decompress_frame(compressed_frame).unwrap();

    // Verify data integrity
    assert_eq!(decompressed_frame.data, original_data);
    assert_eq!(decompressed_frame.priority, Priority::HIGH);
}

#[test]
fn test_end_to_end_dictionary_round_trip_preserves_numeric_fields() {
    // Regression test for issue #333's C1 finding: before the sentinel-marker redesign, the
    // dictionary decoder converted ANY number matching a dictionary index back into a string,
    // with no way to distinguish an encoded index from a genuine payload integer (e.g. `"page":
    // 1` could become `"page": "active"` whenever "active" happened to be assigned dictionary
    // index 1). Sentinel-escaped string markers close this structurally: a substituted value is
    // always a JSON string, so no number is ever a candidate. This exercises the full analyze ->
    // compress -> decompress pipeline on a realistic payload with genuine repetition (5 users,
    // 4/5 sharing "status" and "role" values) that nets a real wire-byte saving.
    let mut compressor = StreamingCompressor::new();
    let mut decompressor = StreamingDecompressor::new();

    let original_data = json!({
        "status": "success",
        "data": {
            "users": [
                {"id": "user_001", "email": "alice@example.com", "status": "subscription_active", "role": "standard_user", "created_at": "2024-01-01T00:00:00Z", "last_login": "2024-01-15T10:30:00Z"},
                {"id": "user_002", "email": "bob@example.com", "status": "subscription_active", "role": "standard_user", "created_at": "2024-01-02T00:00:00Z", "last_login": "2024-01-15T09:15:00Z"},
                {"id": "user_003", "email": "charlie@example.com", "status": "subscription_active", "role": "standard_user", "created_at": "2024-01-03T00:00:00Z", "last_login": "2024-01-10T14:22:00Z"},
                {"id": "user_004", "email": "dave@example.com", "status": "subscription_active", "role": "administrator", "created_at": "2024-01-04T00:00:00Z", "last_login": "2024-01-14T11:05:00Z"},
                {"id": "user_005", "email": "erin@example.com", "status": "subscription_inactive", "role": "standard_user", "created_at": "2024-01-05T00:00:00Z", "last_login": "2024-01-09T08:40:00Z"}
            ]
        },
        "pagination": {"page": 1, "per_page": 25, "total_pages": 4, "total_items": 89},
        "meta": {"request_id": "req_12345", "timestamp": "2024-01-15T10:30:15Z", "version": "v1.2.3"}
    });

    compressor.optimize_for_data(&original_data, &[]).unwrap();

    let frame = StreamFrame {
        data: original_data.clone(),
        priority: Priority::CRITICAL,
        metadata: HashMap::new(),
    };

    let compressed_frame = compressor.compress_frame(frame).unwrap();
    assert!(
        matches!(
            compressed_frame.compressed_data.strategy,
            CompressionStrategy::Dictionary { .. }
        ),
        "expected Dictionary strategy for this payload, got {:?}",
        compressed_frame.compressed_data.strategy
    );

    let decompressed_frame = decompressor.decompress_frame(compressed_frame).unwrap();

    assert_eq!(decompressed_frame.data, original_data);
    // Explicitly pin the numbers that collide with dictionary indices (0/1) in the
    // encoded output, since those are exactly what a value-based decoder would corrupt.
    assert_eq!(decompressed_frame.data["pagination"]["page"], json!(1));
    assert_eq!(
        decompressed_frame.data["pagination"]["total_pages"],
        json!(4)
    );
}

#[test]
fn test_dictionary_forced_key_value_collision_c2_object_round_trips() {
    // Regression test for issue #333's C2 finding (round 2): a payload containing both a
    // nested `{"a": {"b": 0}}` and a sibling `"a.b"` key whose value is dictionary-substituted
    // used to collide under path-string metadata, since both stringify to the same "a.b" path.
    // Sentinel markers are self-describing per value, so no path metadata exists to collide.
    let mut dictionary = HashMap::new();
    dictionary.insert("active".to_string(), 0);

    let mut compressor = StreamingCompressor::with_strategies(
        CompressionStrategy::None,
        CompressionStrategy::Dictionary { dictionary },
    );

    let original_data = json!({"a": {"b": 0}, "a.b": "active"});

    let frame = StreamFrame {
        data: original_data.clone(),
        priority: Priority::LOW,
        metadata: HashMap::new(),
    };
    let compressed_frame = compressor.compress_frame(frame).unwrap();

    let mut decompressor = StreamingDecompressor::new();
    let decompressed_frame = decompressor.decompress_frame(compressed_frame).unwrap();

    assert_eq!(decompressed_frame.data, original_data);
}

#[test]
fn test_dictionary_forced_key_value_collision_c2_array_round_trips() {
    // Array-indexing counterpart of the object collision above: `{"a":[0]}` and a sibling
    // `"a[0]"` key both stringify to the same "a[0]" path under the old path-metadata scheme.
    let mut dictionary = HashMap::new();
    dictionary.insert("active".to_string(), 0);

    let mut compressor = StreamingCompressor::with_strategies(
        CompressionStrategy::None,
        CompressionStrategy::Dictionary { dictionary },
    );

    let original_data = json!({"a": [0], "a[0]": "active"});

    let frame = StreamFrame {
        data: original_data.clone(),
        priority: Priority::LOW,
        metadata: HashMap::new(),
    };
    let compressed_frame = compressor.compress_frame(frame).unwrap();

    let mut decompressor = StreamingDecompressor::new();
    let decompressed_frame = decompressor.decompress_frame(compressed_frame).unwrap();

    assert_eq!(decompressed_frame.data, original_data);
}

#[test]
fn test_dictionary_sentinel_escaping_round_trips_losslessly() {
    // Injectivity proof for the sentinel-marker encoding: payload strings that legitimately
    // start with the sentinel byte, including one that mimics a real marker's exact shape
    // ("\u{7F}0"), must survive a non-empty dictionary unchanged.
    let mut dictionary = HashMap::new();
    dictionary.insert("greeting".to_string(), 0);

    let mut compressor = StreamingCompressor::with_strategies(
        CompressionStrategy::None,
        CompressionStrategy::Dictionary { dictionary },
    );

    let original_data = json!({
        "a": "\u{7F}foo",
        "b": "\u{7F}\u{7F}bar",
        "c": "\u{7F}0",
        "d": "greeting"
    });

    let frame = StreamFrame {
        data: original_data.clone(),
        priority: Priority::LOW,
        metadata: HashMap::new(),
    };
    let compressed_frame = compressor.compress_frame(frame).unwrap();

    let mut decompressor = StreamingDecompressor::new();
    let decompressed_frame = decompressor.decompress_frame(compressed_frame).unwrap();

    assert_eq!(decompressed_frame.data, original_data);
}

#[test]
fn test_dictionary_decode_rejects_out_of_range_marker_index() {
    // A malformed/forged marker referencing a dictionary index that was never assigned must
    // error out rather than silently pass through as a string (issue #333 M7).
    let mut compressor = StreamingCompressor::with_strategies(
        CompressionStrategy::None,
        CompressionStrategy::Dictionary {
            dictionary: [("alpha".to_string(), 0), ("beta".to_string(), 1)]
                .into_iter()
                .collect(),
        },
    );
    let mut decompressor = StreamingDecompressor::new();

    // Seed the decompressor's active dictionary with indices 0 and 1 via a real frame.
    let seed_frame = StreamFrame {
        data: json!({"a": "alpha", "b": "beta"}),
        priority: Priority::LOW,
        metadata: HashMap::new(),
    };
    let compressed_seed = compressor.compress_frame(seed_frame).unwrap();
    decompressor.decompress_frame(compressed_seed).unwrap();

    let malformed_frame = CompressedFrame {
        frame: StreamFrame {
            data: json!({"value": "\u{7F}9"}),
            priority: Priority::LOW,
            metadata: HashMap::new(),
        },
        compressed_data: pjson_rs::compression::CompressedData {
            strategy: CompressionStrategy::Dictionary {
                dictionary: HashMap::new(),
            },
            compressed_size: 20,
            data: json!({"value": "\u{7F}9"}),
            compression_metadata: HashMap::new(),
        },
        decompression_metadata: DecompressionMetadata {
            strategy: CompressionStrategy::Dictionary {
                dictionary: HashMap::new(),
            },
            dictionary_map: HashMap::new(),
            delta_bases: HashMap::new(),
        },
    };

    let result = decompressor.decompress_frame(malformed_frame);
    assert!(result.is_err());
}

#[test]
fn test_hybrid_dictionary_and_delta_do_not_corrupt_each_other() {
    // Regression test for issue #333's M9 finding: dictionary decode is now
    // position-independent (self-describing sentinel markers), so a delta pass that prepends
    // `delta_base` metadata elsewhere in the payload can no longer shift positions a
    // position-based dictionary decode would have depended on.
    let mut string_dict = HashMap::new();
    string_dict.insert("active".to_string(), 0);
    string_dict.insert("inactive".to_string(), 1);

    let mut compressor = StreamingCompressor::with_strategies(
        CompressionStrategy::None,
        CompressionStrategy::Hybrid {
            string_dict,
            numeric_deltas: HashMap::new(),
        },
    );

    let original_data = json!({"states": ["active", "inactive", "active", "inactive"]});

    let frame = StreamFrame {
        data: original_data.clone(),
        priority: Priority::MEDIUM,
        metadata: HashMap::new(),
    };
    let compressed_frame = compressor.compress_frame(frame).unwrap();

    let mut decompressor = StreamingDecompressor::new();
    let decompressed_frame = decompressor.decompress_frame(compressed_frame).unwrap();

    assert_eq!(decompressed_frame.data, original_data);
}

#[test]
fn test_analyze_and_compress_is_deterministic_across_runs() {
    // Regression test for issue #333's M6 finding: dictionary index assignment used to iterate
    // a HashMap, so wire bytes were not reproducible across runs on the same payload.
    let data = json!({
        "products": [
            {"id": 1001, "name": "MacBook Pro", "category": "Electronics", "status": "available", "brand": "Apple", "price": 2399.99},
            {"id": 1002, "name": "iPhone 15", "category": "Electronics", "status": "available", "brand": "Apple", "price": 999.99},
            {"id": 1003, "name": "AirPods Pro", "category": "Electronics", "status": "available", "brand": "Apple", "price": 249.99}
        ],
        "store": {"name": "Tech Store", "status": "operational", "location": "San Francisco"}
    });

    let run = || {
        use pjson_rs::compression::SchemaCompressor;
        let mut compressor = SchemaCompressor::new();
        let strategy = compressor.analyze_and_optimize(&data).unwrap().clone();
        let compressed = SchemaCompressor::with_strategy(strategy)
            .compress(&data)
            .unwrap();
        serde_json::to_string(&compressed.data).unwrap()
    };

    assert_eq!(run(), run());
}

#[test]
fn test_large_frame_compression() {
    let mut compressor = StreamingCompressor::new();

    // Create a large JSON structure
    let mut large_data = serde_json::Map::new();
    for i in 0..100 {
        large_data.insert(format!("field_{}", i), json!(format!("value_{}", i)));
    }

    let frame = StreamFrame {
        data: json!(large_data),
        priority: Priority::MEDIUM,
        metadata: HashMap::new(),
    };

    let result = compressor.compress_frame(frame);
    assert!(result.is_ok());

    let stats = compressor.stats();
    assert!(stats.total_input_bytes > 1000); // Should be reasonably large
}

#[test]
fn test_compression_with_empty_data() {
    let mut compressor = StreamingCompressor::new();

    let frame = StreamFrame {
        data: json!({}),
        priority: Priority::LOW,
        metadata: HashMap::new(),
    };

    let result = compressor.compress_frame(frame);
    assert!(result.is_ok());

    let stats = compressor.stats();
    assert_eq!(stats.frames_processed, 1);
}

#[test]
fn test_compression_preserves_frame_metadata() {
    let mut compressor = StreamingCompressor::new();

    let mut metadata = HashMap::new();
    metadata.insert("client_id".to_string(), "test-client".to_string());
    metadata.insert("request_id".to_string(), "req-123".to_string());

    let frame = StreamFrame {
        data: json!({"data": "test"}),
        priority: Priority::HIGH,
        metadata: metadata.clone(),
    };

    let compressed = compressor.compress_frame(frame).unwrap();

    assert_eq!(compressed.frame.metadata, metadata);
}

#[test]
fn test_delta_compression_round_trip() {
    use pjson_rs::compression::SchemaCompressor;

    let mut base_values = HashMap::new();
    base_values.insert("values".to_string(), 100.0);

    let compressor = SchemaCompressor::with_strategy(CompressionStrategy::Delta { base_values });

    let original_data = json!({
        "values": [100.0, 101.0, 102.0, 103.0, 104.0]
    });

    let compressed = compressor.compress(&original_data).unwrap();

    let decompressor = StreamingDecompressor::new();
    let decompressed = decompressor.decompress_delta(&compressed.data).unwrap();

    assert_eq!(decompressed, original_data);
}

#[test]
fn test_rle_compression_round_trip() {
    use pjson_rs::compression::SchemaCompressor;

    let compressor = SchemaCompressor::with_strategy(CompressionStrategy::RunLength);

    let original_data = json!({
        "repeated_values": [1, 1, 1, 2, 2, 3, 3, 3, 3]
    });

    let compressed = compressor.compress(&original_data).unwrap();

    let decompressor = StreamingDecompressor::new();
    let decompressed = decompressor
        .decompress_run_length(&compressed.data)
        .unwrap();

    assert_eq!(decompressed, original_data);
}

#[test]
fn test_delta_compression_negative_values_round_trip() {
    use pjson_rs::compression::SchemaCompressor;

    let mut base_values = HashMap::new();
    base_values.insert("temps".to_string(), 0.0);

    let compressor = SchemaCompressor::with_strategy(CompressionStrategy::Delta { base_values });

    let original_data = json!({
        "temps": [-10.0, -5.0, 0.0, 5.0, 10.0]
    });

    let compressed = compressor.compress(&original_data).unwrap();

    let decompressor = StreamingDecompressor::new();
    let decompressed = decompressor.decompress_delta(&compressed.data).unwrap();

    assert_eq!(decompressed, original_data);
}

#[test]
fn test_rle_compression_mixed_types_round_trip() {
    use pjson_rs::compression::SchemaCompressor;

    let compressor = SchemaCompressor::with_strategy(CompressionStrategy::RunLength);

    let original_data = json!({
        "data": [
            "a", "a", "a",
            "b",
            "c", "c", "c", "c"
        ]
    });

    let compressed = compressor.compress(&original_data).unwrap();

    let decompressor = StreamingDecompressor::new();
    let decompressed = decompressor
        .decompress_run_length(&compressed.data)
        .unwrap();

    assert_eq!(decompressed, original_data);
}

#[test]
fn test_delta_compression_fractional_values() {
    use pjson_rs::compression::SchemaCompressor;

    let mut base_values = HashMap::new();
    base_values.insert("measurements".to_string(), 10.0);

    let compressor = SchemaCompressor::with_strategy(CompressionStrategy::Delta { base_values });

    let original_data = json!({
        "measurements": [10.5, 11.0, 11.5, 12.0, 12.5]
    });

    let compressed = compressor.compress(&original_data).unwrap();

    let decompressor = StreamingDecompressor::new();
    let decompressed = decompressor.decompress_delta(&compressed.data).unwrap();

    assert_eq!(decompressed, original_data);
}

#[test]
fn test_rle_compression_nested_objects() {
    use pjson_rs::compression::SchemaCompressor;

    let compressor = SchemaCompressor::with_strategy(CompressionStrategy::RunLength);

    let original_data = json!({
        "items": [
            {"id": 1},
            {"id": 1},
            {"id": 1},
            {"id": 2},
            {"id": 2}
        ]
    });

    let compressed = compressor.compress(&original_data).unwrap();

    let decompressor = StreamingDecompressor::new();
    let decompressed = decompressor
        .decompress_run_length(&compressed.data)
        .unwrap();

    assert_eq!(decompressed, original_data);
}

#[test]
fn test_delta_compression_empty_array() {
    use pjson_rs::compression::SchemaCompressor;

    let mut base_values = HashMap::new();
    base_values.insert("values".to_string(), 100.0);

    let compressor = SchemaCompressor::with_strategy(CompressionStrategy::Delta { base_values });

    let original_data = json!({
        "values": []
    });

    let compressed = compressor.compress(&original_data).unwrap();

    let decompressor = StreamingDecompressor::new();
    let decompressed = decompressor.decompress_delta(&compressed.data).unwrap();

    assert_eq!(decompressed, original_data);
}

#[test]
fn test_rle_compression_empty_array() {
    use pjson_rs::compression::SchemaCompressor;

    let compressor = SchemaCompressor::with_strategy(CompressionStrategy::RunLength);

    let original_data = json!({
        "data": []
    });

    let compressed = compressor.compress(&original_data).unwrap();

    let decompressor = StreamingDecompressor::new();
    let decompressed = decompressor
        .decompress_run_length(&compressed.data)
        .unwrap();

    assert_eq!(decompressed, original_data);
}

#[test]
fn test_full_frame_delta_round_trip() {
    let mut base_values = HashMap::new();
    base_values.insert("sequence".to_string(), 1000.0);

    let mut compressor = StreamingCompressor::with_strategies(
        CompressionStrategy::None,
        CompressionStrategy::Delta { base_values },
    );

    let original_data = json!({
        "sequence": [1000.0, 1001.0, 1002.0, 1003.0]
    });

    let frame = StreamFrame {
        data: original_data.clone(),
        priority: Priority::MEDIUM,
        metadata: HashMap::new(),
    };

    let compressed_frame = compressor.compress_frame(frame).unwrap();

    let mut decompressor = StreamingDecompressor::new();
    let decompressed_frame = decompressor.decompress_frame(compressed_frame).unwrap();

    assert_eq!(decompressed_frame.data, original_data);
}

#[test]
fn test_full_frame_rle_round_trip() {
    let mut compressor = StreamingCompressor::with_strategies(
        CompressionStrategy::None,
        CompressionStrategy::RunLength,
    );

    let original_data = json!({
        "states": ["active", "active", "active", "inactive", "inactive"]
    });

    let frame = StreamFrame {
        data: original_data.clone(),
        priority: Priority::LOW,
        metadata: HashMap::new(),
    };

    let compressed_frame = compressor.compress_frame(frame).unwrap();

    let mut decompressor = StreamingDecompressor::new();
    let decompressed_frame = decompressor.decompress_frame(compressed_frame).unwrap();

    assert_eq!(decompressed_frame.data, original_data);
}

// SECURITY TESTS - Protection against decompression bombs

#[test]
fn test_rle_bomb_protection() {
    let decompressor = StreamingDecompressor::new();

    // Attempt to create RLE bomb with count exceeding MAX_RLE_COUNT
    let bomb = json!([{"rle_value": "x", "rle_count": 100_000_001}]);

    let result = decompressor.decompress_run_length(&bomb);
    assert!(result.is_err());

    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("exceeds maximum"));
}

#[test]
fn test_delta_array_size_limit() {
    let decompressor = StreamingDecompressor::new();

    // Create delta array exceeding MAX_DELTA_ARRAY_SIZE
    let mut huge_array = vec![json!({"delta_base": 0.0, "delta_type": "numeric_sequence"})];
    huge_array.extend(vec![json!(1.0); 1_000_001]);

    let result = decompressor.decompress_delta(&json!(huge_array));
    assert!(result.is_err());

    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("Delta array size"));
    assert!(error_msg.contains("exceeds maximum"));
}

#[test]
fn test_decompression_total_size_limit() {
    let decompressor = StreamingDecompressor::new();

    // Multiple RLE runs that individually pass MAX_RLE_COUNT (100k)
    // but together exceed MAX_DECOMPRESSED_SIZE (10MB = 10,485,760 bytes)
    // Create 110 runs of 100k each = 11M total
    let mut runs = Vec::new();
    for _ in 0..110 {
        runs.push(json!({"rle_value": "x", "rle_count": 100_000}));
    }
    let data = json!(runs);

    let result = decompressor.decompress_run_length(&data);
    assert!(result.is_err());

    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("Decompressed size"));
    assert!(error_msg.contains("exceeds maximum"));
}

#[test]
fn test_rle_count_platform_limit() {
    let decompressor = StreamingDecompressor::new();

    // Test with u64::MAX to trigger platform overflow protection
    let overflow = json!([{"rle_value": "x", "rle_count": u64::MAX}]);

    let result = decompressor.decompress_run_length(&overflow);
    assert!(result.is_err());

    let error_msg = result.unwrap_err().to_string();
    // Should fail either on MAX_RLE_COUNT check or platform maximum check
    assert!(error_msg.contains("exceeds"));
}

// ============ Coverage gap fills (#132) ============

/// Covers the error path at compression_integration.rs L415–L418:
/// `decompress_delta_array` returns an error when `delta_base` is present
/// but its value cannot be coerced to `f64`.
///
/// The public entry point `decompress_delta` routes to `decompress_delta_array`
/// when the first array element is an object containing both `delta_base` and
/// `delta_type` keys, regardless of the actual value types.
#[test]
fn test_decompress_delta_array_missing_delta_base_errors() {
    let decompressor = StreamingDecompressor::new();

    // `delta_base` is a string — `as_f64()` returns `None`, triggering the
    // `ok_or_else` error at L414–L418 in compression_integration.rs.
    let data = json!([
        {"delta_base": "not_a_number", "delta_type": "numeric_sequence"},
        1.0,
        2.0
    ]);

    let result = decompressor.decompress_delta(&data);
    assert!(result.is_err(), "expected error for non-numeric delta_base");

    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("delta_base"),
        "error message should mention 'delta_base', got: {err_msg}"
    );
}
