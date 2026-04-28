//! Integration tests for HTTP middleware and authentication layers.
//!
//! Covers:
//! - `RateLimitMiddleware` — requests within budget pass, over-limit returns 429
//! - `ApiKeyAuthLayer` — valid key (200), missing key (401), invalid key (401),
//!   OPTIONS preflight passthrough
//! - `AuthConfigError` — empty key list, whitespace key
//! - Router factory — `create_pjs_router` does not panic with a valid config
//! - `create_pjs_router_with_auth` — health is public, sessions require auth
//! - `create_pjs_router_with_rate_limit_and_auth` — rate limit wraps auth

#![feature(impl_trait_in_assoc_type)]
#![cfg(feature = "http-server")]

mod common;

use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode},
    response::IntoResponse,
    routing::get,
};
use pjson_rs::infrastructure::http::{
    HttpServerConfig, RateLimitConfig, RateLimitMiddleware,
    auth::{ApiKeyAuthLayer, ApiKeyConfig, AuthConfigError},
    axum_adapter::{PjsAppState, create_pjs_router, create_pjs_router_with_auth},
    create_pjs_router_with_rate_limit_and_auth,
};
use std::time::Duration;
use tower::ServiceExt;

// ── Shared test handler ──────────────────────────────────────────────────────

async fn ok_handler() -> impl IntoResponse {
    "OK"
}

// ============================================================================
// AuthConfigError — construction-time validation
// ============================================================================

#[test]
fn test_auth_config_error_empty_key_list() {
    let err = ApiKeyConfig::new(&[]).unwrap_err();
    assert!(
        matches!(err, AuthConfigError::EmptyKeyList),
        "expected EmptyKeyList, got: {err:?}"
    );
}

#[test]
fn test_auth_config_error_whitespace_in_key_space() {
    let err = ApiKeyConfig::new(&["bad key"]).unwrap_err();
    assert!(
        matches!(err, AuthConfigError::WhitespaceInKey),
        "expected WhitespaceInKey, got: {err:?}"
    );
}

#[test]
fn test_auth_config_error_whitespace_in_key_tab() {
    let err = ApiKeyConfig::new(&["bad\tkey"]).unwrap_err();
    assert!(
        matches!(err, AuthConfigError::WhitespaceInKey),
        "expected WhitespaceInKey, got: {err:?}"
    );
}

#[test]
fn test_auth_config_error_whitespace_leading() {
    let err = ApiKeyConfig::new(&[" leadingspace"]).unwrap_err();
    assert!(matches!(err, AuthConfigError::WhitespaceInKey));
}

#[test]
fn test_auth_config_error_whitespace_trailing() {
    let err = ApiKeyConfig::new(&["trailingspace "]).unwrap_err();
    assert!(matches!(err, AuthConfigError::WhitespaceInKey));
}

#[test]
fn test_auth_config_valid_single_key() {
    assert!(ApiKeyConfig::new(&["valid-key-1"]).is_ok());
}

#[test]
fn test_auth_config_valid_multiple_keys() {
    assert!(ApiKeyConfig::new(&["key-a", "key-b", "key-c"]).is_ok());
}

// ============================================================================
// ApiKeyAuthLayer — HTTP-level authentication behaviour
// ============================================================================

/// Build a minimal Tower service wrapped with `ApiKeyAuthLayer`.
fn make_auth_router(key: &str) -> Router {
    let config = ApiKeyConfig::new(&[key]).expect("valid key");
    let auth_layer = ApiKeyAuthLayer::new(config);
    Router::new()
        .route("/protected", get(ok_handler))
        .layer(auth_layer)
}

