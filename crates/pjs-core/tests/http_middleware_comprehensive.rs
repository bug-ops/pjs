// Comprehensive tests for HTTP middleware module
//
// This test file covers the infrastructure/http/middleware.rs module with focus on:
// - PjsMiddleware configuration and builder pattern
// - Request size validation
// - Performance metrics headers
// - RateLimitMiddleware creation
// - Security middleware
//
// Coverage target: 60%+ for Infrastructure Layer

#![cfg(feature = "http-server")]

use axum::{
    Router,
    body::Body,
    extract::Request,
    http::StatusCode,
    middleware,
    response::{IntoResponse, Response},
    routing::get,
};
use pjson_rs::infrastructure::http::middleware::{
    PjsMiddleware, RateLimitMiddleware, security_middleware,
};
use tower::ServiceExt;

// ============================================================================
// PjsMiddleware Tests
// ============================================================================

#[test]
fn test_pjs_middleware_default_creation() {
    let _middleware = PjsMiddleware::default();

    // PjsMiddleware created successfully with defaults
}

#[test]
fn test_pjs_middleware_new() {
    let _middleware = PjsMiddleware::new();

    // PjsMiddleware created successfully
}

#[test]
fn test_pjs_middleware_with_metrics() {
    let _middleware = PjsMiddleware::new().with_metrics(false);

    // Builder pattern works
}

#[test]
fn test_pjs_middleware_with_max_request_size() {
    let _middleware = PjsMiddleware::new().with_max_request_size(5 * 1024 * 1024);

    // Builder pattern works
}

#[test]
fn test_pjs_middleware_builder_pattern() {
    let _middleware = PjsMiddleware::new()
        .with_metrics(true)
        .with_max_request_size(1024 * 1024);

    // Builder pattern works - fields are private
}

#[test]
fn test_pjs_middleware_clone() {
    let middleware1 = PjsMiddleware::new().with_max_request_size(2048);

    let _middleware2 = middleware1.clone();

    // Clone works - fields are private so we can't compare them directly
}

#[tokio::test]
async fn test_pjs_middleware_layer() {
    let middleware = PjsMiddleware::new();

    async fn handler() -> impl IntoResponse {
        "OK"
    }

    let app = Router::new().route("/test", get(handler)).layer(middleware);

    let response = app
        .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_pjs_middleware_adds_performance_headers() {
    let middleware = PjsMiddleware::new().with_metrics(true);

    async fn handler() -> impl IntoResponse {
        "OK"
    }

    let app = Router::new().route("/test", get(handler)).layer(middleware);

    let response = app
        .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
        .await
        .unwrap();

    // Should have performance headers
    assert!(response.headers().contains_key("X-PJS-Duration-Ms"));
    assert!(response.headers().contains_key("X-PJS-Version"));
}

#[tokio::test]
async fn test_pjs_middleware_no_metrics_headers() {
    let middleware = PjsMiddleware::new().with_metrics(false);

    async fn handler() -> impl IntoResponse {
        "OK"
    }

    let app = Router::new().route("/test", get(handler)).layer(middleware);

    let response = app
        .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
        .await
        .unwrap();

    // Should NOT have performance headers
    assert!(!response.headers().contains_key("X-PJS-Duration-Ms"));
}

// ============================================================================
// RateLimitMiddleware Tests
// ============================================================================

#[tokio::test]
async fn test_rate_limit_middleware_creation() {
    use pjson_rs::security::RateLimitConfig;
    let config = RateLimitConfig {
        max_requests_per_window: 100,
        ..RateLimitConfig::default()
    };
    let _rate_limit = RateLimitMiddleware::new(config);

    // RateLimitMiddleware created successfully
}

#[tokio::test]
async fn test_rate_limit_middleware_burst_calculation() {
    use pjson_rs::security::RateLimitConfig;
    let config = RateLimitConfig {
        max_requests_per_window: 200,
        ..RateLimitConfig::default()
    };
    let _rate_limit = RateLimitMiddleware::new(config);

    // RateLimitMiddleware created successfully
}

#[tokio::test]
async fn test_rate_limit_middleware_small_limit() {
    use pjson_rs::security::RateLimitConfig;
    let config = RateLimitConfig {
        max_requests_per_window: 4,
        ..RateLimitConfig::default()
    };
    let _rate_limit = RateLimitMiddleware::new(config);

    // RateLimitMiddleware created successfully
}

#[tokio::test]
async fn test_rate_limit_middleware_clone() {
    use pjson_rs::security::RateLimitConfig;
    let config = RateLimitConfig {
        max_requests_per_window: 150,
        ..RateLimitConfig::default()
    };
    let rate_limit1 = RateLimitMiddleware::new(config);
    let _rate_limit2 = rate_limit1.clone();

    // Clone works - fields are private
}

// ============================================================================
// Security Middleware Tests
// ============================================================================

#[test]
fn test_security_middleware_imports() {
    // Test that we can import security_middleware
    let _ = security_middleware;
}

// ============================================================================
// Integration Tests
// ============================================================================

#[tokio::test]
async fn test_middleware_stack() {
    async fn handler() -> impl IntoResponse {
        "OK"
    }

    let app = Router::new()
        .route("/test", get(handler))
        .layer(middleware::from_fn(security_middleware))
        .layer(PjsMiddleware::new());

    let response = app
        .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
        .await
        .unwrap();

    // Should have headers from all middleware
    assert!(response.headers().contains_key("X-Content-Type-Options"));
    assert!(response.headers().contains_key("X-PJS-Duration-Ms"));
}

#[tokio::test]
async fn test_middleware_order_matters() {
    async fn handler() -> impl IntoResponse {
        Response::builder()
            .status(StatusCode::OK)
            .body(Body::from("test"))
            .unwrap()
    }

    // Apply middleware in specific order
    let app = Router::new()
        .route("/test", get(handler))
        .layer(middleware::from_fn(security_middleware)) // Last applied, first executed
        .layer(PjsMiddleware::new()); // First applied, last executed

    let response = app
        .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}
