//! Acceptance tests for #769 (track E): `POST /pjs/sessions` accepts
//! `max_concurrent_streams` in `1..=1000` only; anything else is answered
//! `400 Bad Request` and no session is created.

#![cfg_attr(feature = "http-server", feature(impl_trait_in_assoc_type))]
#![cfg(feature = "http-server")]

mod common;

use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use pjson_rs::infrastructure::http::axum_adapter::{PjsAppState, create_pjs_router};
use tower::ServiceExt;

async fn create_session(max_concurrent_streams: &str) -> (StatusCode, usize) {
    let repository = Arc::new(common::MockRepository::new());
    let state = PjsAppState::new(
        repository.clone(),
        Arc::new(common::MockEventPublisher::new()),
        Arc::new(common::MockStreamStore::new()),
    );
    let request = Request::builder()
        .uri("/pjs/sessions")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(format!(
            r#"{{"max_concurrent_streams":{max_concurrent_streams}}}"#
        )))
        .expect("request builds");

    let response = create_pjs_router()
        .with_state(state)
        .oneshot(request)
        .await
        .expect("responds");

    (response.status(), repository.session_count())
}

#[tokio::test]
async fn create_session_accepts_the_bounds_of_the_stream_limit() {
    for accepted in ["1", "1000"] {
        assert_eq!(
            create_session(accepted).await,
            (StatusCode::OK, 1),
            "max_concurrent_streams {accepted}"
        );
    }
}

#[tokio::test]
async fn create_session_rejects_a_stream_limit_outside_the_bounds_and_creates_nothing() {
    for rejected in ["0", "1001", "18446744073709551615"] {
        assert_eq!(
            create_session(rejected).await,
            (StatusCode::BAD_REQUEST, 0),
            "max_concurrent_streams {rejected}"
        );
    }
}

#[tokio::test]
async fn create_session_without_a_stream_limit_uses_the_default() {
    let repository = Arc::new(common::MockRepository::new());
    let state = PjsAppState::new(
        repository.clone(),
        Arc::new(common::MockEventPublisher::new()),
        Arc::new(common::MockStreamStore::new()),
    );
    let request = Request::builder()
        .uri("/pjs/sessions")
        .method("POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{}"))
        .expect("request builds");

    let response = create_pjs_router()
        .with_state(state)
        .oneshot(request)
        .await
        .expect("responds");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(repository.session_count(), 1);
}
