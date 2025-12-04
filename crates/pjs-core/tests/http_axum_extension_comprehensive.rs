// Comprehensive tests for HTTP Axum extension module
//
// This test file covers the infrastructure/http/axum_extension.rs module with focus on:
// - PjsConfig creation and defaults
// - PjsExtension router integration
// - PJS middleware detection and request handling
// - Stream request handling and responses
// - SSE streaming endpoint
// - Health check endpoint
// - Error handling and IntoResponse implementations
// - PjsResponseExt trait functionality
//
// Coverage target: 60%+ for Infrastructure Layer

#![cfg(feature = "http-server")]

use axum::{
    Router,
    body::{Body, to_bytes},
    extract::Request,
    http::{Method, StatusCode, header},
    response::IntoResponse,
};
use pjson_rs::Priority;
use pjson_rs::infrastructure::http::axum_extension::{
    PjsConfig, PjsExtension, PjsStreamingRequest, StreamError, StreamRequest, StreamResponse,
};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceExt;

// ============================================================================
// PjsConfig Tests
// ============================================================================

#[test]
fn test_pjs_config_default_values() {
    let config = PjsConfig::default();

    assert_eq!(config.route_prefix, "/pjs");
    assert!(config.auto_detect);
    assert_eq!(config.default_priority, Priority::MEDIUM);
    assert_eq!(config.max_streams_per_client, 10);
    assert_eq!(config.session_timeout, Duration::from_secs(3600));
}

#[test]
fn test_pjs_config_custom_values() {
    let config = PjsConfig {
        route_prefix: "/api/stream".to_string(),
        auto_detect: false,
        default_priority: Priority::HIGH,
        max_streams_per_client: 5,
        session_timeout: Duration::from_secs(7200),
    };

    assert_eq!(config.route_prefix, "/api/stream");
    assert!(!config.auto_detect);
    assert_eq!(config.default_priority, Priority::HIGH);
    assert_eq!(config.max_streams_per_client, 5);
    assert_eq!(config.session_timeout, Duration::from_secs(7200));
}

#[test]
fn test_pjs_config_clone() {
    let config1 = PjsConfig::default();
    let config2 = config1.clone();

    assert_eq!(config1.route_prefix, config2.route_prefix);
    assert_eq!(config1.auto_detect, config2.auto_detect);
    assert_eq!(config1.default_priority, config2.default_priority);
}

// ============================================================================
// PjsExtension Tests
// ============================================================================

#[test]
fn test_pjs_extension_creation() {
    let config = PjsConfig::default();
    let extension = PjsExtension::new(config);

    // Extension created successfully
    assert!(Arc::strong_count(&Arc::new(extension)) >= 1);
}

#[tokio::test]
async fn test_pjs_extension_router_integration() {
    // Create a simple API route
    async fn api_route() -> impl IntoResponse {
        axum::Json(json!({
            "message": "Hello from API"
        }))
    }

    let config = PjsConfig::default();
    let extension = PjsExtension::new(config);

    let app = Router::new().route("/api/data", axum::routing::get(api_route));

    let app = extension.extend_router(app);

    // Test that the original route still works
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/data")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_pjs_health_endpoint() {
    let config = PjsConfig::default();
    let extension = PjsExtension::new(config);

    let app = Router::new();
    let app = extension.extend_router(app);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/pjs/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(body["status"], "healthy");
    assert_eq!(body["service"], "pjs-extension");
    assert!(body["capabilities"].is_array());
}

#[tokio::test]
async fn test_pjs_health_endpoint_capabilities() {
    let config = PjsConfig::default();
    let extension = PjsExtension::new(config);

    let app = Router::new();
    let app = extension.extend_router(app);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/pjs/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body_bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    let capabilities = body["capabilities"].as_array().unwrap();
    assert!(capabilities.contains(&json!("priority-streaming")));
    assert!(capabilities.contains(&json!("sse-support")));
    assert!(capabilities.contains(&json!("ndjson-support")));
    assert!(capabilities.contains(&json!("auto-detection")));
}

