// Integration tests for HTTP rate limiting middleware
//
// This test file covers P2-SEC-001: Rate limiting middleware integration
// - RateLimitMiddleware with token bucket implementation
// - 429 Too Many Requests response when limit exceeded
// - X-RateLimit-* headers per RFC 6585
// - Per-IP rate limiting
// - Concurrent request handling
//
// Coverage target: 100% for rate limiting integration

#![cfg(feature = "http-server")]

use axum::{
    Router,
    body::Body,
    extract::Request,
    http::{StatusCode, header},
    response::IntoResponse,
    routing::get,
};
use pjson_rs::infrastructure::http::{RateLimitConfig, RateLimitMiddleware};
use std::time::Duration;
use tower::ServiceExt;

// ============================================================================
// RateLimitConfig Tests
// ============================================================================

#[test]
fn test_rate_limit_config_default() {
    let config = RateLimitConfig::default();

    assert_eq!(config.max_requests_per_window, 100);
    assert_eq!(config.window_duration, Duration::from_secs(60));
}

#[test]
fn test_rate_limit_config_new() {
    let config = RateLimitConfig::new(50);

    assert_eq!(config.max_requests_per_window, 50);
    assert_eq!(config.window_duration, Duration::from_secs(60));
}

#[test]
fn test_rate_limit_config_with_window() {
    let config = RateLimitConfig::new(100).with_window(Duration::from_secs(30));

    assert_eq!(config.max_requests_per_window, 100);
    assert_eq!(config.window_duration, Duration::from_secs(30));
}

#[test]
fn test_rate_limit_config_builder_pattern() {
    let config = RateLimitConfig::new(200).with_window(Duration::from_secs(120));

    assert_eq!(config.max_requests_per_window, 200);
    assert_eq!(config.window_duration, Duration::from_secs(120));
}

// ============================================================================
// RateLimitMiddleware Integration Tests
// ============================================================================

async fn test_handler() -> impl IntoResponse {
    "OK"
}

fn create_test_router(config: RateLimitConfig) -> Router {
    let middleware = RateLimitMiddleware::new(config);

    Router::new()
        .route("/test", get(test_handler))
        .layer(middleware)
}

#[tokio::test]
async fn test_rate_limit_middleware_allows_requests_under_limit() {
    let config = RateLimitConfig::new(5).with_window(Duration::from_secs(10));
    let app = create_test_router(config);

    // First request should succeed
    let request = Request::builder().uri("/test").body(Body::empty()).unwrap();

    let response = app.clone().oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    // Verify rate limit headers are present
    let headers = response.headers();
    assert!(headers.contains_key("X-RateLimit-Limit"));
    assert!(headers.contains_key("X-RateLimit-Remaining"));
    assert!(headers.contains_key("X-RateLimit-Reset"));
}

#[tokio::test]
async fn test_rate_limit_middleware_blocks_requests_over_limit() {
    // Very strict limit for testing
    let config = RateLimitConfig::new(2).with_window(Duration::from_millis(100));
    let app = create_test_router(config);

    // First two requests should succeed
    for _ in 0..2 {
        let request = Request::builder().uri("/test").body(Body::empty()).unwrap();

        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    // Third request should be rate limited
    let request = Request::builder().uri("/test").body(Body::empty()).unwrap();

    let response = app.clone().oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);

    // Verify Retry-After header
    assert!(response.headers().contains_key("Retry-After"));

    // Verify rate limit headers are present
    let headers = response.headers();
    assert!(headers.contains_key("X-RateLimit-Limit"));
    assert!(headers.contains_key("X-RateLimit-Remaining"));
    assert!(headers.contains_key("X-RateLimit-Reset"));
}

#[tokio::test]
async fn test_rate_limit_headers_format() {
    let config = RateLimitConfig::new(10).with_window(Duration::from_secs(60));
    let app = create_test_router(config);

    let request = Request::builder().uri("/test").body(Body::empty()).unwrap();

    let response = app.clone().oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let headers = response.headers();

    // X-RateLimit-Limit should be present and parseable
    let limit = headers
        .get("X-RateLimit-Limit")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u32>().ok());
    assert!(limit.is_some());

    // X-RateLimit-Remaining should be present and parseable
    let remaining = headers
        .get("X-RateLimit-Remaining")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u32>().ok());
    assert!(remaining.is_some());

    // X-RateLimit-Reset should be present and be a valid Unix timestamp
    let reset = headers
        .get("X-RateLimit-Reset")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok());
    assert!(reset.is_some());

    // Reset time should be in the future
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    assert!(reset.unwrap() > now);
}

