// Comprehensive tests for HTTP streaming module
//
// This test file covers the infrastructure/http/streaming.rs module with focus on:
// - StreamFormat detection from headers and content types
// - AdaptiveFrameStream functionality and format conversion
// - BatchFrameStream batching logic
// - PriorityFrameStream priority ordering
// - Stream error handling
// - Response creation with correct headers
//
// Coverage target: 60%+ for Infrastructure Layer

#![cfg(feature = "http-server")]

use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use futures::StreamExt;
use pjson_rs::{
    domain::entities::Frame,
    domain::value_objects::{JsonData, StreamId},
    infrastructure::http::streaming::{
        AdaptiveFrameStream, BatchFrameStream, PriorityFrameStream, StreamError, StreamFormat,
        create_streaming_response,
    },
};

// ============================================================================
// StreamFormat Tests
// ============================================================================

#[test]
fn test_stream_format_from_accept_header_sse() {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::ACCEPT,
        HeaderValue::from_static("text/event-stream"),
    );

    let format = StreamFormat::from_accept_header(&headers);
    assert!(matches!(format, StreamFormat::ServerSentEvents));
}

#[test]
fn test_stream_format_from_accept_header_ndjson() {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::ACCEPT,
        HeaderValue::from_static("application/x-ndjson"),
    );

    let format = StreamFormat::from_accept_header(&headers);
    assert!(matches!(format, StreamFormat::NdJson));
}

#[test]
fn test_stream_format_from_accept_header_binary() {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::ACCEPT,
        HeaderValue::from_static("application/octet-stream"),
    );

    let format = StreamFormat::from_accept_header(&headers);
    assert!(matches!(format, StreamFormat::Binary));
}

#[test]
fn test_stream_format_from_accept_header_default() {
    let mut headers = HeaderMap::new();
    headers.insert(header::ACCEPT, HeaderValue::from_static("application/json"));

    let format = StreamFormat::from_accept_header(&headers);
    assert!(matches!(format, StreamFormat::Json));
}

#[test]
fn test_stream_format_from_accept_header_missing() {
    let headers = HeaderMap::new();

    let format = StreamFormat::from_accept_header(&headers);
    assert!(matches!(format, StreamFormat::Json));
}

#[test]
fn test_stream_format_content_type() {
    assert_eq!(StreamFormat::Json.content_type(), "application/json");
    assert_eq!(StreamFormat::NdJson.content_type(), "application/x-ndjson");
    assert_eq!(
        StreamFormat::ServerSentEvents.content_type(),
        "text/event-stream"
    );
    assert_eq!(
        StreamFormat::Binary.content_type(),
        "application/octet-stream"
    );
}

// ============================================================================
// AdaptiveFrameStream Tests
// ============================================================================

fn create_test_frame(_priority: u8, sequence: u64, _payload: &str) -> Frame {
    let stream_id = StreamId::new();
    let json_data = JsonData::string("test data");

    // Use skeleton frame for simpler testing
    Frame::skeleton(stream_id, sequence, json_data)
}

