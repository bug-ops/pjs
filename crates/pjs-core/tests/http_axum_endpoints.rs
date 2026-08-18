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
        .uri(format!("/pjs/sessions/{}", session_id))
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
        .uri(format!("/pjs/sessions/{}", non_existent_id))
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
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
        .uri(format!("/pjs/sessions/{}/health", session_id))
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
        .uri(format!("/pjs/sessions/{}/health", non_existent_id))
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
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
        .uri(format!("/pjs/sessions/{}/streams", session_id))
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
        .uri(format!("/pjs/sessions/{}/streams", non_existent_id))
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"data":{"test":"value"}}"#))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_create_stream_invalid_json() {
    let session = common::SessionBuilder::new().build();
    let session_id = session.id();

    let state = common::create_test_app_state_with_session(session);
    let app = create_pjs_router().with_state(state);

    let request = Request::builder()
        .uri(format!("/pjs/sessions/{}/streams", session_id))
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
        .uri(format!(
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
        .uri(format!(
            "/pjs/sessions/{}/streams/{}/start",
            session_id, stream_id
        ))
        .method("POST")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_start_stream_invalid_stream_id() {
    let session = common::SessionBuilder::new().build();
    let session_id = session.id();

    let state = common::create_test_app_state_with_session(session);
    let app = create_pjs_router().with_state(state);

    let request = Request::builder()
        .uri(format!(
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

    let stream = session.stream(stream_id).unwrap().clone();

    let repository = Arc::new(common::MockRepository::with_session(session));
    let event_publisher = Arc::new(common::MockEventPublisher::new());
    let stream_store = Arc::new(common::MockStreamStore::with_stream(stream));

    use pjson_rs::infrastructure::http::axum_adapter::PjsAppState;
    let state = PjsAppState::new(repository, event_publisher, stream_store);
    let app = create_pjs_router().with_state(state);

    let request = Request::builder()
        .uri(format!(
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
        .uri(format!(
            "/pjs/sessions/{}/streams/{}",
            session_id, stream_id
        ))
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
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
        .uri(format!(
            "/pjs/sessions/{}/streams/not-a-valid-uuid",
            session_id
        ))
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

// ===== New Endpoints: Session Stats and Stream Frames =====

#[tokio::test]
async fn test_get_session_stats_success() {
    let session = common::SessionBuilder::new().build();
    let session_id = session.id();

    let state = common::create_test_app_state_with_session(session);
    let app = create_pjs_router().with_state(state);

    let request = Request::builder()
        .uri(format!("/pjs/sessions/{}/stats", session_id))
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: JsonValue = serde_json::from_slice(&body).unwrap();

    assert!(json.get("session_id").is_some());
    assert!(json.get("stream_count").is_some());
    assert!(json.get("stats").is_some());
}

#[tokio::test]
async fn test_get_session_stats_not_found() {
    let state = common::create_test_app_state();
    let app = create_pjs_router().with_state(state);

    let request = Request::builder()
        .uri(format!("/pjs/sessions/{}/stats", SessionId::new()))
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_get_stream_frames_success() {
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
        .uri(format!(
            "/pjs/sessions/{}/streams/{}/frames",
            session_id, stream_id
        ))
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: JsonValue = serde_json::from_slice(&body).unwrap();

    assert!(json.get("frames").is_some());
    assert!(json.get("total_count").is_some());
    assert_eq!(json["total_count"], 0);
}

/// Regression test for #269: after `POST .../streams/{id}/generate-frames`
/// produces frames, `GET .../streams/{id}/frames` must return them instead of
/// always replying `{"frames":[],"total_count":0}`.
#[tokio::test]
async fn test_get_stream_frames_returns_persisted_frames() {
    let mut session = common::SessionBuilder::new().build();
    let session_id = session.id();
    let stream_id = session
        .create_stream(serde_json::json!({"items": [1, 2, 3, 4, 5, 6, 7, 8]}).into())
        .unwrap();
    session.start_stream(stream_id).unwrap();

    let repository = Arc::new(common::MockRepository::with_session(session));
    let event_publisher = Arc::new(common::MockEventPublisher::new());
    let stream_store = Arc::new(common::MockStreamStore::new());

    use pjson_rs::infrastructure::http::axum_adapter::PjsAppState;
    let state = PjsAppState::new(repository, event_publisher, stream_store);
    let app = create_pjs_router().with_state(state);

    // Generate frames via the command endpoint.
    let generate = Request::builder()
        .uri(format!(
            "/pjs/sessions/{}/streams/{}/generate-frames",
            session_id, stream_id
        ))
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"max_frames":4}"#))
        .unwrap();
    let response = app.clone().oneshot(generate).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let generated: JsonValue = serde_json::from_slice(&body).unwrap();
    let generated_count = generated["frame_count"].as_u64().unwrap();
    assert!(
        generated_count > 0,
        "command must produce at least one frame: {generated:?}",
    );

    // Now fetch them back through the query endpoint.
    let fetch = Request::builder()
        .uri(format!(
            "/pjs/sessions/{}/streams/{}/frames",
            session_id, stream_id
        ))
        .method("GET")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(fetch).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let fetched: JsonValue = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        fetched["total_count"].as_u64().unwrap(),
        generated_count,
        "frames endpoint must report every persisted frame: {fetched:?}",
    );
    assert_eq!(
        fetched["frames"].as_array().unwrap().len() as u64,
        generated_count,
    );
}

#[tokio::test]
async fn test_get_stream_frames_not_found() {
    let state = common::create_test_app_state();
    let app = create_pjs_router().with_state(state);

    let request = Request::builder()
        .uri(format!(
            "/pjs/sessions/{}/streams/{}/frames",
            SessionId::new(),
            StreamId::new()
        ))
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
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

// ===== GET .../frames/stream Endpoint Tests (#511) =====

mod stream_frames_stream {
    use super::*;
    use futures::StreamExt;

    /// Builds a router with a session/stream that already has `frame_count` frames
    /// generated via `POST .../generate-frames`.
    ///
    /// Each frame is produced from its own top-level object key (`extract_patches`
    /// emits one patch per leaf-level value, and `chunk_patches_for_commit` chunks
    /// 1:1 into frames when `patches.len() == max_frames`), so `frame_count` keys
    /// generated at `max_frames = frame_count` yields exactly `frame_count` frames.
    async fn seed_streamed_frames(frame_count: usize) -> (axum::Router, SessionId, StreamId) {
        let mut session = common::SessionBuilder::new().build();
        let session_id = session.id();
        let data: serde_json::Map<String, JsonValue> = (0..frame_count)
            .map(|i| (format!("k{i}"), JsonValue::from(i)))
            .collect();
        let stream_id = session
            .create_stream(JsonValue::Object(data).into())
            .unwrap();
        session.start_stream(stream_id).unwrap();

        let repository = Arc::new(common::MockRepository::with_session(session));
        let event_publisher = Arc::new(common::MockEventPublisher::new());
        let stream_store = Arc::new(common::MockStreamStore::new());

        use pjson_rs::infrastructure::http::axum_adapter::PjsAppState;
        let state = PjsAppState::new(repository, event_publisher, stream_store);
        let app = create_pjs_router().with_state(state);

        let generate = Request::builder()
            .uri(format!(
                "/pjs/sessions/{session_id}/streams/{stream_id}/generate-frames"
            ))
            .method("POST")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(format!(
                r#"{{"max_frames":{}}}"#,
                frame_count.max(1)
            )))
            .unwrap();
        let response = app.clone().oneshot(generate).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let generated: JsonValue = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            generated["frame_count"].as_u64().unwrap(),
            frame_count as u64,
            "seed setup must generate exactly the requested frame count: {generated:?}",
        );

        (app, session_id, stream_id)
    }

    fn stream_request(
        session_id: SessionId,
        stream_id: StreamId,
        accept: Option<&str>,
    ) -> Request<Body> {
        let mut builder = Request::builder()
            .uri(format!(
                "/pjs/sessions/{session_id}/streams/{stream_id}/frames/stream"
            ))
            .method("GET");
        if let Some(accept) = accept {
            builder = builder.header(header::ACCEPT, accept);
        }
        builder.body(Body::empty()).unwrap()
    }

    #[tokio::test]
    async fn test_stream_frames_default_accept_returns_ndjson() {
        let (app, session_id, stream_id) = seed_streamed_frames(4).await;

        let response = app
            .oneshot(stream_request(session_id, stream_id, None))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap()
            .to_string();
        assert_eq!(content_type, "application/x-ndjson");

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = std::str::from_utf8(&body).unwrap();
        let mut line_count = 0;
        for line in text.lines() {
            if line.is_empty() {
                continue;
            }
            let v: JsonValue = serde_json::from_str(line).expect("each line must be a JSON object");
            assert!(v.is_object());
            line_count += 1;
        }
        assert_eq!(line_count, 4);
    }

    /// Regression test for #516: the streaming route's per-frame JSON shape must
    /// match the buffered `/frames` route's shape exactly — same field names
    /// (`frame_type`, not the old hand-rolled `type`), `stream_id` present, and
    /// `frame_type` using the same serde representation (not `Debug`-formatted).
    #[tokio::test]
    async fn test_stream_frames_shape_matches_buffered_frames_route() {
        let (app, session_id, stream_id) = seed_streamed_frames(1).await;

        let buffered_request = Request::builder()
            .uri(format!(
                "/pjs/sessions/{session_id}/streams/{stream_id}/frames"
            ))
            .method("GET")
            .body(Body::empty())
            .unwrap();
        let buffered_response = app.clone().oneshot(buffered_request).await.unwrap();
        assert_eq!(buffered_response.status(), StatusCode::OK);
        let buffered_body = axum::body::to_bytes(buffered_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let buffered_json: JsonValue = serde_json::from_slice(&buffered_body).unwrap();
        let buffered_frame = buffered_json["frames"][0].clone();

        let streamed_response = app
            .oneshot(stream_request(session_id, stream_id, None))
            .await
            .unwrap();
        assert_eq!(streamed_response.status(), StatusCode::OK);
        let streamed_body = axum::body::to_bytes(streamed_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = std::str::from_utf8(&streamed_body).unwrap();
        let streamed_frame: JsonValue = serde_json::from_str(text.lines().next().unwrap())
            .expect("streamed frame must be valid JSON");

        let mut buffered_keys: Vec<&str> = buffered_frame
            .as_object()
            .expect("buffered frame must be an object")
            .keys()
            .map(String::as_str)
            .collect();
        let mut streamed_keys: Vec<&str> = streamed_frame
            .as_object()
            .expect("streamed frame must be an object")
            .keys()
            .map(String::as_str)
            .collect();
        buffered_keys.sort_unstable();
        streamed_keys.sort_unstable();
        assert_eq!(
            streamed_keys, buffered_keys,
            "streaming route field names must match the buffered /frames route"
        );

        assert!(
            streamed_frame.get("stream_id").is_some(),
            "streamed frame must carry stream_id, like the buffered route"
        );
        assert!(
            streamed_frame.get("frame_type").is_some(),
            "field must be named frame_type, not type"
        );
        assert_eq!(
            streamed_frame["frame_type"], buffered_frame["frame_type"],
            "frame_type must use the buffered route's serde representation, not Debug format"
        );
    }

    #[tokio::test]
    async fn test_stream_frames_sse_accept() {
        let (app, session_id, stream_id) = seed_streamed_frames(3).await;

        let response = app
            .oneshot(stream_request(
                session_id,
                stream_id,
                Some("text/event-stream"),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/event-stream"
        );
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-cache"
        );
        assert!(
            response.headers().get(header::CONNECTION).is_none(),
            "the encoder, not the application, owns connection-management headers"
        );
        assert_eq!(response.headers().get("X-Accel-Buffering").unwrap(), "no");

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = std::str::from_utf8(&body).unwrap();
        let events: Vec<&str> = text.split("\n\n").filter(|s| !s.is_empty()).collect();
        assert_eq!(events.len(), 3);
        for event in events {
            assert!(event.starts_with("data: "));
            let json_part = &event["data: ".len()..];
            let v: JsonValue = serde_json::from_str(json_part).unwrap();
            assert!(v.is_object());
        }
    }

    #[tokio::test]
    async fn test_stream_frames_ndjson_accept() {
        let (app, session_id, stream_id) = seed_streamed_frames(2).await;

        let response = app
            .oneshot(stream_request(
                session_id,
                stream_id,
                Some("application/x-ndjson"),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/x-ndjson"
        );
    }

    /// S5: this route offers no real binary wire format — `application/octet-stream`
    /// must fall back to NDJSON, not a JSON array mislabelled as binary.
    #[tokio::test]
    async fn test_stream_frames_octet_stream_falls_back_to_ndjson() {
        let (app, session_id, stream_id) = seed_streamed_frames(2).await;

        let response = app
            .oneshot(stream_request(
                session_id,
                stream_id,
                Some("application/octet-stream"),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/x-ndjson"
        );
    }

    /// Incrementality proof: the body must arrive as one chunk per frame, not one
    /// contiguous buffer — without this, `to_bytes` would hide whether anything
    /// actually streams.
    #[tokio::test]
    async fn test_stream_frames_is_chunked_per_frame() {
        let (app, session_id, stream_id) = seed_streamed_frames(5).await;

        let response = app
            .oneshot(stream_request(session_id, stream_id, None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let mut data_stream = response.into_body().into_data_stream();
        let mut chunk_count = 0;
        while let Some(chunk) = data_stream.next().await {
            chunk.expect("chunk must not error");
            chunk_count += 1;
        }
        assert_eq!(
            chunk_count, 5,
            "expected one chunk per frame, proving incremental delivery"
        );
    }

    /// Guard for the manual `Transfer-Encoding` deletion: the handler must not emit
    /// it. This says nothing about hyper's own framing, which `oneshot` cannot
    /// observe.
    #[tokio::test]
    async fn test_stream_frames_handler_sets_no_transfer_encoding() {
        let (app, session_id, stream_id) = seed_streamed_frames(2).await;

        let response = app
            .oneshot(stream_request(session_id, stream_id, None))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().get("Transfer-Encoding").is_none());
    }

    /// End-to-end proof that the q-value fix is wired: a lower-q SSE preference
    /// must not beat the higher-q (default) `application/json`.
    #[tokio::test]
    async fn test_stream_frames_q_value_preference() {
        let (app, session_id, stream_id) = seed_streamed_frames(2).await;

        let response = app
            .oneshot(stream_request(
                session_id,
                stream_id,
                Some("application/json, text/event-stream;q=0.1"),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/x-ndjson"
        );
    }

    #[tokio::test]
    async fn test_stream_frames_sets_total_count_header() {
        let (app, session_id, stream_id) = seed_streamed_frames(6).await;

        // M13: seed more frames than the page limit so the header can only pass
        // by actually reporting the total match count, not the page length.
        let request = Request::builder()
            .uri(format!(
                "/pjs/sessions/{session_id}/streams/{stream_id}/frames/stream?limit=2"
            ))
            .method("GET")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("X-Total-Count").unwrap(), "6");

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = std::str::from_utf8(&body).unwrap();
        let line_count = text.lines().filter(|l| !l.is_empty()).count();
        assert_eq!(
            line_count, 2,
            "the page itself must still honor ?limit=2 even though X-Total-Count reports 6"
        );
    }

    #[tokio::test]
    async fn test_stream_frames_not_found() {
        let state = common::create_test_app_state();
        let app = create_pjs_router().with_state(state);

        let response = app
            .oneshot(stream_request(SessionId::new(), StreamId::new(), None))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: JsonValue = serde_json::from_slice(&body).unwrap();
        assert!(json.get("error").is_some());
    }

    #[tokio::test]
    async fn test_stream_frames_invalid_priority() {
        let (app, session_id, stream_id) = seed_streamed_frames(1).await;

        let request = Request::builder()
            .uri(format!(
                "/pjs/sessions/{session_id}/streams/{stream_id}/frames/stream?priority=0"
            ))
            .method("GET")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_stream_frames_empty_page() {
        let (app, session_id, stream_id) = seed_streamed_frames(0).await;

        let response = app
            .oneshot(stream_request(session_id, stream_id, None))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(body.is_empty());
    }
}