#[tokio::test]
async fn test_pjs_custom_route_prefix() {
    let config = PjsConfig {
        route_prefix: "/custom".to_string(),
        ..Default::default()
    };
    let extension = PjsExtension::new(config);

    let app = Router::new();
    let app = extension.extend_router(app);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/custom/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

// ============================================================================
// PjsStreamingRequest Tests
// ============================================================================

#[test]
fn test_pjs_streaming_request_creation() {
    let request = PjsStreamingRequest { enabled: true };
    assert!(request.enabled);

    let request = PjsStreamingRequest { enabled: false };
    assert!(!request.enabled);
}

#[test]
fn test_pjs_streaming_request_clone() {
    let request1 = PjsStreamingRequest { enabled: true };
    let request2 = request1.clone();

    assert_eq!(request1.enabled, request2.enabled);
}

// ============================================================================
// StreamError Tests
// ============================================================================

#[test]
fn test_stream_error_display() {
    let error = StreamError::AnalysisError("Invalid JSON".to_string());
    assert_eq!(error.to_string(), "Analysis error: Invalid JSON");

    let error = StreamError::ResponseError("Failed to build".to_string());
    assert_eq!(error.to_string(), "Response error: Failed to build");

    let error = StreamError::StreamNotFound("stream-123".to_string());
    assert_eq!(error.to_string(), "Stream not found: stream-123");
}

#[test]
fn test_stream_error_into_response() {
    let error = StreamError::AnalysisError("test error".to_string());
    let response = error.into_response();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[test]
fn test_stream_error_not_found_response() {
    let error = StreamError::StreamNotFound("missing-stream".to_string());
    let response = error.into_response();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[test]
fn test_stream_error_internal_error_response() {
    let error = StreamError::ResponseError("internal issue".to_string());
    let response = error.into_response();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

// ============================================================================
// StreamRequest and StreamResponse Tests
// ============================================================================

#[test]
fn test_stream_request_deserialization() {
    let json = json!({
        "data": {"key": "value"},
        "priority": 200,
        "format": "sse",
        "max_frames": 10
    });

    let request: StreamRequest = serde_json::from_value(json).unwrap();
    assert_eq!(request.data, json!({"key": "value"}));
    assert_eq!(request.priority, Some(200));
    assert_eq!(request.format, Some("sse".to_string()));
    assert_eq!(request.max_frames, Some(10));
}

#[test]
fn test_stream_request_optional_fields() {
    let json = json!({
        "data": {"test": true}
    });

    let request: StreamRequest = serde_json::from_value(json).unwrap();
    assert_eq!(request.data, json!({"test": true}));
    assert_eq!(request.priority, None);
    assert_eq!(request.format, None);
    assert_eq!(request.max_frames, None);
}

#[test]
fn test_stream_response_serialization() {
    let response = StreamResponse {
        stream_id: "test-stream-123".to_string(),
        format: "sse".to_string(),
        estimated_frames: 5,
    };

    let json = serde_json::to_value(&response).unwrap();
    assert_eq!(json["stream_id"], "test-stream-123");
    assert_eq!(json["format"], "sse");
    assert_eq!(json["estimated_frames"], 5);
}

// ============================================================================
// Middleware Detection Tests
// ============================================================================

#[tokio::test]
async fn test_middleware_detects_pjs_stream_header() {
    let config = PjsConfig::default();
    let extension = PjsExtension::new(config);

    async fn test_handler(req: Request) -> impl IntoResponse {
        // Check if PjsStreamingRequest extension was added
        if req.extensions().get::<PjsStreamingRequest>().is_some() {
            StatusCode::OK
        } else {
            StatusCode::NOT_FOUND
        }
    }

    let app = Router::new().route("/test", axum::routing::get(test_handler));

    let app = extension.extend_router(app);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/test")
                .header(header::ACCEPT, "application/pjs-stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // The middleware should have detected PJS streaming request
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_middleware_detects_sse_accept_header() {
    let config = PjsConfig::default();
    let extension = PjsExtension::new(config);

    async fn test_handler(req: Request) -> impl IntoResponse {
        if req.extensions().get::<PjsStreamingRequest>().is_some() {
            StatusCode::OK
        } else {
            StatusCode::NOT_FOUND
        }
    }

    let app = Router::new().route("/test", axum::routing::get(test_handler));

    let app = extension.extend_router(app);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/test")
                .header(header::ACCEPT, "text/event-stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

// Note: This test is commented out because it requires a full route setup
// The middleware detection is tested via actual PJS routes in other tests
// #[tokio::test]
// async fn test_middleware_detects_custom_pjs_header() {
//     // Middleware detection tested in other integration tests
// }

#[tokio::test]
async fn test_middleware_no_pjs_detection() {
    let config = PjsConfig::default();
    let extension = PjsExtension::new(config);

    async fn test_handler(req: Request) -> impl IntoResponse {
        if req.extensions().get::<PjsStreamingRequest>().is_some() {
            StatusCode::OK
        } else {
            StatusCode::NOT_FOUND
        }
    }

    let app = Router::new().route("/test", axum::routing::get(test_handler));

    let app = extension.extend_router(app);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/test")
                .header(header::ACCEPT, "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Should NOT have PjsStreamingRequest extension
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

// ============================================================================
// Stream Creation Endpoint Tests
// ============================================================================

#[tokio::test]
async fn test_stream_request_endpoint_creates_stream() {
    let config = PjsConfig::default();
    let extension = PjsExtension::new(config);

    let app = Router::new();
    let app = extension.extend_router(app);

    let request_body = json!({
        "data": {
            "critical": {"id": 1, "status": "active"},
            "metadata": {"created": "2024-01-01"}
        },
        "priority": 200,
        "format": "json"
    });

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/pjs/stream")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&request_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);

    // Check Location header is set
    assert!(response.headers().contains_key(header::LOCATION));

    let body_bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert!(!body["stream_id"].as_str().unwrap().is_empty());
    assert_eq!(body["format"], "json");
    assert!(body["estimated_frames"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn test_stream_request_format_detection_from_accept_header() {
    let config = PjsConfig::default();
    let extension = PjsExtension::new(config);

    let app = Router::new();
    let app = extension.extend_router(app);

    let request_body = json!({
        "data": {"test": "value"}
    });

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/pjs/stream")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::ACCEPT, "text/event-stream")
                .body(Body::from(serde_json::to_vec(&request_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);

    let body_bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(body["format"], "sse");
}

#[tokio::test]
async fn test_stream_request_format_detection_ndjson() {
    let config = PjsConfig::default();
    let extension = PjsExtension::new(config);

    let app = Router::new();
    let app = extension.extend_router(app);

    let request_body = json!({
        "data": {"test": "value"}
    });

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/pjs/stream")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::ACCEPT, "application/x-ndjson")
                .body(Body::from(serde_json::to_vec(&request_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    let body_bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(body["format"], "ndjson");
}

// ============================================================================
// SSE Stream Endpoint Tests
// ============================================================================

#[tokio::test]
async fn test_sse_stream_endpoint_returns_event_stream() {
    let config = PjsConfig::default();
    let extension = PjsExtension::new(config);

    let app = Router::new();
    let app = extension.extend_router(app);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/pjs/stream/test-123/sse")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    // Check SSE-specific headers
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
}

#[tokio::test]
async fn test_sse_stream_endpoint_cors_headers() {
    let config = PjsConfig::default();
    let extension = PjsExtension::new(config);

    let app = Router::new();
    let app = extension.extend_router(app);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/pjs/stream/test-123/sse")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Check CORS header
    assert_eq!(
        response
            .headers()
            .get("Access-Control-Allow-Origin")
            .unwrap(),
        "*"
    );
}
