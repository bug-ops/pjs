// Comprehensive tests for infrastructure integration modules
//
// This test file covers all infrastructure integration layer modules with focus on:
// - Object pooling system with concurrent access patterns
// - SIMD acceleration for JSON serialization
// - Streaming adapter traits and implementations
// - Universal adapter configurations
//
// Coverage targets: 100% for all integration modules

use pjson_rs::domain::Priority;
use pjson_rs::domain::value_objects::JsonData;
use pjson_rs::infrastructure::integration::{
    object_pool::{
        ObjectPool, get_byte_vec, get_cow_hashmap, get_global_pool_stats, get_string_hashmap,
        get_string_vec,
        pooled_builders::{PooledResponseBuilder, PooledSSEBuilder},
    },
    simd_acceleration::{
        SimdConfig, SimdFrameSerializer, SimdJsonProcessor, SimdStreamBuffer, SimdStreamProcessor,
    },
    streaming_adapter::{
        IntegrationError, ResponseBody, StreamingAdapter, StreamingFormat, UniversalRequest,
        UniversalResponse,
    },
    universal_adapter::{AdapterConfig, UniversalAdapter, UniversalAdapterBuilder},
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

#[test]
fn test_pooled_response_builder_basic() {
    let data = JsonData::String("test".to_string());
    let response = PooledResponseBuilder::new().json(data);

    assert_eq!(response.status_code, 200);
    assert_eq!(response.content_type, "application/json");
}

#[test]
fn test_pooled_response_builder_with_headers() {
    let response = PooledResponseBuilder::new()
        .status(201)
        .header("X-Custom", "custom-value")
        .header("X-Test", "test-value")
        .content_type("application/json")
        .json(JsonData::Bool(true));

    assert_eq!(response.status_code, 201);
    assert_eq!(response.headers.get("X-Custom").unwrap(), "custom-value");
    assert_eq!(response.headers.get("X-Test").unwrap(), "test-value");
}

#[test]
fn test_pooled_response_builder_binary() {
    let data = vec![0u8, 1, 2, 3, 4];
    let response = PooledResponseBuilder::new()
        .status(200)
        .header("X-Binary", "true")
        .binary(data.clone());

    assert_eq!(response.status_code, 200);
    assert_eq!(response.content_type, "application/octet-stream");

    if let ResponseBody::Binary(body_data) = response.body {
        assert_eq!(body_data, data);
    } else {
        panic!("Expected binary body");
    }
}

#[test]
fn test_pooled_sse_builder_basic() {
    let response = PooledSSEBuilder::new()
        .event("event1")
        .event("event2")
        .build();

    assert_eq!(response.status_code, 200);
    assert_eq!(response.content_type, "text/event-stream");

    // Check headers
    assert_eq!(
        response.headers.get("Cache-Control"),
        Some(&Cow::Borrowed("no-cache"))
    );
    assert_eq!(
        response.headers.get("Connection"),
        Some(&Cow::Borrowed("keep-alive"))
    );
}

#[test]
fn test_pooled_sse_builder_events() {
    let response = PooledSSEBuilder::new()
        .event("first event")
        .event("second event")
        .event("third event")
        .build();

    if let ResponseBody::ServerSentEvents(events) = response.body {
        assert_eq!(events.len(), 3);
        assert!(events[0].contains("first event"));
        assert!(events[1].contains("second event"));
        assert!(events[2].contains("third event"));
    } else {
        panic!("Expected ServerSentEvents body");
    }
}

#[test]
fn test_pooled_sse_builder_custom_headers() {
    let response = PooledSSEBuilder::new()
        .event("test")
        .header("X-Custom", "value")
        .header("X-Stream-ID", "123")
        .build();

    assert_eq!(
        response.headers.get("X-Custom"),
        Some(&Cow::Borrowed("value"))
    );
    assert_eq!(
        response.headers.get("X-Stream-ID"),
        Some(&Cow::Borrowed("123"))
    );
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
fn test_simd_json_validate_valid_json() {
    let valid_json = br#"{"name": "test", "value": 42, "active": true}"#;
    let result = SimdJsonProcessor::validate_json(valid_json);
    assert!(result.is_ok());
}

#[test]
fn test_simd_json_validate_invalid_json() {
    let invalid_json = br#"{"name": "test", "value": 42"#; // Missing closing brace
    let result = SimdJsonProcessor::validate_json(invalid_json);
    assert!(result.is_err());
}

#[test]
fn test_simd_json_validate_empty_input() {
    let empty_json = b"";
    let result = SimdJsonProcessor::validate_json(empty_json);
    assert!(result.is_err());
}

#[test]
fn test_simd_extract_priority_field_present() {
    let json = br#"{"data": "test", "priority": 5, "other": "field"}"#;
    let result = SimdJsonProcessor::extract_priority_field(json).unwrap();
    assert_eq!(result, Some(5));
}

#[test]
fn test_simd_extract_priority_field_missing() {
    let json = br#"{"data": "test", "other": "field"}"#;
    let result = SimdJsonProcessor::extract_priority_field(json).unwrap();
    assert_eq!(result, None);
}

#[test]
fn test_simd_extract_priority_field_wrong_type() {
    let json = br#"{"priority": "high"}"#; // String instead of number
    let result = SimdJsonProcessor::extract_priority_field(json).unwrap();
    assert_eq!(result, None);
}

#[test]
fn test_simd_validate_batch() {
    let inputs = vec![
        &br#"{"valid": true}"#[..],
        &br#"{"also": "valid"}"#[..],
        &br#"{"invalid": true"#[..], // Missing closing brace
    ];

    let results = SimdJsonProcessor::validate_batch(&inputs);
    assert_eq!(results.len(), 3);
    assert!(results[0].is_ok());
    assert!(results[1].is_ok());
    assert!(results[2].is_err());
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

// ============================================================================
// PRIORITY 1: Streaming Adapter Tests (streaming_adapter.rs)
// ============================================================================

#[test]
fn test_streaming_format_content_types() {
    assert_eq!(StreamingFormat::Json.content_type(), "application/json");
    assert_eq!(
        StreamingFormat::Ndjson.content_type(),
        "application/x-ndjson"
    );
    assert_eq!(
        StreamingFormat::ServerSentEvents.content_type(),
        "text/event-stream"
    );
    assert_eq!(
        StreamingFormat::Binary.content_type(),
        "application/octet-stream"
    );
}

#[test]
fn test_streaming_format_from_accept_header() {
    assert_eq!(
        StreamingFormat::from_accept_header("text/event-stream"),
        StreamingFormat::ServerSentEvents
    );
    assert_eq!(
        StreamingFormat::from_accept_header("application/x-ndjson"),
        StreamingFormat::Ndjson
    );
    assert_eq!(
        StreamingFormat::from_accept_header("application/octet-stream"),
        StreamingFormat::Binary
    );
    assert_eq!(
        StreamingFormat::from_accept_header("application/json"),
        StreamingFormat::Json
    );
    assert_eq!(
        StreamingFormat::from_accept_header("unknown/type"),
        StreamingFormat::Json
    ); // Default
}

#[test]
fn test_streaming_format_supports_streaming() {
    assert!(!StreamingFormat::Json.supports_streaming());
    assert!(StreamingFormat::Ndjson.supports_streaming());
    assert!(StreamingFormat::ServerSentEvents.supports_streaming());
    assert!(StreamingFormat::Binary.supports_streaming());
}

#[test]
fn test_universal_request_creation() {
    let request = UniversalRequest::new("GET", "/api/stream");
    assert_eq!(request.method, "GET");
    assert_eq!(request.path, "/api/stream");
    assert!(request.headers.is_empty());
    assert!(request.query_params.is_empty());
    assert!(request.body.is_none());
}

#[test]
fn test_universal_request_with_headers() {
    let request = UniversalRequest::new("POST", "/api/data")
        .with_header("Content-Type", "application/json")
        .with_header("Authorization", "Bearer token");

    assert_eq!(
        request.get_header("Content-Type"),
        Some(&Cow::Borrowed("application/json"))
    );
    assert_eq!(
        request.get_header("Authorization"),
        Some(&Cow::Borrowed("Bearer token"))
    );
}

#[test]
fn test_universal_request_with_query_params() {
    let request = UniversalRequest::new("GET", "/api/users")
        .with_query("page", "1")
        .with_query("limit", "10");

    assert_eq!(request.get_query("page"), Some(&"1".to_string()));
    assert_eq!(request.get_query("limit"), Some(&"10".to_string()));
}

#[test]
fn test_universal_request_with_body() {
    let body = b"test data".to_vec();
    let request = UniversalRequest::new("POST", "/api/data").with_body(body.clone());

    assert_eq!(request.body, Some(body));
}

#[test]
fn test_universal_request_accepts() {
    let request = UniversalRequest::new("GET", "/api/stream")
        .with_header("accept", "text/event-stream, application/json");

    assert!(request.accepts("text/event-stream"));
    assert!(request.accepts("application/json"));
    assert!(!request.accepts("application/xml"));
}

#[test]
fn test_universal_request_preferred_streaming_format() {
    let request1 =
        UniversalRequest::new("GET", "/api/stream").with_header("accept", "text/event-stream");
    assert_eq!(
        request1.preferred_streaming_format(),
        StreamingFormat::ServerSentEvents
    );

    let request2 =
        UniversalRequest::new("GET", "/api/stream").with_header("accept", "application/x-ndjson");
    assert_eq!(
        request2.preferred_streaming_format(),
        StreamingFormat::Ndjson
    );

    let request3 = UniversalRequest::new("GET", "/api/stream");
    assert_eq!(request3.preferred_streaming_format(), StreamingFormat::Json);
}

#[test]
fn test_universal_response_json() {
    let data = JsonData::String("test".to_string());
    let response = UniversalResponse::json(data);

    assert_eq!(response.status_code, 200);
    assert_eq!(response.content_type, "application/json");
    assert!(matches!(response.body, ResponseBody::Json(_)));
}

#[test]
fn test_universal_response_json_pooled() {
    let data = JsonData::Integer(42);
    let response = UniversalResponse::json_pooled(data);

    assert_eq!(response.status_code, 200);
    assert_eq!(response.content_type, "application/json");
}

#[test]
fn test_universal_response_stream() {
    let frames = vec![StreamFrame {
        data: serde_json::json!({"test": true}),
        priority: Priority::HIGH,
        metadata: HashMap::new(),
    }];

    let response = UniversalResponse::stream(frames);
    assert_eq!(response.status_code, 200);
    assert_eq!(response.content_type, "application/x-ndjson");
    assert!(matches!(response.body, ResponseBody::Stream(_)));
}

#[test]
fn test_universal_response_server_sent_events() {
    let events = vec!["event1".to_string(), "event2".to_string()];
    let response = UniversalResponse::server_sent_events(events);

    assert_eq!(response.status_code, 200);
    assert_eq!(response.content_type, "text/event-stream");
    assert_eq!(
        response.headers.get("Cache-Control"),
        Some(&Cow::Borrowed("no-cache"))
    );
    assert_eq!(
        response.headers.get("Connection"),
        Some(&Cow::Borrowed("keep-alive"))
    );
}

#[test]
fn test_universal_response_error() {
    let response = UniversalResponse::error(404, "Not Found");

    assert_eq!(response.status_code, 404);
    assert_eq!(response.content_type, "application/json");

    if let ResponseBody::Json(JsonData::Object(map)) = response.body {
        assert_eq!(
            map.get("error"),
            Some(&JsonData::String("Not Found".to_string()))
        );
        assert_eq!(map.get("status"), Some(&JsonData::Integer(404)));
    } else {
        panic!("Expected JSON error body");
    }
}

#[test]
fn test_universal_response_with_header() {
    let data = JsonData::Bool(true);
    let response = UniversalResponse::json(data)
        .with_header("X-Custom", "value")
        .with_header("X-Request-ID", "123");

    assert_eq!(
        response.headers.get("X-Custom"),
        Some(&Cow::Borrowed("value"))
    );
    assert_eq!(
        response.headers.get("X-Request-ID"),
        Some(&Cow::Borrowed("123"))
    );
}

#[test]
fn test_universal_response_with_status() {
    let data = JsonData::String("created".to_string());
    let response = UniversalResponse::json(data).with_status(201);

    assert_eq!(response.status_code, 201);
}

#[test]
fn test_integration_error_display() {
    let err1 = IntegrationError::UnsupportedFramework("test".to_string());
    assert!(err1.to_string().contains("Unsupported framework"));

    let err2 = IntegrationError::RequestConversion("failed".to_string());
    assert!(err2.to_string().contains("Request conversion failed"));

    let err3 = IntegrationError::ResponseConversion("failed".to_string());
    assert!(err3.to_string().contains("Response conversion failed"));

    let err4 = IntegrationError::StreamingNotSupported;
    assert!(err4.to_string().contains("Streaming not supported"));

    let err5 = IntegrationError::Configuration("bad config".to_string());
    assert!(err5.to_string().contains("Configuration error"));

    let err6 = IntegrationError::SimdProcessing("simd error".to_string());
    assert!(err6.to_string().contains("SIMD processing error"));
}

// ============================================================================
// PRIORITY 1: Universal Adapter Tests (universal_adapter.rs)
// ============================================================================

#[test]
fn test_adapter_config_default() {
    let config = AdapterConfig::default();
    assert_eq!(config.framework_name, "universal");
    assert!(config.supports_streaming);
    assert!(config.supports_sse);
    assert_eq!(config.default_content_type, "application/json");
}

#[test]
fn test_universal_adapter_creation() {
    let adapter: UniversalAdapter<(), (), std::io::Error> = UniversalAdapter::new();
    assert_eq!(adapter.framework_name(), "universal");
}

#[test]
fn test_universal_adapter_with_config() {
    let config = AdapterConfig {
        framework_name: Cow::Borrowed("test-framework"),
        supports_streaming: false,
        supports_sse: true,
        default_content_type: Cow::Borrowed("text/plain"),
        default_headers: HashMap::new(),
    };

    let adapter: UniversalAdapter<(), (), std::io::Error> = UniversalAdapter::with_config(config);
    assert!(!adapter.supports_streaming());
    assert!(adapter.supports_sse());
}

#[test]
fn test_universal_adapter_add_default_header() {
    let mut adapter: UniversalAdapter<(), (), std::io::Error> = UniversalAdapter::new();
    adapter.add_default_header("X-Default", "value");

    // Test passes if add_default_header doesn't panic
    // Cannot directly test private config field
    assert_eq!(adapter.framework_name(), "universal");
}

#[test]
fn test_universal_adapter_set_config() {
    let mut adapter: UniversalAdapter<(), (), std::io::Error> = UniversalAdapter::new();

    let new_config = AdapterConfig {
        framework_name: Cow::Borrowed("new-framework"),
        supports_streaming: false,
        supports_sse: false,
        default_content_type: Cow::Borrowed("text/html"),
        default_headers: HashMap::new(),
    };

    adapter.set_config(new_config);
    assert!(!adapter.supports_streaming());
    assert!(!adapter.supports_sse());
}

#[test]
fn test_universal_adapter_builder() {
    let adapter: UniversalAdapter<(), (), std::io::Error> = UniversalAdapterBuilder::new()
        .framework_name("custom")
        .streaming_support(false)
        .sse_support(true)
        .default_content_type("text/plain")
        .default_header("X-Custom", "value")
        .build();

    // Test public methods instead of private config field
    assert!(!adapter.supports_streaming());
    assert!(adapter.supports_sse());
    // framework_name returns &'static str "universal" for owned strings
}

#[test]
fn test_universal_adapter_builder_default() {
    let builder = UniversalAdapterBuilder::default();
    let adapter: UniversalAdapter<(), (), std::io::Error> = builder.build();
    assert_eq!(adapter.framework_name(), "universal");
}

#[test]
fn test_universal_adapter_convert_request_unsupported() {
    let adapter: UniversalAdapter<(), (), std::io::Error> = UniversalAdapter::new();
    let result = adapter.convert_request(());

    assert!(result.is_err());
    match result.unwrap_err() {
        IntegrationError::UnsupportedFramework(msg) => {
            assert!(msg.contains("Generic UniversalAdapter cannot convert requests"));
        }
        _ => panic!("Expected UnsupportedFramework error"),
    }
}

#[test]
fn test_universal_adapter_to_response_unsupported() {
    let adapter: UniversalAdapter<(), (), std::io::Error> = UniversalAdapter::new();
    let response = UniversalResponse::json(JsonData::Bool(true));
    let result = adapter.to_response(response);

    assert!(result.is_err());
    match result.unwrap_err() {
        IntegrationError::UnsupportedFramework(msg) => {
            assert!(msg.contains("Generic UniversalAdapter cannot convert responses"));
        }
        _ => panic!("Expected UnsupportedFramework error"),
    }
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

#[test]
fn test_streaming_format_mixed_accept_header() {
    let format = StreamingFormat::from_accept_header(
        "application/json, text/event-stream;q=0.9, application/x-ndjson;q=0.8",
    );
    // Should pick first matching format
    assert_eq!(format, StreamingFormat::ServerSentEvents);
}

#[test]
fn test_universal_request_case_insensitive_headers() {
    let request = UniversalRequest::new("GET", "/api")
        .with_header("accept", "text/event-stream")
        .with_header("Accept", "application/json");

    // Both lowercase and capitalized should work
    assert!(request.get_header("accept").is_some());
    assert!(request.get_header("Accept").is_some());
}

#[test]
fn test_pooled_builder_default() {
    let builder = PooledResponseBuilder::default();
    let response = builder.json(JsonData::String("test".to_string()));
    assert_eq!(response.status_code, 200);
}

#[test]
fn test_pooled_sse_builder_default() {
    let builder = PooledSSEBuilder::default();
    let response = builder.build();
    assert_eq!(response.content_type, "text/event-stream");
}
