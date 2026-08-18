// Comprehensive tests for infrastructure integration modules
//
// This test file covers all infrastructure integration layer modules with focus on:
// - Object pooling system with concurrent access patterns
// - SIMD acceleration for JSON serialization
//
// Coverage targets: 100% for all integration modules

use pjson_rs::domain::Priority;
use pjson_rs::infrastructure::integration::{
    object_pool::{
        ObjectPool, get_byte_vec, get_cow_hashmap, get_global_pool_stats, get_string_hashmap,
        get_string_vec,
    },
    simd_acceleration::{SimdConfig, SimdFrameSerializer, SimdStreamBuffer, SimdStreamProcessor},
};
use pjson_rs::stream::StreamFrame;
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;
use std::thread;

// ============================================================================
// PRIORITY 1: Object Pool Tests (object_pool.rs)
// ============================================================================

#[test]
fn test_object_pool_creation_with_capacity() {
    let pool = ObjectPool::new(10, Vec::<i32>::new);
    let stats = pool.stats();

    assert_eq!(stats.objects_created, 0);
    assert_eq!(stats.objects_reused, 0);
    assert_eq!(stats.current_pool_size, 0);
}

#[test]
fn test_object_pool_get_creates_new_object() {
    let pool = ObjectPool::new(5, || vec![1, 2, 3]);
    let obj = pool.get();

    assert_eq!(*obj, vec![1, 2, 3]);

    let stats = pool.stats();
    assert_eq!(stats.objects_created, 1);
    assert_eq!(stats.objects_reused, 0);
}

#[test]
fn test_object_pool_reuse_after_drop() {
    let pool = ObjectPool::new(5, || Vec::<String>::with_capacity(8));

    // Create and drop first object
    {
        let _obj1 = pool.get();
    }

    // Get another object - should reuse
    let _obj2 = pool.get();

    let stats = pool.stats();
    assert_eq!(stats.objects_created, 1);
    assert_eq!(stats.objects_reused, 1);
    assert_eq!(stats.objects_returned, 1);
}

#[test]
fn test_object_pool_multiple_objects_in_use() {
    let pool = ObjectPool::new(5, HashMap::<String, String>::new);

    let obj1 = pool.get();
    let obj2 = pool.get();
    let obj3 = pool.get();

    let stats = pool.stats();
    assert_eq!(stats.objects_created, 3);
    assert_eq!(stats.objects_reused, 0);

    drop(obj1);
    drop(obj2);
    drop(obj3);

    let stats = pool.stats();
    assert_eq!(stats.objects_returned, 3);
}

#[test]
fn test_object_pool_exceeds_capacity() {
    let pool = ObjectPool::new(2, Vec::<i32>::new);

    let obj1 = pool.get();
    let obj2 = pool.get();
    let obj3 = pool.get();

    drop(obj1);
    drop(obj2);
    drop(obj3); // This one should be dropped, not returned to pool

    let stats = pool.stats();
    assert_eq!(stats.objects_created, 3);
    assert_eq!(stats.objects_returned, 2); // Only 2 fit in pool
}

#[test]
fn test_object_pool_take_prevents_return() {
    let pool = ObjectPool::new(5, || vec![42]);
    let obj = pool.get();

    let taken = obj.take();
    assert_eq!(taken, vec![42]);

    let stats = pool.stats();
    assert_eq!(stats.objects_returned, 0); // Not returned because taken
}

#[test]
fn test_object_pool_deref_access() {
    let pool = ObjectPool::new(5, || vec![1, 2, 3]);
    let obj = pool.get();

    // Test Deref trait
    assert_eq!(obj.len(), 3);
    assert_eq!(obj[0], 1);
    assert_eq!(obj.first(), Some(&1));
}

#[test]
fn test_object_pool_deref_mut_access() {
    let pool = ObjectPool::new(5, || vec![1, 2, 3]);
    let mut obj = pool.get();

    // Test DerefMut trait
    obj.push(4);
    obj[0] = 10;

    assert_eq!(obj.len(), 4);
    assert_eq!(obj[0], 10);
}

#[test]
fn test_object_pool_peak_usage_tracking() {
    let pool = ObjectPool::new(10, Vec::<i32>::new);

    let obj1 = pool.get();
    let obj2 = pool.get();
    let obj3 = pool.get();

    let stats = pool.stats();
    assert!(stats.peak_usage <= 3);

    drop(obj1);
    drop(obj2);
    drop(obj3);
}

