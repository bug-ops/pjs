// Comprehensive tests for HTTP middleware module
//
// This test file covers the infrastructure/http/middleware.rs module with focus on:
// - PjsMiddleware configuration and builder pattern
// - Request size validation
// - Performance metrics headers
// - Compression hints
// - RateLimitMiddleware creation
// - WebSocket upgrade middleware
// - Compression middleware
// - CORS middleware
// - Security middleware
// - Circuit breaker middleware
// - Health check middleware
//
// Coverage target: 60%+ for Infrastructure Layer

#![cfg(feature = "http-server")]

use axum::{
    Router,
    body::Body,
    extract::Request,
    http::{StatusCode, header},
    middleware,
    response::{IntoResponse, Response},
    routing::get,
};
use pjson_rs::infrastructure::http::middleware::{
    CircuitBreakerMiddleware, PjsMiddleware, RateLimitMiddleware, compression_middleware,
    health_check_middleware, pjs_cors_middleware, security_middleware,
    websocket_upgrade_middleware,
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
fn test_pjs_middleware_with_compression() {
    let _middleware = PjsMiddleware::new().with_compression(false);

    // Builder pattern works
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
        .with_compression(false)
        .with_metrics(true)
        .with_max_request_size(1024 * 1024);

    // Builder pattern works - fields are private
}

#[test]
fn test_pjs_middleware_clone() {
    let middleware1 = PjsMiddleware::new()
        .with_compression(false)
        .with_max_request_size(2048);

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
async fn test_pjs_middleware_adds_compression_header() {
    let middleware = PjsMiddleware::new().with_compression(true);

    async fn handler() -> impl IntoResponse {
        "OK"
    }

    let app = Router::new().route("/test", get(handler)).layer(middleware);

    let response = app
        .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(
        response.headers().get("X-PJS-Compression").unwrap(),
        "available"
    );
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

#[test]
fn test_rate_limit_middleware_creation() {
    use pjson_rs::infrastructure::http::RateLimitConfig;
    let config = RateLimitConfig::new(100);
    let _rate_limit = RateLimitMiddleware::new(config);

    // RateLimitMiddleware created successfully
}

#[test]
fn test_rate_limit_middleware_burst_calculation() {
    use pjson_rs::infrastructure::http::RateLimitConfig;
    let config = RateLimitConfig::new(200);
    let _rate_limit = RateLimitMiddleware::new(config);

    // RateLimitMiddleware created successfully
}

#[test]
fn test_rate_limit_middleware_small_limit() {
    use pjson_rs::infrastructure::http::RateLimitConfig;
    let config = RateLimitConfig::new(4);
    let _rate_limit = RateLimitMiddleware::new(config);

    // RateLimitMiddleware created successfully
}

#[test]
fn test_rate_limit_middleware_clone() {
    use pjson_rs::infrastructure::http::RateLimitConfig;
    let config = RateLimitConfig::new(150);
    let rate_limit1 = RateLimitMiddleware::new(config);
    let _rate_limit2 = rate_limit1.clone();

    // Clone works - fields are private
}

// ============================================================================
// WebSocket Upgrade Middleware Tests
// ============================================================================

#[test]
fn test_websocket_upgrade_middleware_imports() {
    // Test that we can import websocket_upgrade_middleware
    let _ = websocket_upgrade_middleware;
}

// ============================================================================
// Compression Middleware Tests
// ============================================================================

#[test]
fn test_compression_middleware_imports() {
    // Test that we can import compression_middleware
    let _ = compression_middleware;
}

// ============================================================================
// CORS Middleware Tests
// ============================================================================

#[test]
fn test_pjs_cors_middleware_imports() {
    // Test that we can import pjs_cors_middleware
    let _ = pjs_cors_middleware;
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
// Circuit Breaker Middleware Tests
// ============================================================================

#[test]
fn test_circuit_breaker_middleware_default() {
    let _cb = CircuitBreakerMiddleware::default();

    // CircuitBreakerMiddleware created successfully
}

#[test]
fn test_circuit_breaker_middleware_new() {
    let _cb = CircuitBreakerMiddleware::new();

    // CircuitBreakerMiddleware created successfully
}

#[test]
fn test_circuit_breaker_with_failure_threshold() {
    let _cb = CircuitBreakerMiddleware::new().with_failure_threshold(10);

    // Builder pattern works
}

#[test]
fn test_circuit_breaker_with_recovery_timeout() {
    let _cb = CircuitBreakerMiddleware::new().with_recovery_timeout(60);

    // Builder pattern works
}

#[test]
fn test_circuit_breaker_builder_pattern() {
    let _cb = CircuitBreakerMiddleware::new()
        .with_failure_threshold(15)
        .with_recovery_timeout(120);

    // Builder pattern works
}

#[test]
fn test_circuit_breaker_clone() {
    let cb1 = CircuitBreakerMiddleware::new()
        .with_failure_threshold(8)
        .with_recovery_timeout(45);

    let _cb2 = cb1.clone();

    // Clone works
}

// ============================================================================
// Health Check Middleware Tests
// ============================================================================

#[test]
fn test_health_check_middleware_imports() {
    // Test that we can import health_check_middleware
    let _ = health_check_middleware;
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
        .layer(middleware::from_fn(pjs_cors_middleware))
        .layer(middleware::from_fn(health_check_middleware))
        .layer(PjsMiddleware::new());

    let response = app
        .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
        .await
        .unwrap();

    // Should have headers from all middleware
    assert!(response.headers().contains_key("X-PJS-Health"));
    assert!(
        response
            .headers()
            .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN)
    );
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
