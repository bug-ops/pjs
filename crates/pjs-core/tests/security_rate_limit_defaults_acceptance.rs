//! Validation tests for #704: the HTTP rate limiter takes `security::RateLimitConfig`
//! only, whose defaults (100 requests per 60 s window) are the single source, and
//! it honors the window.
#![cfg(feature = "http-server")]

use std::time::Duration;

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    routing::get,
};
use pjson_rs::infrastructure::http::RateLimitMiddleware;
use pjson_rs::security::RateLimitConfig;
use tower::ServiceExt;

#[test]
fn rate_limit_config_default_is_100_requests_per_minute() {
    let config = RateLimitConfig::default();

    assert_eq!(config.max_requests_per_window, 100);
    assert_eq!(config.window_duration, Duration::from_secs(60));
}

#[tokio::test]
async fn http_rate_limit_allows_requests_again_after_the_window() {
    let config = RateLimitConfig {
        max_requests_per_window: 1,
        window_duration: Duration::from_millis(300),
        ..RateLimitConfig::default()
    };
    let app = Router::new()
        .route("/t", get(|| async { "OK" }))
        .layer(RateLimitMiddleware::new(config));
    let call = |app: Router| async move {
        app.oneshot(Request::builder().uri("/t").body(Body::empty()).unwrap())
            .await
            .unwrap()
            .status()
    };

    assert_eq!(call(app.clone()).await, StatusCode::OK);
    assert_eq!(call(app.clone()).await, StatusCode::TOO_MANY_REQUESTS);
    tokio::time::sleep(Duration::from_millis(450)).await;
    assert_eq!(call(app).await, StatusCode::OK);
}