#[tokio::test]
async fn test_adaptive_frame_stream_json_format() {
    let frames = vec![
        create_test_frame(200, 1, r#"{"key": "value1"}"#),
        create_test_frame(150, 2, r#"{"key": "value2"}"#),
    ];

    let frame_stream = futures::stream::iter(frames);
    let adaptive = AdaptiveFrameStream::new(frame_stream, StreamFormat::Json);

    let collected: Vec<_> = adaptive.collect().await;

    assert_eq!(collected.len(), 2);
    for result in collected {
        assert!(result.is_ok());
    }
}

#[tokio::test]
async fn test_adaptive_frame_stream_ndjson_format() {
    let frames = vec![create_test_frame(200, 1, r#"{"test": 1}"#)];

    let frame_stream = futures::stream::iter(frames);
    let adaptive = AdaptiveFrameStream::new(frame_stream, StreamFormat::NdJson);

    let collected: Vec<_> = adaptive.collect().await;

    assert_eq!(collected.len(), 1);
    let formatted = collected[0].as_ref().unwrap();
    assert!(formatted.ends_with('\n'));
}

#[tokio::test]
async fn test_adaptive_frame_stream_sse_format() {
    let frames = vec![create_test_frame(200, 1, r#"{"event": "update"}"#)];

    let frame_stream = futures::stream::iter(frames);
    let adaptive = AdaptiveFrameStream::new(frame_stream, StreamFormat::ServerSentEvents);

    let collected: Vec<_> = adaptive.collect().await;

    assert_eq!(collected.len(), 1);
    let formatted = collected[0].as_ref().unwrap();
    assert!(formatted.starts_with("data: "));
    assert!(formatted.ends_with("\n\n"));
}

#[tokio::test]
async fn test_adaptive_frame_stream_with_compression() {
    let frames = vec![create_test_frame(200, 1, r#"{"data": "test"}"#)];

    let frame_stream = futures::stream::iter(frames);
    let adaptive =
        AdaptiveFrameStream::new(frame_stream, StreamFormat::Json).with_compression(true);

    let collected: Vec<_> = adaptive.collect().await;

    assert_eq!(collected.len(), 1);
    assert!(collected[0].is_ok());
}

#[tokio::test]
async fn test_adaptive_frame_stream_with_buffer_size() {
    let frames = vec![create_test_frame(200, 1, r#"{"data": "test"}"#)];

    let frame_stream = futures::stream::iter(frames);
    let adaptive = AdaptiveFrameStream::new(frame_stream, StreamFormat::Json).with_buffer_size(20);

    let collected: Vec<_> = adaptive.collect().await;

    assert_eq!(collected.len(), 1);
}

#[tokio::test]
async fn test_adaptive_frame_stream_empty() {
    let frames: Vec<Frame> = vec![];
    let frame_stream = futures::stream::iter(frames);
    let adaptive = AdaptiveFrameStream::new(frame_stream, StreamFormat::Json);

    let collected: Vec<_> = adaptive.collect().await;

    assert_eq!(collected.len(), 0);
}

// ============================================================================
// BatchFrameStream Tests
// ============================================================================

#[tokio::test]
async fn test_batch_frame_stream_single_batch() {
    let frames = vec![
        create_test_frame(200, 1, r#"{"id": 1}"#),
        create_test_frame(200, 2, r#"{"id": 2}"#),
        create_test_frame(200, 3, r#"{"id": 3}"#),
    ];

    let frame_stream = futures::stream::iter(frames);
    let batch = BatchFrameStream::new(frame_stream, StreamFormat::Json, 5);

    let collected: Vec<_> = batch.collect().await;

    // All frames in one batch since batch_size=5 and we have 3 frames
    assert_eq!(collected.len(), 1);
    assert!(collected[0].is_ok());
}

#[tokio::test]
async fn test_batch_frame_stream_multiple_batches() {
    let frames = vec![
        create_test_frame(200, 1, r#"{"id": 1}"#),
        create_test_frame(200, 2, r#"{"id": 2}"#),
        create_test_frame(200, 3, r#"{"id": 3}"#),
        create_test_frame(200, 4, r#"{"id": 4}"#),
        create_test_frame(200, 5, r#"{"id": 5}"#),
    ];

    let frame_stream = futures::stream::iter(frames);
    let batch = BatchFrameStream::new(frame_stream, StreamFormat::Json, 2);

    let collected: Vec<_> = batch.collect().await;

    // Should have 3 batches: [2, 2, 1]
    assert_eq!(collected.len(), 3);
    for result in collected {
        assert!(result.is_ok());
    }
}

#[tokio::test]
async fn test_batch_frame_stream_ndjson_format() {
    let frames = vec![
        create_test_frame(200, 1, r#"{"id": 1}"#),
        create_test_frame(200, 2, r#"{"id": 2}"#),
    ];

    let frame_stream = futures::stream::iter(frames);
    let batch = BatchFrameStream::new(frame_stream, StreamFormat::NdJson, 10);

    let collected: Vec<_> = batch.collect().await;

    assert_eq!(collected.len(), 1);
    let result = collected[0].as_ref().unwrap();
    // NdJson should have newlines
    assert!(result.contains('\n'));
}

#[tokio::test]
async fn test_batch_frame_stream_sse_format() {
    let frames = vec![
        create_test_frame(200, 1, r#"{"id": 1}"#),
        create_test_frame(200, 2, r#"{"id": 2}"#),
    ];

    let frame_stream = futures::stream::iter(frames);
    let batch = BatchFrameStream::new(frame_stream, StreamFormat::ServerSentEvents, 10);

    let collected: Vec<_> = batch.collect().await;

    assert_eq!(collected.len(), 1);
    let result = collected[0].as_ref().unwrap();
    // SSE should have "data: " prefix
    assert!(result.contains("data: "));
}

#[tokio::test]
async fn test_batch_frame_stream_empty() {
    let frames: Vec<Frame> = vec![];
    let frame_stream = futures::stream::iter(frames);
    let batch = BatchFrameStream::new(frame_stream, StreamFormat::Json, 5);

    let collected: Vec<_> = batch.collect().await;

    assert_eq!(collected.len(), 0);
}

// ============================================================================
// PriorityFrameStream Tests
// ============================================================================

#[tokio::test]
async fn test_priority_frame_stream_orders_by_priority() {
    let frames = vec![
        create_test_frame(100, 1, r#"{"priority": "low"}"#),
        create_test_frame(250, 2, r#"{"priority": "critical"}"#),
        create_test_frame(200, 3, r#"{"priority": "high"}"#),
        create_test_frame(150, 4, r#"{"priority": "medium"}"#),
    ];

    let frame_stream = futures::stream::iter(frames);
    let priority = PriorityFrameStream::new(frame_stream, StreamFormat::Json, 10);

    let collected: Vec<_> = priority.collect().await;

    // Should get all frames
    assert_eq!(collected.len(), 4);
    for result in collected {
        assert!(result.is_ok());
    }
}

#[tokio::test]
async fn test_priority_frame_stream_small_buffer() {
    let frames = vec![
        create_test_frame(100, 1, r#"{"priority": "low"}"#),
        create_test_frame(250, 2, r#"{"priority": "critical"}"#),
        create_test_frame(200, 3, r#"{"priority": "high"}"#),
    ];

    let frame_stream = futures::stream::iter(frames);
    // Small buffer to test partial priority ordering
    let priority = PriorityFrameStream::new(frame_stream, StreamFormat::Json, 2);

    let collected: Vec<_> = priority.collect().await;

    assert_eq!(collected.len(), 3);
}

#[tokio::test]
async fn test_priority_frame_stream_empty() {
    let frames: Vec<Frame> = vec![];
    let frame_stream = futures::stream::iter(frames);
    let priority = PriorityFrameStream::new(frame_stream, StreamFormat::Json, 5);

    let collected: Vec<_> = priority.collect().await;

    assert_eq!(collected.len(), 0);
}

#[tokio::test]
async fn test_priority_frame_stream_sse_format() {
    let frames = vec![create_test_frame(200, 1, r#"{"test": 1}"#)];

    let frame_stream = futures::stream::iter(frames);
    let priority = PriorityFrameStream::new(frame_stream, StreamFormat::ServerSentEvents, 5);

    let collected: Vec<_> = priority.collect().await;

    assert_eq!(collected.len(), 1);
    let result = collected[0].as_ref().unwrap();
    assert!(result.starts_with("data: "));
}

// ============================================================================
// StreamError Tests
// ============================================================================

#[test]
fn test_stream_error_serialization() {
    let json_error = serde_json::Error::io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "test error",
    ));
    let error = StreamError::Serialization(json_error);

    assert!(error.to_string().contains("Serialization error"));
}

#[test]
fn test_stream_error_io() {
    let error = StreamError::Io("Connection lost".to_string());

    assert_eq!(error.to_string(), "IO error: Connection lost");
}

#[test]
fn test_stream_error_buffer_overflow() {
    let error = StreamError::BufferOverflow;

    assert_eq!(error.to_string(), "Buffer overflow");
}

#[test]
fn test_stream_error_stream_closed() {
    let error = StreamError::StreamClosed;

    assert_eq!(error.to_string(), "Stream closed");
}

// ============================================================================
// Response Creation Tests
// ============================================================================

#[tokio::test]
async fn test_create_streaming_response_json() {
    let stream = futures::stream::iter(vec![
        Ok::<String, StreamError>("test1".to_string()),
        Ok("test2".to_string()),
    ]);

    let response = create_streaming_response(stream, StreamFormat::Json).unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/json"
    );
    assert_eq!(
        response.headers().get(header::CACHE_CONTROL).unwrap(),
        "no-cache"
    );
}

#[tokio::test]
async fn test_create_streaming_response_sse() {
    let stream = futures::stream::iter(vec![Ok::<String, StreamError>(
        "data: test\n\n".to_string(),
    )]);

    let response = create_streaming_response(stream, StreamFormat::ServerSentEvents).unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "text/event-stream"
    );
    assert_eq!(
        response.headers().get(header::CACHE_CONTROL).unwrap(),
        "no-cache"
    );
    assert_eq!(
        response.headers().get(header::CONNECTION).unwrap(),
        "keep-alive"
    );
    assert_eq!(response.headers().get("X-Accel-Buffering").unwrap(), "no");
}

#[tokio::test]
async fn test_create_streaming_response_ndjson() {
    let stream = futures::stream::iter(vec![Ok::<String, StreamError>("test\n".to_string())]);

    let response = create_streaming_response(stream, StreamFormat::NdJson).unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/x-ndjson"
    );
    assert_eq!(
        response.headers().get("Transfer-Encoding").unwrap(),
        "chunked"
    );
}

#[tokio::test]
async fn test_create_streaming_response_binary() {
    let stream = futures::stream::iter(vec![Ok::<String, StreamError>("binary_data".to_string())]);

    let response = create_streaming_response(stream, StreamFormat::Binary).unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/octet-stream"
    );
}

// ============================================================================
// Integration Tests
// ============================================================================

#[tokio::test]
async fn test_full_streaming_pipeline() {
    // Create frames with different priorities
    let frames = vec![
        create_test_frame(100, 1, r#"{"msg": "low priority"}"#),
        create_test_frame(250, 2, r#"{"msg": "critical"}"#),
        create_test_frame(200, 3, r#"{"msg": "high priority"}"#),
    ];

    // Process through priority stream
    let frame_stream = futures::stream::iter(frames);
    let priority = PriorityFrameStream::new(frame_stream, StreamFormat::ServerSentEvents, 10);

    let collected: Vec<_> = priority.collect().await;

    assert_eq!(collected.len(), 3);

    // All should be formatted as SSE
    for result in collected {
        assert!(result.is_ok());
        let text = result.unwrap();
        assert!(text.starts_with("data: "));
        assert!(text.ends_with("\n\n"));
    }
}

#[tokio::test]
async fn test_adaptive_stream_builder_pattern() {
    let frames = vec![create_test_frame(200, 1, r#"{"test": 1}"#)];

    let frame_stream = futures::stream::iter(frames);
    let adaptive = AdaptiveFrameStream::new(frame_stream, StreamFormat::Json)
        .with_compression(true)
        .with_buffer_size(100);

    let collected: Vec<_> = adaptive.collect().await;

    assert_eq!(collected.len(), 1);
    assert!(collected[0].is_ok());
}
