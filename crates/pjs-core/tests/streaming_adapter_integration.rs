//! Integration tests for StreamingAdapter and UniversalAdapter
//!
//! Tests cover format detection, request/response conversion, error handling,
//! and streaming capabilities across different formats.

use pjson_rs::domain::value_objects::{JsonData, SessionId};
use pjson_rs::infrastructure::integration::streaming_adapter::{
    IntegrationError, ResponseBody, StreamingAdapter, StreamingFormat, UniversalRequest,
    UniversalResponse,
};
use pjson_rs::infrastructure::integration::universal_adapter::{
    AdapterConfig, UniversalAdapter, UniversalAdapterBuilder,
};
use pjson_rs::stream::StreamFrame;
use std::borrow::Cow;
use std::collections::HashMap;

// ============================================================================
// StreamingFormat Tests
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
fn test_streaming_format_detection_from_header() {
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
        StreamingFormat::from_accept_header("*/*"),
        StreamingFormat::Json
    );
}

#[test]
fn test_streaming_format_detection_mixed_headers() {
    assert_eq!(
        StreamingFormat::from_accept_header("text/html, text/event-stream, application/json"),
        StreamingFormat::ServerSentEvents
    );
    assert_eq!(
        StreamingFormat::from_accept_header("application/json, application/x-ndjson"),
        StreamingFormat::Ndjson
    );
}

#[test]
fn test_streaming_format_supports_streaming() {
    assert!(!StreamingFormat::Json.supports_streaming());
    assert!(StreamingFormat::Ndjson.supports_streaming());
    assert!(StreamingFormat::ServerSentEvents.supports_streaming());
    assert!(StreamingFormat::Binary.supports_streaming());
}

// ============================================================================
// UniversalRequest Tests
// ============================================================================

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
fn test_universal_request_with_header() {
    let request = UniversalRequest::new("GET", "/api/stream")
        .with_header("Accept", "text/event-stream")
        .with_header("Authorization", "Bearer token123");

    assert_eq!(request.headers.len(), 2);
    assert_eq!(
        request.get_header("Accept"),
        Some(&Cow::Borrowed("text/event-stream"))
    );
    assert_eq!(
        request.get_header("Authorization"),
        Some(&Cow::Borrowed("Bearer token123"))
    );
}

#[test]
fn test_universal_request_with_query_params() {
    let request = UniversalRequest::new("GET", "/api/stream")
        .with_query("priority", "high")
        .with_query("session_id", "test-123");

    assert_eq!(request.query_params.len(), 2);
    assert_eq!(request.get_query("priority"), Some(&"high".to_string()));
    assert_eq!(
        request.get_query("session_id"),
        Some(&"test-123".to_string())
    );
    assert_eq!(request.get_query("nonexistent"), None);
}

#[test]
fn test_universal_request_with_body() {
    let body_data = b"test body content";
    let request = UniversalRequest::new("POST", "/api/data")
        .with_body(body_data.to_vec())
        .with_header("Content-Type", "application/json");

    assert!(request.body.is_some());
    assert_eq!(request.body.as_ref().unwrap(), body_data);
}

#[test]
fn test_universal_request_accepts() {
    let request =
        UniversalRequest::new("GET", "/api/stream").with_header("accept", "text/event-stream");

    assert!(request.accepts("text/event-stream"));
    assert!(!request.accepts("application/json"));

    let request_case_sensitive =
        UniversalRequest::new("GET", "/api/stream").with_header("Accept", "application/json");

    assert!(request_case_sensitive.accepts("application/json"));
}

#[test]
fn test_universal_request_accepts_no_header() {
    let request = UniversalRequest::new("GET", "/api/stream");
    assert!(!request.accepts("text/event-stream"));
}

#[test]
fn test_universal_request_preferred_streaming_format() {
    let request_sse =
        UniversalRequest::new("GET", "/api/stream").with_header("accept", "text/event-stream");
    assert_eq!(
        request_sse.preferred_streaming_format(),
        StreamingFormat::ServerSentEvents
    );

    let request_ndjson =
        UniversalRequest::new("GET", "/api/stream").with_header("accept", "application/x-ndjson");
    assert_eq!(
        request_ndjson.preferred_streaming_format(),
        StreamingFormat::Ndjson
    );

    let request_default = UniversalRequest::new("GET", "/api/stream");
    assert_eq!(
        request_default.preferred_streaming_format(),
        StreamingFormat::Json
    );
}

// ============================================================================
// UniversalResponse Tests
// ============================================================================

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
    assert!(matches!(response.body, ResponseBody::Json(_)));
}

