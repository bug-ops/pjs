// Integration tests for HTTP rate limiting middleware
//
// This test file covers P2-SEC-001: Rate limiting middleware integration
// - RateLimitMiddleware with token bucket implementation
// - 429 Too Many Requests response when limit exceeded
// - X-RateLimit-* headers per RFC 6585
// - Per-IP rate limiting keyed on the real TCP peer address (ConnectInfo)
// - X-Forwarded-For/X-Real-IP are untrusted by default (#336); only consulted
//   when TrustedProxyConfig explicitly allowlists the peer
// - Concurrent request handling
//
// Coverage target: 100% for rate limiting integration

#![cfg(feature = "http-server")]

use axum::{
    Router,
    body::Body,
    extract::{ConnectInfo, Request},
    http::{StatusCode, header},
    response::IntoResponse,
    routing::get,
};
use pjson_rs::infrastructure::http::{RateLimitConfig, RateLimitMiddleware, TrustedProxyConfig};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;
use tower::ServiceExt;

fn peer(addr: [u8; 4], port: u16) -> ConnectInfo<SocketAddr> {
    ConnectInfo(SocketAddr::from((IpAddr::V4(Ipv4Addr::from(addr)), port)))
}

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

/// Without a `ConnectInfo<SocketAddr>` extension (i.e. the router not served
/// via `into_make_service_with_connect_info`), the real peer is unknown and
/// every request falls back to the same key. `X-Forwarded-For` is never
/// trusted by default (#336), so a *different* header value must not grant a
/// fresh bucket.
#[tokio::test]
async fn test_rate_limit_ignores_x_forwarded_for_by_default() {
    let config = RateLimitConfig::new(2).with_window(Duration::from_millis(100));
    let app = create_test_router(config);

    for _ in 0..2 {
        let request = Request::builder()
            .uri("/test")
            .header("X-Forwarded-For", "192.168.1.100")
            .body(Body::empty())
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    // Different X-Forwarded-For value must NOT grant a fresh bucket.
    let request = Request::builder()
        .uri("/test")
        .header("X-Forwarded-For", "10.0.0.1")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
}

/// Same as above for `X-Real-IP` — never trusted by default.
#[tokio::test]
async fn test_rate_limit_ignores_x_real_ip_by_default() {
    let config = RateLimitConfig::new(2).with_window(Duration::from_millis(100));
    let app = create_test_router(config);

    for _ in 0..2 {
        let request = Request::builder()
            .uri("/test")
            .header("X-Real-IP", "10.0.0.50")
            .body(Body::empty())
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    // Different X-Real-IP value must NOT grant a fresh bucket.
    let request = Request::builder()
        .uri("/test")
        .header("X-Real-IP", "10.0.0.99")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
}

/// Isolation must be keyed on the real TCP peer (`ConnectInfo`), not on any
/// client-supplied header.
#[tokio::test]
async fn test_rate_limit_different_ips_isolated() {
    let config = RateLimitConfig::new(1).with_window(Duration::from_millis(100));
    let app = create_test_router(config);

    let mut request1 = Request::builder().uri("/test").body(Body::empty()).unwrap();
    request1.extensions_mut().insert(peer([192, 168, 1, 1], 1));

    let response1 = app.clone().oneshot(request1).await.unwrap();
    assert_eq!(response1.status(), StatusCode::OK);

    // Second request from the same peer should be rate limited
    let mut request2 = Request::builder().uri("/test").body(Body::empty()).unwrap();
    request2.extensions_mut().insert(peer([192, 168, 1, 1], 2));

    let response2 = app.clone().oneshot(request2).await.unwrap();
    assert_eq!(response2.status(), StatusCode::TOO_MANY_REQUESTS);

    // A different real peer should still work
    let mut request3 = Request::builder().uri("/test").body(Body::empty()).unwrap();
    request3.extensions_mut().insert(peer([192, 168, 1, 2], 1));

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

/// `X-Forwarded-For` proxy-chain parsing walks right-to-left and keys on the
/// rightmost entry that is not itself a trusted proxy — the entry appended
/// by the closest trusted hop, which the client cannot forge. A client behind
/// the trusted proxy varying the leftmost (client-supplied) entry on every
/// request must NOT get a fresh bucket; that would be the exact #336 bypass
/// reopened inside the opt-in trusted-proxy path.
#[tokio::test]
async fn test_rate_limit_x_forwarded_for_uses_rightmost_untrusted_entry() {
    let trusted_proxy = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 200));
    let config = RateLimitConfig::new(2)
        .with_window(Duration::from_millis(100))
        .with_trusted_proxies(TrustedProxyConfig::new(vec![trusted_proxy]));
    let app = create_test_router(config);

    // Same real client (rightmost entry, appended by the trusted proxy) but a
    // different attacker-controlled leftmost entry each time — must still
    // share the same bucket.
    for spoofed in ["203.0.113.1", "10.10.10.10"] {
        let mut request = Request::builder().uri("/test").body(Body::empty()).unwrap();
        request
            .extensions_mut()
            .insert(peer([198, 51, 100, 200], 443));
        request.headers_mut().insert(
            "X-Forwarded-For",
            format!("{spoofed}, 192.0.2.1").parse().unwrap(),
        );

        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "requests sharing the real (rightmost) client IP must share budget"
        );
    }

    // Budget of 2 is now exhausted by the real client 192.0.2.1. A third
    // request with yet another spoofed leftmost entry but the same rightmost
    // (real) client must still be rejected.
    let mut request = Request::builder().uri("/test").body(Body::empty()).unwrap();
    request
        .extensions_mut()
        .insert(peer([198, 51, 100, 200], 443));
    request
        .headers_mut()
        .insert("X-Forwarded-For", "1.2.3.4, 192.0.2.1".parse().unwrap());

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "leftmost spoofed entry must not grant a fresh rate-limit bucket"
    );

    // A genuinely different real client (different rightmost entry) must get
    // an independent bucket.
    let mut request2 = Request::builder().uri("/test").body(Body::empty()).unwrap();
    request2
        .extensions_mut()
        .insert(peer([198, 51, 100, 200], 443));
    request2.headers_mut().insert(
        "X-Forwarded-For",
        "203.0.113.1, 192.0.2.99".parse().unwrap(),
    );

    let response2 = app.clone().oneshot(request2).await.unwrap();
    assert_eq!(
        response2.status(),
        StatusCode::OK,
        "a different real (rightmost) client IP must not be throttled by another client's budget"
    );
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

    // Send 10 concurrent requests from same real peer
    for i in 0..10 {
        let app_clone = app.clone();
        let handle = tokio::spawn(async move {
            let mut request = Request::builder().uri("/test").body(Body::empty()).unwrap();
            request
                .extensions_mut()
                .insert(peer([192, 168, 1, 100], 1000 + i as u16));

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

    // Send requests from 3 different real peers, 3 requests each
    for ip_suffix in 1..=3u8 {
        for req_num in 0..3u16 {
            let app_clone = app.clone();

            let handle = tokio::spawn(async move {
                let mut request = Request::builder().uri("/test").body(Body::empty()).unwrap();
                request
                    .extensions_mut()
                    .insert(peer([192, 168, 1, ip_suffix], 2000 + req_num));

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