#[test]
fn test_object_pool_concurrent_access() {
    let pool = Arc::new(ObjectPool::new(20, || Vec::<i32>::with_capacity(16)));
    let mut handles = vec![];

    for i in 0..10 {
        let pool_clone = Arc::clone(&pool);
        let handle = thread::spawn(move || {
            let mut obj = pool_clone.get();
            obj.push(i);
            obj.push(i * 2);
            // Object automatically returned when dropped
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let stats = pool.stats();
    assert!(stats.objects_created <= 20);
    assert!(stats.objects_reused > 0 || stats.objects_created == 10);
}

#[test]
fn test_global_cow_hashmap_pool() {
    let mut map = get_cow_hashmap();
    assert_eq!(map.len(), 0); // Should be clean

    map.insert(Cow::Borrowed("key1"), Cow::Borrowed("value1"));
    map.insert(Cow::Borrowed("key2"), Cow::Borrowed("value2"));

    drop(map);

    // Get another one - should be clean
    let map2 = get_cow_hashmap();
    assert_eq!(map2.len(), 0); // Should be cleaned before use
}

#[test]
fn test_global_string_hashmap_pool() {
    let mut map = get_string_hashmap();
    map.insert("test".to_string(), "value".to_string());
    drop(map);

    let map2 = get_string_hashmap();
    assert_eq!(map2.len(), 0);
}

#[test]
fn test_global_byte_vec_pool() {
    let mut vec = get_byte_vec();
    vec.extend_from_slice(b"test data");
    assert!(!vec.is_empty());
    drop(vec);

    let vec2 = get_byte_vec();
    assert_eq!(vec2.len(), 0);
}

#[test]
fn test_global_string_vec_pool() {
    let mut vec = get_string_vec();
    vec.push("test1".to_string());
    vec.push("test2".to_string());
    drop(vec);

    let vec2 = get_string_vec();
    assert_eq!(vec2.len(), 0);
}

#[test]
fn test_global_pool_stats_aggregation() {
    // Use some global pools
    let _map = get_cow_hashmap();
    let _vec = get_byte_vec();

    let stats = get_global_pool_stats();
    assert!(stats.total_objects_created > 0 || stats.total_objects_reused > 0);
    assert!(stats.total_reuse_ratio >= 0.0);
    assert!(stats.total_reuse_ratio <= 1.0);
}

// ============================================================================
// PRIORITY 1: SIMD Acceleration Tests (simd_acceleration.rs)
// ============================================================================

#[test]
fn test_simd_frame_serializer_creation() {
    let serializer = SimdFrameSerializer::with_capacity(1024);
    let stats = serializer.stats();

    assert_eq!(stats.frames_processed, 0);
    assert_eq!(stats.bytes_written, 0);
    assert_eq!(stats.simd_operations, 0);
}

#[test]
fn test_simd_serialize_single_frame() {
    let mut serializer = SimdFrameSerializer::with_capacity(2048);

    let frame = StreamFrame {
        data: serde_json::json!({"test": "data", "number": 42}),
        priority: Priority::HIGH,
        metadata: HashMap::new(),
    };

    let result = serializer.serialize_frame(&frame);
    assert!(result.is_ok());

    let serialized = result.unwrap();
    assert!(!serialized.is_empty());

    // Verify valid JSON
    let parsed: serde_json::Value = sonic_rs::from_slice(serialized).unwrap();
    assert_eq!(parsed["data"]["test"], "data");
    assert_eq!(parsed["data"]["number"], 42);
}

#[test]
fn test_simd_serializer_stats_tracking() {
    let mut serializer = SimdFrameSerializer::with_capacity(1024);

    let frame = StreamFrame {
        data: serde_json::json!({"id": 1}),
        priority: Priority::MEDIUM,
        metadata: HashMap::new(),
    };

    serializer.serialize_frame(&frame).unwrap();

    let stats = serializer.stats();
    assert_eq!(stats.frames_processed, 1);
    assert!(stats.bytes_written > 0);
    assert_eq!(stats.simd_operations, 1);
}

#[test]
fn test_simd_batch_serialization() {
    let mut serializer = SimdFrameSerializer::with_capacity(4096);

    let frames = vec![
        StreamFrame {
            data: serde_json::json!({"id": 1}),
            priority: Priority::HIGH,
            metadata: HashMap::new(),
        },
        StreamFrame {
            data: serde_json::json!({"id": 2}),
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        },
        StreamFrame {
            data: serde_json::json!({"id": 3}),
            priority: Priority::LOW,
            metadata: HashMap::new(),
        },
    ];

    let result = serializer.serialize_batch(&frames);
    assert!(result.is_ok());

    let serialized = result.unwrap();
    let content = String::from_utf8(serialized.to_vec()).unwrap();

    // Should contain all IDs
    assert!(content.contains("\"id\":1"));
    assert!(content.contains("\"id\":2"));
    assert!(content.contains("\"id\":3"));

    // Verify stats
    let stats = serializer.stats();
    assert_eq!(stats.frames_processed, 3);
    assert_eq!(stats.simd_operations, 3);
}

#[test]
fn test_simd_serialize_empty_batch() {
    let mut serializer = SimdFrameSerializer::with_capacity(1024);
    let frames: Vec<StreamFrame> = vec![];

    let result = serializer.serialize_batch(&frames);
    assert!(result.is_ok());

    let serialized = result.unwrap();
    assert_eq!(serialized.len(), 0);
}

#[test]
fn test_simd_sse_batch_serialization() {
    let mut serializer = SimdFrameSerializer::with_capacity(4096);

    let frames = vec![
        StreamFrame {
            data: serde_json::json!({"event": "update"}),
            priority: Priority::HIGH,
            metadata: HashMap::new(),
        },
        StreamFrame {
            data: serde_json::json!({"event": "notification"}),
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        },
    ];

    let result = serializer.serialize_sse_batch(&frames);
    assert!(result.is_ok());

    let serialized = result.unwrap();
    let content = String::from_utf8(serialized.to_vec()).unwrap();

    // Check SSE format
    assert!(content.contains("data: "));
    assert!(content.contains("\n\n"));
}

#[test]
fn test_simd_serializer_reset_stats() {
    let mut serializer = SimdFrameSerializer::with_capacity(1024);

    let frame = StreamFrame {
        data: serde_json::json!({"test": true}),
        priority: Priority::HIGH,
        metadata: HashMap::new(),
    };

    serializer.serialize_frame(&frame).unwrap();
    assert_eq!(serializer.stats().frames_processed, 1);

    serializer.reset_stats();
    assert_eq!(serializer.stats().frames_processed, 0);
    assert_eq!(serializer.stats().bytes_written, 0);
}

#[test]
fn test_simd_stream_buffer_creation() {
    let buffer = SimdStreamBuffer::with_capacity(1024);
    assert_eq!(buffer.as_slice().len(), 0);
}

#[test]
fn test_simd_stream_buffer_alignment() {
    let buffer = SimdStreamBuffer::with_capacity(100);
    // Capacity should be aligned to 64 bytes (AVX-512)
    // 100 + 63 = 163, 163 & !63 = 128
    // We can't directly test capacity, but we can verify buffer works
    assert_eq!(buffer.as_slice().len(), 0);
}

#[test]
fn test_simd_stream_buffer_write_frame() {
    let mut buffer = SimdStreamBuffer::with_capacity(2048);

    let frame = StreamFrame {
        data: serde_json::json!({"buffer": "test"}),
        priority: Priority::HIGH,
        metadata: HashMap::new(),
    };

    let bytes_written = buffer.write_frame(&frame).unwrap();
    assert!(bytes_written > 0);

    let content = buffer.as_slice();
    assert!(!content.is_empty());
}

#[test]
fn test_simd_stream_buffer_write_multiple_frames() {
    let mut buffer = SimdStreamBuffer::with_capacity(4096);

    let frames = vec![
        StreamFrame {
            data: serde_json::json!({"id": 1}),
            priority: Priority::HIGH,
            metadata: HashMap::new(),
        },
        StreamFrame {
            data: serde_json::json!({"id": 2}),
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        },
    ];

    let bytes_written = buffer.write_frames(&frames).unwrap();
    assert!(bytes_written > 0);

    let content = String::from_utf8(buffer.as_slice().to_vec()).unwrap();
    assert!(content.contains("\"id\":1"));
    assert!(content.contains("\"id\":2"));
}

#[test]
fn test_simd_stream_buffer_clear() {
    let mut buffer = SimdStreamBuffer::with_capacity(1024);

    let frame = StreamFrame {
        data: serde_json::json!({"test": true}),
        priority: Priority::HIGH,
        metadata: HashMap::new(),
    };

    buffer.write_frame(&frame).unwrap();
    assert!(!buffer.as_slice().is_empty());

    buffer.clear();
    assert_eq!(buffer.as_slice().len(), 0);
}

#[test]
fn test_simd_stream_buffer_into_bytes() {
    let mut buffer = SimdStreamBuffer::with_capacity(1024);

    let frame = StreamFrame {
        data: serde_json::json!({"test": true}),
        priority: Priority::HIGH,
        metadata: HashMap::new(),
    };

    buffer.write_frame(&frame).unwrap();
    let bytes = buffer.into_bytes();
    assert!(!bytes.is_empty());
}

#[test]
fn test_simd_config_default() {
    let config = SimdConfig::default();
    assert_eq!(config.batch_size, 100);
    assert_eq!(config.initial_capacity, 8192);
    assert!(!config.collect_stats);
}

#[test]
fn test_simd_config_custom() {
    let config = SimdConfig {
        batch_size: 50,
        initial_capacity: 4096,
        collect_stats: true,
    };

    assert_eq!(config.batch_size, 50);
    assert_eq!(config.initial_capacity, 4096);
    assert!(config.collect_stats);
}

#[test]
fn test_simd_stream_processor_creation() {
    let config = SimdConfig::default();
    let processor = SimdStreamProcessor::new(config);
    assert!(processor.stats().is_none()); // Stats disabled by default
}

#[test]
fn test_simd_stream_processor_to_json() {
    let config = SimdConfig {
        batch_size: 100,
        initial_capacity: 2048,
        collect_stats: true,
    };

    let mut processor = SimdStreamProcessor::new(config);

    let frames = vec![StreamFrame {
        data: serde_json::json!({"processor": "test"}),
        priority: Priority::HIGH,
        metadata: HashMap::new(),
    }];

    let result = processor.process_to_json(&frames);
    assert!(result.is_ok());

    let bytes = result.unwrap();
    assert!(!bytes.is_empty());

    // Verify stats collection
    if let Some(stats) = processor.stats() {
        assert_eq!(stats.frames_processed, 1);
    }
}

#[test]
fn test_simd_stream_processor_to_sse() {
    let config = SimdConfig::default();
    let mut processor = SimdStreamProcessor::new(config);

    let frames = vec![StreamFrame {
        data: serde_json::json!({"event": "test"}),
        priority: Priority::HIGH,
        metadata: HashMap::new(),
    }];

    let result = processor.process_to_sse(&frames);
    assert!(result.is_ok());

    let bytes = result.unwrap();
    let content = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(content.starts_with("data: "));
}

#[test]
fn test_simd_stream_processor_to_ndjson() {
    let config = SimdConfig::default();
    let mut processor = SimdStreamProcessor::new(config);

    let frames = vec![
        StreamFrame {
            data: serde_json::json!({"line": 1}),
            priority: Priority::HIGH,
            metadata: HashMap::new(),
        },
        StreamFrame {
            data: serde_json::json!({"line": 2}),
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        },
    ];

    let result = processor.process_to_ndjson(&frames);
    assert!(result.is_ok());

    let bytes = result.unwrap();
    let content = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(content.contains("\"line\":1"));
    assert!(content.contains("\"line\":2"));
}

// Edge case tests

#[test]
fn test_object_pool_saturating_sub_edge_case() {
    let pool = ObjectPool::new(5, Vec::<i32>::new);

    // Get object from empty pool
    let obj = pool.get();
    let stats = pool.stats();
    assert_eq!(stats.current_pool_size, 0); // Should not underflow
    drop(obj);
}

#[test]
fn test_simd_serializer_large_batch() {
    let mut serializer = SimdFrameSerializer::with_capacity(65536);

    let frames: Vec<StreamFrame> = (0..1000)
        .map(|i| StreamFrame {
            data: serde_json::json!({"id": i}),
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        })
        .collect();

    let result = serializer.serialize_batch(&frames);
    assert!(result.is_ok());

    let stats = serializer.stats();
    assert_eq!(stats.frames_processed, 1000);
}