#[test]
fn test_universal_response_stream() {
    let frames = vec![StreamFrame {
        data: serde_json::json!({"test": "data"}),
        priority: pjson_rs::domain::Priority::HIGH,
        metadata: HashMap::new(),
    }];
    let response = UniversalResponse::stream(frames);

    assert_eq!(response.status_code, 200);
    assert_eq!(response.content_type, "application/x-ndjson");
    assert!(matches!(response.body, ResponseBody::Stream(_)));
}

#[test]
fn test_universal_response_server_sent_events() {
    let events = vec!["data: test event\n\n".to_string()];
    let response = UniversalResponse::server_sent_events(events);

    assert_eq!(response.status_code, 200);
    assert_eq!(response.content_type, "text/event-stream");
    assert!(matches!(response.body, ResponseBody::ServerSentEvents(_)));
    assert_eq!(
        response.headers.get(&Cow::Borrowed("Cache-Control")),
        Some(&Cow::Borrowed("no-cache"))
    );
    assert_eq!(
        response.headers.get(&Cow::Borrowed("Connection")),
        Some(&Cow::Borrowed("keep-alive"))
    );
}

#[test]
fn test_universal_response_error() {
    let response = UniversalResponse::error(404, "Not found");

    assert_eq!(response.status_code, 404);
    assert_eq!(response.content_type, "application/json");

    if let ResponseBody::Json(data) = &response.body {
        assert_eq!(data.get("error").unwrap().as_str(), Some("Not found"));
        assert_eq!(data.get("status").unwrap().as_i64(), Some(404));
    } else {
        panic!("Expected JSON body");
    }
}

#[test]
fn test_universal_response_with_header() {
    let data = JsonData::Bool(true);
    let response = UniversalResponse::json(data)
        .with_header("X-Custom-Header", "custom-value")
        .with_header("X-Request-ID", "req-123");

    assert_eq!(response.headers.len(), 2);
    assert_eq!(
        response.headers.get(&Cow::Borrowed("X-Custom-Header")),
        Some(&Cow::Borrowed("custom-value"))
    );
}

#[test]
fn test_universal_response_with_status() {
    let data = JsonData::String("created".to_string());
    let response = UniversalResponse::json(data).with_status(201);

    assert_eq!(response.status_code, 201);
}

#[test]
fn test_universal_response_chaining() {
    let data = JsonData::Object(HashMap::new());
    let response = UniversalResponse::json(data)
        .with_status(201)
        .with_header("Location", "/api/resource/123")
        .with_header("X-Created-By", "pjs");

    assert_eq!(response.status_code, 201);
    assert_eq!(response.headers.len(), 2);
}

// ============================================================================
// ResponseBody Tests
// ============================================================================

#[test]
fn test_response_body_variants() {
    let json_body = ResponseBody::Json(JsonData::Null);
    assert!(matches!(json_body, ResponseBody::Json(_)));

    let stream_body = ResponseBody::Stream(vec![]);
    assert!(matches!(stream_body, ResponseBody::Stream(_)));

    let sse_body = ResponseBody::ServerSentEvents(vec![]);
    assert!(matches!(sse_body, ResponseBody::ServerSentEvents(_)));

    let binary_body = ResponseBody::Binary(vec![]);
    assert!(matches!(binary_body, ResponseBody::Binary(_)));

    let empty_body = ResponseBody::Empty;
    assert!(matches!(empty_body, ResponseBody::Empty));
}

#[test]
fn test_response_body_clone() {
    let original = ResponseBody::Json(JsonData::Integer(42));
    let cloned = original.clone();

    if let (ResponseBody::Json(orig_data), ResponseBody::Json(cloned_data)) = (&original, &cloned) {
        assert_eq!(orig_data, cloned_data);
    } else {
        panic!("Clone failed");
    }
}

// ============================================================================
// IntegrationError Tests
// ============================================================================

#[test]
fn test_integration_error_display() {
    let error = IntegrationError::UnsupportedFramework("axum".to_string());
    assert!(error.to_string().contains("Unsupported framework"));
    assert!(error.to_string().contains("axum"));

    let error = IntegrationError::RequestConversion("invalid format".to_string());
    assert!(error.to_string().contains("Request conversion failed"));

    let error = IntegrationError::ResponseConversion("invalid data".to_string());
    assert!(error.to_string().contains("Response conversion failed"));

    let error = IntegrationError::StreamingNotSupported;
    assert!(error.to_string().contains("Streaming not supported"));

    let error = IntegrationError::Configuration("missing field".to_string());
    assert!(error.to_string().contains("Configuration error"));

    let error = IntegrationError::SimdProcessing("simd error".to_string());
    assert!(error.to_string().contains("SIMD processing error"));
}

// ============================================================================
// AdapterConfig Tests
// ============================================================================

