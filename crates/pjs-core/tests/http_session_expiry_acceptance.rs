//! Acceptance test for #596: `POST /pjs/sessions` returns the aggregate's stored `expires_at`.

#![cfg_attr(feature = "http-server", feature(impl_trait_in_assoc_type))]
#![cfg(feature = "http-server")]

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use chrono::{DateTime, Utc};
use pjson_rs::infrastructure::http::axum_adapter::create_pjs_router;
use serde_json::Value;
use tower::ServiceExt;

async fn body_json(response: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body reads");
    serde_json::from_slice(&bytes).expect("body is json")
}

#[tokio::test]
async fn test_create_session_expires_at_equals_stored_expires_at() {
    let state = common::create_test_app_state();

    let create = Request::builder()
        .uri("/pjs/sessions")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"timeout_seconds":1800}"#))
        .expect("request builds");
    let response = create_pjs_router()
        .with_state(state.clone())
        .oneshot(create)
        .await
        .expect("create responds");
    assert_eq!(response.status(), StatusCode::OK);
    let created = body_json(response).await;
    let session_id = created["session_id"]
        .as_str()
        .expect("session_id")
        .to_owned();
    let created_expiry: DateTime<Utc> = created["expires_at"]
        .as_str()
        .expect("expires_at")
        .parse()
        .expect("rfc3339");

    let get = Request::builder()
        .uri(format!("/pjs/sessions/{session_id}"))
        .method("GET")
        .body(Body::empty())
        .expect("request builds");
    let response = create_pjs_router()
        .with_state(state)
        .oneshot(get)
        .await
        .expect("get responds");
    assert_eq!(response.status(), StatusCode::OK);
    let stored = body_json(response).await;
    let stored_expiry: DateTime<Utc> = stored["session"]["expires_at"]
        .as_str()
        .expect("stored expires_at")
        .parse()
        .expect("rfc3339");

    assert_eq!(created_expiry, stored_expiry);
}

#[tokio::test]
async fn test_create_session_timeout_over_limit_rejected_with_400() {
    let state = common::create_test_app_state();
    for body in [
        r#"{"timeout_seconds":0}"#,
        r#"{"timeout_seconds":604801}"#,
        r#"{"timeout_seconds":18446744073709551615}"#,
    ] {
        let request = Request::builder()
            .uri("/pjs/sessions")
            .method("POST")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .expect("request builds");
        let response = create_pjs_router()
            .with_state(state.clone())
            .oneshot(request)
            .await
            .expect("responds");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{body}");
    }
}