#[tokio::test]
async fn test_api_key_auth_valid_bearer_returns_200() {
    let app = make_auth_router("test-secret");

    let req = Request::builder()
        .method(Method::GET)
        .uri("/protected")
        .header("Authorization", "Bearer test-secret")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_api_key_auth_valid_x_pjs_api_key_returns_200() {
    let app = make_auth_router("test-secret");

    let req = Request::builder()
        .method(Method::GET)
        .uri("/protected")
        .header("X-PJS-API-Key", "test-secret")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_api_key_auth_missing_header_returns_401() {
    let app = make_auth_router("test-secret");

    let req = Request::builder()
        .method(Method::GET)
        .uri("/protected")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_api_key_auth_invalid_bearer_returns_401() {
    let app = make_auth_router("test-secret");

    let req = Request::builder()
        .method(Method::GET)
        .uri("/protected")
        .header("Authorization", "Bearer wrong-key")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_api_key_auth_invalid_x_pjs_api_key_returns_401() {
    let app = make_auth_router("test-secret");

    let req = Request::builder()
        .method(Method::GET)
        .uri("/protected")
        .header("X-PJS-API-Key", "not-the-right-key")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_api_key_auth_options_passthrough_without_credentials() {
    // OPTIONS (CORS preflight) must bypass auth even when no key is present.
    let app = make_auth_router("test-secret");

    let req = Request::builder()
        .method(Method::OPTIONS)
        .uri("/protected")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    // The inner router returns 405 for OPTIONS on a GET-only route; what matters
    // is that the auth layer itself does NOT return 401.
    assert_ne!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "OPTIONS must not be blocked by ApiKeyAuthLayer"
    );
}

#[tokio::test]
async fn test_api_key_auth_multi_key_any_valid_key_passes() {
    let config = ApiKeyConfig::new(&["key-one", "key-two"]).unwrap();
    let auth_layer = ApiKeyAuthLayer::new(config);
    let app = Router::new()
        .route("/protected", get(ok_handler))
        .layer(auth_layer);

    for key in ["key-one", "key-two"] {
        let req = Request::builder()
            .method(Method::GET)
            .uri("/protected")
            .header("X-PJS-API-Key", key)
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "key '{key}' should be accepted"
        );
    }
}

#[tokio::test]
async fn test_api_key_auth_unauthorized_response_is_json() {
    let app = make_auth_router("test-secret");

    let req = Request::builder()
        .method(Method::GET)
        .uri("/protected")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        ct.contains("application/json"),
        "401 body must be JSON, got content-type: {ct}"
    );
}

// ============================================================================
// RateLimitMiddleware — enforcement
// ============================================================================

fn make_rate_limit_router(config: RateLimitConfig) -> Router {
    Router::new()
        .route("/test", get(ok_handler))
        .layer(RateLimitMiddleware::new(config))
}

#[tokio::test]
async fn test_rate_limit_requests_within_budget_return_200() {
    let config = RateLimitConfig::new(10).with_window(Duration::from_secs(60));
    let app = make_rate_limit_router(config);

    let req = Request::builder().uri("/test").body(Body::empty()).unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_rate_limit_response_includes_headers() {
    let config = RateLimitConfig::new(10).with_window(Duration::from_secs(60));
    let app = make_rate_limit_router(config);

    let req = Request::builder().uri("/test").body(Body::empty()).unwrap();

    let resp = app.oneshot(req).await.unwrap();
    let headers = resp.headers();
    assert!(
        headers.contains_key("X-RateLimit-Limit"),
        "missing X-RateLimit-Limit header"
    );
    assert!(
        headers.contains_key("X-RateLimit-Remaining"),
        "missing X-RateLimit-Remaining header"
    );
    assert!(
        headers.contains_key("X-RateLimit-Reset"),
        "missing X-RateLimit-Reset header"
    );
}

#[tokio::test]
async fn test_rate_limit_over_limit_returns_429() {
    // Limit of 1 request — the second request must be rejected.
    let config = RateLimitConfig::new(1).with_window(Duration::from_millis(500));
    let app = make_rate_limit_router(config);

    // First request — should succeed.
    let req1 = Request::builder().uri("/test").body(Body::empty()).unwrap();
    let resp1 = app.clone().oneshot(req1).await.unwrap();
    assert_eq!(
        resp1.status(),
        StatusCode::OK,
        "first request must pass under budget"
    );

    // Second request — rate limiter should block it.
    let req2 = Request::builder().uri("/test").body(Body::empty()).unwrap();
    let resp2 = app.oneshot(req2).await.unwrap();
    assert_eq!(
        resp2.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "second request must be rejected with 429"
    );
}

#[tokio::test]
async fn test_rate_limit_429_includes_retry_after() {
    let config = RateLimitConfig::new(1).with_window(Duration::from_millis(500));
    let app = make_rate_limit_router(config);

    // Exhaust the budget.
    let _ = app
        .clone()
        .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
        .await
        .unwrap();

    let resp = app
        .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(
        resp.headers().contains_key("Retry-After"),
        "429 response must include Retry-After header"
    );
}

// ============================================================================
// Router construction — create_pjs_router does not panic
// ============================================================================

#[test]
fn test_create_pjs_router_does_not_panic() {
    use common::{MockEventPublisher, MockRepository, MockStreamStore};

    let state = common::create_test_app_state();
    // Router construction must not panic — just consuming the result is sufficient.
    let _router: axum::Router<PjsAppState<MockRepository, MockEventPublisher, MockStreamStore>> =
        create_pjs_router::<MockRepository, MockEventPublisher, MockStreamStore>()
            .with_state(state);
}

// ── Helpers for auth router tests ─────────────────────────────────────────────

fn build_auth_router(api_key: &str) -> Router {
    use common::{MockEventPublisher, MockRepository, MockStreamStore};

    let state = common::create_test_app_state();
    let config = ApiKeyConfig::new(&[api_key]).expect("valid key");
    let auth_layer = ApiKeyAuthLayer::new(config);
    let server_config = HttpServerConfig::default();

    create_pjs_router_with_auth::<MockRepository, MockEventPublisher, MockStreamStore>(
        &server_config,
        auth_layer,
    )
    .expect("router with auth must build with default config")
    .with_state(state)
}

fn build_rate_limit_and_auth_router(api_key: &str, rate_limit: RateLimitConfig) -> Router {
    use common::{MockEventPublisher, MockRepository, MockStreamStore};

    let state = common::create_test_app_state();
    let config = ApiKeyConfig::new(&[api_key]).expect("valid key");
    let auth_layer = ApiKeyAuthLayer::new(config);
    let rate_limit_middleware = RateLimitMiddleware::new(rate_limit);
    let server_config = HttpServerConfig::default();

    create_pjs_router_with_rate_limit_and_auth::<MockRepository, MockEventPublisher, MockStreamStore>(
        &server_config,
        rate_limit_middleware,
        auth_layer,
    )
    .expect("router with rate limit and auth must build with default config")
    .with_state(state)
}

// ============================================================================
// create_pjs_router_with_auth — public vs protected routes
// ============================================================================

/// GET /pjs/health requires no auth — health is always public.
#[tokio::test]
async fn test_router_with_auth_health_no_auth_returns_200() {
    let app = build_auth_router("test-api-key");

    let req = Request::builder()
        .method(Method::GET)
        .uri("/pjs/health")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

/// GET /pjs/health with a wrong key still returns 200 — auth does not apply to public routes.
#[tokio::test]
async fn test_router_with_auth_health_wrong_key_returns_200() {
    let app = build_auth_router("test-api-key");

    let req = Request::builder()
        .method(Method::GET)
        .uri("/pjs/health")
        .header("X-PJS-API-Key", "wrong-key")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

/// POST /pjs/sessions with no auth header must return 401.
#[tokio::test]
async fn test_router_with_auth_sessions_no_auth_returns_401() {
    let app = build_auth_router("test-api-key");

    let req = Request::builder()
        .method(Method::POST)
        .uri("/pjs/sessions")
        .header("Content-Type", "application/json")
        .body(Body::from("{}"))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// POST /pjs/sessions with the wrong key must return 401.
#[tokio::test]
async fn test_router_with_auth_sessions_wrong_key_returns_401() {
    let app = build_auth_router("test-api-key");

    let req = Request::builder()
        .method(Method::POST)
        .uri("/pjs/sessions")
        .header("Content-Type", "application/json")
        .header("X-PJS-API-Key", "wrong-key")
        .body(Body::from("{}"))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// POST /pjs/sessions with the correct X-PJS-API-Key header must pass auth.
///
/// The mock repository accepts any valid session request and returns 200.
#[tokio::test]
async fn test_router_with_auth_sessions_valid_key_returns_200() {
    let app = build_auth_router("test-api-key");

    let req = Request::builder()
        .method(Method::POST)
        .uri("/pjs/sessions")
        .header("Content-Type", "application/json")
        .header("X-PJS-API-Key", "test-api-key")
        .body(Body::from("{}"))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "valid key must not be rejected by auth layer"
    );
}

/// POST /pjs/sessions with Authorization: Bearer <key> must pass auth.
#[tokio::test]
async fn test_router_with_auth_sessions_bearer_returns_200() {
    let app = build_auth_router("test-api-key");

    let req = Request::builder()
        .method(Method::POST)
        .uri("/pjs/sessions")
        .header("Content-Type", "application/json")
        .header("Authorization", "Bearer test-api-key")
        .body(Body::from("{}"))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "bearer token must not be rejected by auth layer"
    );
}

// ============================================================================
// create_pjs_router_with_rate_limit_and_auth — combined layers
// ============================================================================

/// GET /pjs/health with a generous rate limit and no auth header must return 200.
#[tokio::test]
async fn test_router_rl_auth_health_no_auth_returns_200() {
    let rate_limit = RateLimitConfig::new(1000).with_window(Duration::from_secs(60));
    let app = build_rate_limit_and_auth_router("test-api-key", rate_limit);

    let req = Request::builder()
        .method(Method::GET)
        .uri("/pjs/health")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

/// POST /pjs/sessions with a generous rate limit and no auth must return 401.
#[tokio::test]
async fn test_router_rl_auth_sessions_no_auth_returns_401() {
    let rate_limit = RateLimitConfig::new(1000).with_window(Duration::from_secs(60));
    let app = build_rate_limit_and_auth_router("test-api-key", rate_limit);

    let req = Request::builder()
        .method(Method::POST)
        .uri("/pjs/sessions")
        .header("Content-Type", "application/json")
        .body(Body::from("{}"))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// POST /pjs/sessions with a generous rate limit and valid auth must succeed.
#[tokio::test]
async fn test_router_rl_auth_sessions_valid_auth_succeeds() {
    let rate_limit = RateLimitConfig::new(1000).with_window(Duration::from_secs(60));
    let app = build_rate_limit_and_auth_router("test-api-key", rate_limit);

    let req = Request::builder()
        .method(Method::POST)
        .uri("/pjs/sessions")
        .header("Content-Type", "application/json")
        .header("X-PJS-API-Key", "test-api-key")
        .body(Body::from("{}"))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "valid key must not be rejected by auth layer"
    );
}

/// With a tight rate limit (1 request), the second request to /pjs/health must return 429.
#[tokio::test]
async fn test_router_rl_auth_health_rate_limited() {
    let rate_limit = RateLimitConfig::new(1).with_window(Duration::from_millis(500));
    let app = build_rate_limit_and_auth_router("test-api-key", rate_limit);

    let req1 = Request::builder()
        .method(Method::GET)
        .uri("/pjs/health")
        .body(Body::empty())
        .unwrap();
    let resp1 = app.clone().oneshot(req1).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::OK, "first request must pass");

    let req2 = Request::builder()
        .method(Method::GET)
        .uri("/pjs/health")
        .body(Body::empty())
        .unwrap();
    let resp2 = app.oneshot(req2).await.unwrap();
    assert_eq!(
        resp2.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "second request must be rate limited"
    );
}

/// Rate limit is the outermost layer: when exhausted, 429 is returned before auth runs.
///
/// Indirect ordering proof with a budget of 1:
/// - req1 passes rate-limit (consuming the full budget), then auth rejects it → 401.
/// - req2 hits the exhausted budget → 429 without ever reaching the auth layer.
///
/// Getting 429 on req2 proves that rate-limit consumed budget during req1, i.e. it
/// evaluated first — it is the outermost layer.
#[tokio::test]
async fn test_router_rl_auth_sessions_rate_limit_before_auth() {
    let rate_limit = RateLimitConfig::new(1).with_window(Duration::from_millis(500));
    let app = build_rate_limit_and_auth_router("test-api-key", rate_limit);

    // First request: within rate limit budget, no auth → auth layer rejects with 401.
    let req1 = Request::builder()
        .method(Method::POST)
        .uri("/pjs/sessions")
        .header("Content-Type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let resp1 = app.clone().oneshot(req1).await.unwrap();
    assert_eq!(
        resp1.status(),
        StatusCode::UNAUTHORIZED,
        "first unauthenticated request must yield 401"
    );

    // Second request: rate limit budget exhausted → rate limiter rejects with 429
    // before the auth layer even runs.
    let req2 = Request::builder()
        .method(Method::POST)
        .uri("/pjs/sessions")
        .header("Content-Type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let resp2 = app.oneshot(req2).await.unwrap();
    assert_eq!(
        resp2.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "second request must be rejected by rate limiter (outer layer) before auth"
    );
}
