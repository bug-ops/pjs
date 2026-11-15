//! Acceptance test for #604 criterion 6: `GET /pjs/sessions?limit=0` is a client error (#621).
//!
//! The real `GatInMemoryStreamRepository` rejects it with `DomainError::InvalidInput`, which the
//! HTTP adapter maps to 400. The mock repository does not validate, so it is not pinned.

#![feature(impl_trait_in_assoc_type)]
#![cfg(feature = "http-server")]

mod common;

use std::sync::Arc;

use axum::{body::Body, http::Request};
use pjson_rs::infrastructure::adapters::gat_memory_repository::GatInMemoryStreamRepository;
use pjson_rs::infrastructure::http::axum_adapter::{PjsAppState, create_pjs_router};
use tower::ServiceExt;

fn list_request(query: &str) -> Request<Body> {
    Request::builder()
        .uri(format!("/pjs/sessions?{query}"))
        .method("GET")
        .body(Body::empty())
        .expect("request builds")
}

#[tokio::test]
async fn test_list_sessions_limit_zero_with_real_repository_returns_bad_request() {
    let state = PjsAppState::new(
        Arc::new(GatInMemoryStreamRepository::new()),
        Arc::new(common::MockEventPublisher::new()),
        Arc::new(common::MockStreamStore::new()),
    );
    let response = create_pjs_router()
        .with_state(state)
        .oneshot(list_request("limit=0"))
        .await
        .expect("responds");
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
}
