//! Integration tests for HTTP Axum endpoints
//!
//! Tests all 8 REST endpoints end-to-end using tower::ServiceExt::oneshot

#![feature(impl_trait_in_assoc_type)]
#![cfg(feature = "http-server")]

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use pjson_rs::{
    domain::value_objects::{SessionId, StreamId},
    infrastructure::http::axum_adapter::create_pjs_router,
};
use serde_json::Value as JsonValue;
use std::sync::Arc;
use tower::ServiceExt;

// ===== Session Endpoints Tests =====

#[tokio::test]
async fn test_create_session_success() {
    let state = common::create_test_app_state();
    let app = create_pjs_router().with_state(state);

    let request = Request::builder()
        .uri("/pjs/sessions")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"max_concurrent_streams":5,"timeout_seconds":1800}"#,
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: JsonValue = serde_json::from_slice(&body).unwrap();

    assert!(json.get("session_id").is_some());
    assert!(json.get("expires_at").is_some());
}

#[tokio::test]
async fn test_create_session_invalid_json() {
    let state = common::create_test_app_state();
    let app = create_pjs_router().with_state(state);

    let request = Request::builder()
        .uri("/pjs/sessions")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"invalid json{"#))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_create_session_with_defaults() {
    let state = common::create_test_app_state();
    let app = create_pjs_router().with_state(state);

    let request = Request::builder()
        .uri("/pjs/sessions")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{}"#))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_get_session_success() {
    let session = common::SessionBuilder::new().build();
    let session_id = session.id();

    let state = common::create_test_app_state_with_session(session);
    let app = create_pjs_router().with_state(state);

    let request = Request::builder()
        .uri(&format!("/pjs/sessions/{}", session_id))
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_get_session_not_found() {
    let state = common::create_test_app_state();
    let app = create_pjs_router().with_state(state);

    let non_existent_id = SessionId::new();

    let request = Request::builder()
        .uri(&format!("/pjs/sessions/{}", non_existent_id))
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn test_get_session_invalid_uuid() {
    let state = common::create_test_app_state();
    let app = create_pjs_router().with_state(state);

    let request = Request::builder()
        .uri("/pjs/sessions/invalid-uuid-format")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_session_health_success() {
    let session = common::SessionBuilder::new().build();
    let session_id = session.id();

    let state = common::create_test_app_state_with_session(session);
    let app = create_pjs_router().with_state(state);

    let request = Request::builder()
        .uri(&format!("/pjs/sessions/{}/health", session_id))
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: JsonValue = serde_json::from_slice(&body).unwrap();

    assert!(json.get("is_healthy").is_some());
    assert!(json.get("active_streams").is_some());
    assert!(json.get("failed_streams").is_some());
    assert!(json.get("is_expired").is_some());
}

#[tokio::test]
async fn test_session_health_not_found() {
    let state = common::create_test_app_state();
    let app = create_pjs_router().with_state(state);

    let non_existent_id = SessionId::new();

    let request = Request::builder()
        .uri(&format!("/pjs/sessions/{}/health", non_existent_id))
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn test_list_sessions_success() {
    let state = common::create_test_app_state();
    let app = create_pjs_router().with_state(state);

    let request = Request::builder()
        .uri("/pjs/sessions")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_list_sessions_with_pagination() {
    let state = common::create_test_app_state();
    let app = create_pjs_router().with_state(state);

    let request = Request::builder()
        .uri("/pjs/sessions?limit=10&offset=0")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

// ===== Stream Endpoints Tests =====

#[tokio::test]
async fn test_create_stream_success() {
    let session = common::SessionBuilder::new().build();
    let session_id = session.id();

    let state = common::create_test_app_state_with_session(session);
    let app = create_pjs_router().with_state(state);

    let request = Request::builder()
        .uri(&format!("/pjs/sessions/{}/streams", session_id))
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"data":{"test":"value"}}"#))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: JsonValue = serde_json::from_slice(&body).unwrap();

    assert!(json.get("stream_id").is_some());
    assert_eq!(
        json.get("status"),
        Some(&JsonValue::String("created".to_string()))
    );
}

#[tokio::test]
async fn test_create_stream_invalid_session() {
    let state = common::create_test_app_state();
    let app = create_pjs_router().with_state(state);

    let non_existent_id = SessionId::new();

    let request = Request::builder()
        .uri(&format!("/pjs/sessions/{}/streams", non_existent_id))
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"data":{"test":"value"}}"#))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn test_create_stream_invalid_json() {
    let session = common::SessionBuilder::new().build();
    let session_id = session.id();

    let state = common::create_test_app_state_with_session(session);
    let app = create_pjs_router().with_state(state);

    let request = Request::builder()
        .uri(&format!("/pjs/sessions/{}/streams", session_id))
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"invalid json{"#))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_start_stream_success() {
    let mut session = common::SessionBuilder::new().build();
    let session_id = session.id();

    let stream_id = session
        .create_stream(serde_json::json!({"test": "data"}).into())
        .unwrap();

    let repository = Arc::new(common::MockRepository::with_session(session));
    let event_publisher = Arc::new(common::MockEventPublisher::new());
    let stream_store = Arc::new(common::MockStreamStore::new());

    use pjson_rs::infrastructure::http::axum_adapter::PjsAppState;
    let state = PjsAppState::new(repository, event_publisher, stream_store);
    let app = create_pjs_router().with_state(state);

    let request = Request::builder()
        .uri(&format!(
            "/pjs/sessions/{}/streams/{}/start",
            session_id, stream_id
        ))
        .method("POST")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_start_stream_not_found() {
    let state = common::create_test_app_state();
    let app = create_pjs_router().with_state(state);

    let session_id = SessionId::new();
    let stream_id = StreamId::new();

    let request = Request::builder()
        .uri(&format!(
            "/pjs/sessions/{}/streams/{}/start",
            session_id, stream_id
        ))
        .method("POST")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn test_start_stream_invalid_stream_id() {
    let session = common::SessionBuilder::new().build();
    let session_id = session.id();

    let state = common::create_test_app_state_with_session(session);
    let app = create_pjs_router().with_state(state);

    let request = Request::builder()
        .uri(&format!(
            "/pjs/sessions/{}/streams/invalid-uuid/start",
            session_id
        ))
        .method("POST")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_get_stream_success() {
    let mut session = common::SessionBuilder::new().build();
    let session_id = session.id();

    let stream_id = session
        .create_stream(serde_json::json!({"test": "data"}).into())
        .unwrap();

    let stream = session.get_stream(stream_id).unwrap().clone();

    let repository = Arc::new(common::MockRepository::with_session(session));
    let event_publisher = Arc::new(common::MockEventPublisher::new());
    let stream_store = Arc::new(common::MockStreamStore::with_stream(stream));

    use pjson_rs::infrastructure::http::axum_adapter::PjsAppState;
    let state = PjsAppState::new(repository, event_publisher, stream_store);
    let app = create_pjs_router().with_state(state);

    let request = Request::builder()
        .uri(&format!(
            "/pjs/sessions/{}/streams/{}",
            session_id, stream_id
        ))
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_get_stream_not_found() {
    let state = common::create_test_app_state();
    let app = create_pjs_router().with_state(state);

    let session_id = SessionId::new();
    let stream_id = StreamId::new();

    let request = Request::builder()
        .uri(&format!(
            "/pjs/sessions/{}/streams/{}",
            session_id, stream_id
        ))
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

// ===== System Health Tests =====

#[tokio::test]
async fn test_system_health_success() {
    let state = common::create_test_app_state();
    let app = create_pjs_router().with_state(state);

    let request = Request::builder()
        .uri("/pjs/health")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: JsonValue = serde_json::from_slice(&body).unwrap();

    assert_eq!(
        json.get("status"),
        Some(&JsonValue::String("healthy".to_string()))
    );
    assert!(json.get("version").is_some());
    assert!(json.get("features").is_some());
}

#[tokio::test]
async fn test_system_health_has_correct_version() {
    let state = common::create_test_app_state();
    let app = create_pjs_router().with_state(state);

    let request = Request::builder()
        .uri("/pjs/health")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: JsonValue = serde_json::from_slice(&body).unwrap();

    let version = json.get("version").and_then(|v| v.as_str()).unwrap();
    assert!(!version.is_empty());
}

// ===== Error Handling Tests =====

#[tokio::test]
async fn test_invalid_session_id_returns_400() {
    let state = common::create_test_app_state();
    let app = create_pjs_router().with_state(state);

    let request = Request::builder()
        .uri("/pjs/sessions/not-a-valid-uuid")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: JsonValue = serde_json::from_slice(&body).unwrap();

    assert!(json.get("error").is_some());
}

#[tokio::test]
async fn test_invalid_stream_id_returns_400() {
    let session = common::SessionBuilder::new().build();
    let session_id = session.id();

    let state = common::create_test_app_state_with_session(session);
    let app = create_pjs_router().with_state(state);

    let request = Request::builder()
        .uri(&format!(
            "/pjs/sessions/{}/streams/not-a-valid-uuid",
            session_id
        ))
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

// ===== Response Headers Tests =====

#[tokio::test]
async fn test_cors_headers_present() {
    let state = common::create_test_app_state();
    let app = create_pjs_router().with_state(state);

    let request = Request::builder()
        .uri("/pjs/health")
        .method("GET")
        .header(header::ORIGIN, "http://localhost:3000")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert!(
        response
            .headers()
            .get("access-control-allow-origin")
            .is_some()
    );
}

#[tokio::test]
async fn test_security_headers_present() {
    let state = common::create_test_app_state();
    let app = create_pjs_router().with_state(state);

    let request = Request::builder()
        .uri("/pjs/health")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert!(response.headers().get("x-content-type-options").is_some());
    assert!(response.headers().get("x-frame-options").is_some());
    assert!(response.headers().get("content-security-policy").is_some());
}

#[tokio::test]
async fn test_content_type_json() {
    let state = common::create_test_app_state();
    let app = create_pjs_router().with_state(state);

    let request = Request::builder()
        .uri("/pjs/health")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap();

    assert!(content_type.contains("application/json"));
}