#[tokio::test]
async fn test_rate_limit_extracts_ip_from_x_forwarded_for() {
    let config = RateLimitConfig::new(2).with_window(Duration::from_millis(100));
    let app = create_test_router(config);

    // Requests with same X-Forwarded-For should share rate limit
    for _ in 0..2 {
        let request = Request::builder()
            .uri("/test")
            .header("X-Forwarded-For", "192.168.1.100")
            .body(Body::empty())
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    // Third request from same IP should be rate limited
    let request = Request::builder()
        .uri("/test")
        .header("X-Forwarded-For", "192.168.1.100")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn test_rate_limit_extracts_ip_from_x_real_ip() {
    let config = RateLimitConfig::new(2).with_window(Duration::from_millis(100));
    let app = create_test_router(config);

    // Requests with same X-Real-IP should share rate limit
    for _ in 0..2 {
        let request = Request::builder()
            .uri("/test")
            .header("X-Real-IP", "10.0.0.50")
            .body(Body::empty())
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    // Third request from same IP should be rate limited
    let request = Request::builder()
        .uri("/test")
        .header("X-Real-IP", "10.0.0.50")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn test_rate_limit_different_ips_isolated() {
    let config = RateLimitConfig::new(1).with_window(Duration::from_millis(100));
    let app = create_test_router(config);

    // First IP uses its limit
    let request1 = Request::builder()
        .uri("/test")
        .header("X-Forwarded-For", "192.168.1.1")
        .body(Body::empty())
        .unwrap();

    let response1 = app.clone().oneshot(request1).await.unwrap();
    assert_eq!(response1.status(), StatusCode::OK);

    // Second request from first IP should be rate limited
    let request2 = Request::builder()
        .uri("/test")
        .header("X-Forwarded-For", "192.168.1.1")
        .body(Body::empty())
        .unwrap();

    let response2 = app.clone().oneshot(request2).await.unwrap();
    assert_eq!(response2.status(), StatusCode::TOO_MANY_REQUESTS);

    // Different IP should still work
    let request3 = Request::builder()
        .uri("/test")
        .header("X-Forwarded-For", "192.168.1.2")
        .body(Body::empty())
        .unwrap();

    let response3 = app.clone().oneshot(request3).await.unwrap();
    assert_eq!(response3.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_rate_limit_window_reset() {
    let config = RateLimitConfig::new(1).with_window(Duration::from_millis(50));
    let app = create_test_router(config);

    // First request succeeds
    let request1 = Request::builder().uri("/test").body(Body::empty()).unwrap();

    let response1 = app.clone().oneshot(request1).await.unwrap();
    assert_eq!(response1.status(), StatusCode::OK);

    // Second request immediately fails
    let request2 = Request::builder().uri("/test").body(Body::empty()).unwrap();

    let response2 = app.clone().oneshot(request2).await.unwrap();
    assert_eq!(response2.status(), StatusCode::TOO_MANY_REQUESTS);

    // Wait for window to reset
    tokio::time::sleep(Duration::from_millis(60)).await;

    // Third request should succeed after reset
    let request3 = Request::builder().uri("/test").body(Body::empty()).unwrap();

    let response3 = app.clone().oneshot(request3).await.unwrap();
    assert_eq!(response3.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_rate_limit_429_response_body() {
    let config = RateLimitConfig::new(1).with_window(Duration::from_millis(100));
    let app = create_test_router(config);

    // Exhaust rate limit
    let request1 = Request::builder().uri("/test").body(Body::empty()).unwrap();
    let _ = app.clone().oneshot(request1).await.unwrap();

    // Get 429 response
    let request2 = Request::builder().uri("/test").body(Body::empty()).unwrap();

    let response = app.clone().oneshot(request2).await.unwrap();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);

    // Verify Content-Type is JSON
    let content_type = response.headers().get(header::CONTENT_TYPE);
    assert!(content_type.is_some());
    assert_eq!(content_type.unwrap(), "application/json");

    // Verify response body contains error details
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();

    assert!(body_str.contains("Too Many Requests"));
    assert!(body_str.contains("retry_after"));
}

#[tokio::test]
async fn test_rate_limit_x_forwarded_for_multiple_ips() {
    let config = RateLimitConfig::new(2).with_window(Duration::from_millis(100));
    let app = create_test_router(config);

    // X-Forwarded-For with multiple IPs (proxy chain)
    // Should use the first IP in the chain
    for _ in 0..2 {
        let request = Request::builder()
            .uri("/test")
            .header("X-Forwarded-For", "203.0.113.1, 198.51.100.1, 192.0.2.1")
            .body(Body::empty())
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    // Third request should be rate limited (same first IP)
    let request = Request::builder()
        .uri("/test")
        .header("X-Forwarded-For", "203.0.113.1, 198.51.100.99")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn test_rate_limit_fallback_to_localhost() {
    let config = RateLimitConfig::new(2).with_window(Duration::from_millis(100));
    let app = create_test_router(config);

    // Requests without IP headers should use localhost
    for _ in 0..2 {
        let request = Request::builder().uri("/test").body(Body::empty()).unwrap();

        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    // Third request should be rate limited
    let request = Request::builder().uri("/test").body(Body::empty()).unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
}

// ============================================================================
// Concurrent Request Tests
// ============================================================================

#[tokio::test]
async fn test_rate_limit_concurrent_requests_same_ip() {
    let config = RateLimitConfig::new(5).with_window(Duration::from_millis(200));
    let app = create_test_router(config);

    let mut handles = vec![];

    // Send 10 concurrent requests from same IP
    for _ in 0..10 {
        let app_clone = app.clone();
        let handle = tokio::spawn(async move {
            let request = Request::builder()
                .uri("/test")
                .header("X-Forwarded-For", "192.168.1.100")
                .body(Body::empty())
                .unwrap();

            app_clone.oneshot(request).await.unwrap().status()
        });
        handles.push(handle);
    }

    // Collect results
    let mut success_count = 0;
    let mut rate_limited_count = 0;

    for handle in handles {
        let status = handle.await.unwrap();
        match status {
            StatusCode::OK => success_count += 1,
            StatusCode::TOO_MANY_REQUESTS => rate_limited_count += 1,
            _ => panic!("Unexpected status code: {}", status),
        }
    }

    // Should have exactly 5 successes (the limit) and 5 rate limited
    assert_eq!(success_count, 5);
    assert_eq!(rate_limited_count, 5);
}

#[tokio::test]
async fn test_rate_limit_concurrent_requests_different_ips() {
    let config = RateLimitConfig::new(2).with_window(Duration::from_millis(200));
    let app = create_test_router(config);

    let mut handles = vec![];

    // Send requests from 3 different IPs, 3 requests each
    for ip_suffix in 1..=3 {
        for _ in 0..3 {
            let app_clone = app.clone();
            let ip = format!("192.168.1.{}", ip_suffix);

            let handle = tokio::spawn(async move {
                let request = Request::builder()
                    .uri("/test")
                    .header("X-Forwarded-For", ip)
                    .body(Body::empty())
                    .unwrap();

                (
                    ip_suffix,
                    app_clone.oneshot(request).await.unwrap().status(),
                )
            });
            handles.push(handle);
        }
    }

    // Each IP should have 2 successes and 1 rate limited
    let mut results_by_ip: std::collections::HashMap<u8, Vec<StatusCode>> =
        std::collections::HashMap::new();

    for handle in handles {
        let (ip_suffix, status) = handle.await.unwrap();
        results_by_ip.entry(ip_suffix).or_default().push(status);
    }

    // Verify each IP was rate limited independently
    for (_, statuses) in results_by_ip {
        let success_count = statuses.iter().filter(|&&s| s == StatusCode::OK).count();
        let rate_limited_count = statuses
            .iter()
            .filter(|&&s| s == StatusCode::TOO_MANY_REQUESTS)
            .count();

        assert_eq!(success_count, 2);
        assert_eq!(rate_limited_count, 1);
    }
}

// ============================================================================
// Performance Tests
// ============================================================================

#[tokio::test]
async fn test_rate_limit_overhead_minimal() {
    let config = RateLimitConfig::new(1000).with_window(Duration::from_secs(60));
    let app = create_test_router(config);

    let start = std::time::Instant::now();

    // Send 100 requests
    for _ in 0..100 {
        let request = Request::builder().uri("/test").body(Body::empty()).unwrap();

        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    let duration = start.elapsed();

    // Rate limiting overhead should be minimal (< 1ms per request average)
    let avg_per_request = duration.as_micros() / 100;
    assert!(
        avg_per_request < 1000,
        "Rate limiting overhead too high: {} µs per request",
        avg_per_request
    );
}