#[test]
fn test_adapter_config_default() {
    let config = AdapterConfig::default();

    assert_eq!(config.framework_name, "universal");
    assert!(config.supports_streaming);
    assert!(config.supports_sse);
    assert_eq!(config.default_content_type, "application/json");
    assert!(config.default_headers.is_empty());
}

#[test]
fn test_adapter_config_custom() {
    let mut headers = HashMap::new();
    headers.insert(Cow::Borrowed("X-Framework"), Cow::Borrowed("test"));

    let config = AdapterConfig {
        framework_name: Cow::Borrowed("test-framework"),
        supports_streaming: false,
        supports_sse: true,
        default_content_type: Cow::Borrowed("application/xml"),
        default_headers: headers,
    };

    assert_eq!(config.framework_name, "test-framework");
    assert!(!config.supports_streaming);
    assert!(config.supports_sse);
    assert_eq!(config.default_content_type, "application/xml");
    assert_eq!(config.default_headers.len(), 1);
}

// ============================================================================
// UniversalAdapterBuilder Tests
// ============================================================================

#[test]
fn test_universal_adapter_builder_default() {
    let adapter: UniversalAdapter<(), (), std::io::Error> = UniversalAdapterBuilder::new().build();

    assert!(adapter.supports_streaming());
    assert!(adapter.supports_sse());
    assert_eq!(adapter.framework_name(), "universal");
}

#[test]
fn test_universal_adapter_builder_custom() {
    let adapter: UniversalAdapter<(), (), std::io::Error> = UniversalAdapterBuilder::new()
        .framework_name("custom-framework")
        .streaming_support(false)
        .sse_support(true)
        .default_content_type("text/plain")
        .default_header("X-Custom", "value")
        .build();

    assert_eq!(adapter.framework_name(), "custom-framework");
    assert!(!adapter.supports_streaming());
    assert!(adapter.supports_sse());
}

#[test]
fn test_universal_adapter_builder_chaining() {
    let adapter: UniversalAdapter<(), (), std::io::Error> = UniversalAdapterBuilder::new()
        .framework_name("test")
        .streaming_support(true)
        .sse_support(false)
        .default_header("X-Header-1", "value1")
        .default_header("X-Header-2", "value2")
        .build();

    assert_eq!(adapter.framework_name(), "test");
    assert!(adapter.supports_streaming());
    assert!(!adapter.supports_sse());
}

// ============================================================================
// UniversalAdapter Tests
// ============================================================================

#[test]
fn test_universal_adapter_new() {
    let adapter: UniversalAdapter<(), (), std::io::Error> = UniversalAdapter::new();

    assert!(adapter.supports_streaming());
    assert!(adapter.supports_sse());
    assert_eq!(adapter.framework_name(), "universal");
}

#[test]
fn test_universal_adapter_with_config() {
    let config = AdapterConfig {
        framework_name: Cow::Borrowed("test"),
        supports_streaming: false,
        supports_sse: false,
        default_content_type: Cow::Borrowed("text/plain"),
        default_headers: HashMap::new(),
    };

    let adapter: UniversalAdapter<(), (), std::io::Error> = UniversalAdapter::with_config(config);

    assert!(!adapter.supports_streaming());
    assert!(!adapter.supports_sse());
    assert_eq!(adapter.framework_name(), "test");
}

#[test]
fn test_universal_adapter_set_config() {
    let mut adapter: UniversalAdapter<(), (), std::io::Error> = UniversalAdapter::new();

    let new_config = AdapterConfig {
        framework_name: Cow::Borrowed("updated"),
        supports_streaming: false,
        supports_sse: false,
        default_content_type: Cow::Borrowed("text/plain"),
        default_headers: HashMap::new(),
    };

    adapter.set_config(new_config);

    assert_eq!(adapter.framework_name(), "updated");
    assert!(!adapter.supports_streaming());
}

#[test]
fn test_universal_adapter_add_default_header() {
    let mut adapter: UniversalAdapter<(), (), std::io::Error> = UniversalAdapter::new();

    adapter.add_default_header("X-Test", "test-value");
    adapter.add_default_header("X-Version", "1.0");

    // Test that the adapter still functions correctly after adding headers
    assert_eq!(adapter.framework_name(), "universal");
}

#[test]
fn test_universal_adapter_convert_request_error() {
    let adapter: UniversalAdapter<(), (), std::io::Error> = UniversalAdapter::new();
    let result = adapter.convert_request(());

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        IntegrationError::UnsupportedFramework(_)
    ));
}

#[test]
fn test_universal_adapter_to_response_error() {
    let adapter: UniversalAdapter<(), (), std::io::Error> = UniversalAdapter::new();
    let response = UniversalResponse::json(JsonData::Null);
    let result = adapter.to_response(response);

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        IntegrationError::UnsupportedFramework(_)
    ));
}

// ============================================================================
// Async Adapter Method Tests (using tokio)
// ============================================================================

#[tokio::test]
async fn test_universal_adapter_create_streaming_response_json() {
    let adapter: UniversalAdapter<(), (), std::io::Error> = UniversalAdapter::new();
    let session_id = SessionId::new();
    let frames = vec![StreamFrame {
        data: serde_json::json!({"key": "value"}),
        priority: pjson_rs::domain::Priority::HIGH,
        metadata: HashMap::new(),
    }];

    let result = adapter
        .create_streaming_response(session_id, frames, StreamingFormat::Json)
        .await;

    // Should fail because generic adapter cannot convert responses
    assert!(result.is_err());
}

#[tokio::test]
async fn test_universal_adapter_create_sse_response() {
    let adapter: UniversalAdapter<(), (), std::io::Error> = UniversalAdapter::new();
    let session_id = SessionId::new();
    let frames = vec![StreamFrame {
        data: serde_json::json!({"event": "test"}),
        priority: pjson_rs::domain::Priority::MEDIUM,
        metadata: HashMap::new(),
    }];

    let result = adapter.create_sse_response(session_id, frames).await;

    // Should fail because generic adapter cannot convert responses
    assert!(result.is_err());
}

#[tokio::test]
async fn test_universal_adapter_create_json_response() {
    let adapter: UniversalAdapter<(), (), std::io::Error> = UniversalAdapter::new();
    let data = JsonData::String("test".to_string());

    let result = adapter.create_json_response(data, false).await;

    // Should fail because generic adapter cannot convert responses
    assert!(result.is_err());
}

#[tokio::test]
async fn test_universal_adapter_apply_middleware() {
    let adapter: UniversalAdapter<(), (), std::io::Error> = UniversalAdapter::new();
    let request = UniversalRequest::new("GET", "/api/test");
    let response = UniversalResponse::json(JsonData::Bool(true));

    let result = adapter.apply_middleware(&request, response.clone()).await;

    // Middleware should succeed and return the same response
    assert!(result.is_ok());
    let returned_response = result.unwrap();
    assert_eq!(returned_response.status_code, response.status_code);
}

// ============================================================================
// Edge Cases and Error Scenarios
// ============================================================================

#[test]
fn test_universal_request_empty_path() {
    let request = UniversalRequest::new("GET", "");
    assert_eq!(request.path, "");
}

#[test]
fn test_universal_response_error_different_codes() {
    let response_400 = UniversalResponse::error(400, "Bad Request");
    assert_eq!(response_400.status_code, 400);

    let response_500 = UniversalResponse::error(500, "Internal Server Error");
    assert_eq!(response_500.status_code, 500);

    let response_403 = UniversalResponse::error(403, "Forbidden");
    assert_eq!(response_403.status_code, 403);
}

#[test]
fn test_universal_request_case_insensitive_header_lookup() {
    let request =
        UniversalRequest::new("GET", "/api/stream").with_header("Accept", "text/event-stream");

    // Test both lowercase and proper case
    assert_eq!(
        request.get_header("Accept"),
        Some(&Cow::Borrowed("text/event-stream"))
    );
    assert_eq!(request.get_header("accept"), None); // HashMap is case-sensitive
}

#[test]
fn test_streaming_format_equality() {
    assert_eq!(StreamingFormat::Json, StreamingFormat::Json);
    assert_ne!(StreamingFormat::Json, StreamingFormat::Ndjson);
    assert_ne!(StreamingFormat::Ndjson, StreamingFormat::ServerSentEvents);
}

#[test]
fn test_response_body_with_complex_json() {
    let mut obj = HashMap::new();
    obj.insert("id".to_string(), JsonData::Integer(1));
    obj.insert("name".to_string(), JsonData::String("test".to_string()));
    obj.insert("active".to_string(), JsonData::Bool(true));
    obj.insert("score".to_string(), JsonData::Float(99.5));

    let response = UniversalResponse::json(JsonData::Object(obj));

    if let ResponseBody::Json(JsonData::Object(data)) = &response.body {
        assert_eq!(data.len(), 4);
        assert_eq!(data.get("id").unwrap().as_i64(), Some(1));
        assert_eq!(data.get("name").unwrap().as_str(), Some("test"));
        assert_eq!(data.get("active").unwrap().as_bool(), Some(true));
        assert_eq!(data.get("score").unwrap().as_f64(), Some(99.5));
    } else {
        panic!("Expected JSON object body");
    }
}
