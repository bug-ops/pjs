//! Axum HTTP server adapter for PJS streaming

use crate::domain::value_objects::JsonData;
use axum::{
    Json, Router,
    extract::DefaultBodyLimit,
    http::{
        HeaderValue, Method, StatusCode,
        header::{AUTHORIZATION, CONTENT_TYPE},
    },
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tower::limit::GlobalConcurrencyLimitLayer;
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    timeout::{ResponseBodyTimeoutLayer, TimeoutLayer},
    trace::TraceLayer,
};

use crate::{
    application::{
        handlers::{
            command_handlers::SessionCommandHandler,
            query_handlers::{SessionQueryHandler, StreamQueryHandler, SystemQueryHandler},
        },
        queries::SortOrder,
    },
    domain::{
        SessionState,
        aggregates::stream_session::SessionHealth,
        entities::Frame,
        ports::{
            DictionaryStore, EventPublisherGat, FrameStoreGat, NoopDictionaryStore,
            SessionSortField, StreamRepositoryGat, StreamStoreGat,
        },
        value_objects::{SessionId, StreamId},
    },
    infrastructure::{
        adapters::InMemoryFrameStore,
        http::middleware::{RateLimitMiddleware, security_middleware},
    },
};

#[cfg(all(feature = "compression", not(target_arch = "wasm32")))]
use super::handlers::dictionary::get_session_dictionary;
use super::handlers::{
    health::{get_system_stats, system_health},
    sessions::{
        create_session, get_session, get_session_stats, list_sessions, search_sessions,
        session_health,
    },
    streams::{
        create_stream, generate_frames, get_stream, get_stream_frames, start_stream,
        stream_stream_frames,
    },
};

/// HTTP server configuration.
///
/// # Production warning
///
/// `HttpServerConfig::default()` returns a configuration suitable for **local development
/// only** — it allows a single hard-coded origin (`http://localhost:3000`). Production
/// deployments must construct an explicit `HttpServerConfig` with the actual list of
/// allowed origins, or pass `vec![]` to deny all cross-origin requests.
///
/// Use [`create_pjs_router_with_config`] to apply a non-default configuration.
///
/// # Adding fields
///
/// This struct is marked `#[non_exhaustive]` so future additive fields
/// (e.g. `allow_credentials`, `max_age`) do not become breaking changes.
/// External callers cannot use the struct-init pattern; construct an instance
/// via [`HttpServerConfig::new`] or [`HttpServerConfig::default`] and mutate
/// the public fields you need.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct HttpServerConfig {
    /// List of origins allowed by the CORS layer.
    ///
    /// # Matching semantics
    ///
    /// Origins are matched against the request's `Origin` header by **case-sensitive byte
    /// equality**. This is `tower_http::cors::AllowOrigin::list` behavior; it is not the
    /// case-insensitive scheme/host comparison defined by RFC 6454 §6.
    ///
    /// In practice this matches all real browser traffic, because mainstream browsers
    /// always send lowercase scheme and host. Write your origins in lowercase.
    ///
    /// # Special values
    ///
    /// - `[]` (empty) — deny all cross-origin requests (fail-closed)
    /// - `["*"]` — allow any origin (passes through to `tower_http::cors::Any`)
    /// - Mixing `"*"` with explicit origins is rejected at construction time
    pub allowed_origins: Vec<String>,
}

impl HttpServerConfig {
    /// Construct a configuration with an explicit list of allowed CORS origins.
    ///
    /// Pass `vec![]` to deny all cross-origin requests, or `vec!["*".into()]`
    /// to allow any origin. Mixing `"*"` with explicit origins is rejected
    /// later when the CORS layer is built.
    ///
    /// # Examples
    ///
    /// ```
    /// use pjson_rs::infrastructure::http::HttpServerConfig;
    ///
    /// let config = HttpServerConfig::new(vec!["https://app.example.com".into()]);
    /// assert_eq!(config.allowed_origins.len(), 1);
    /// ```
    pub fn new(allowed_origins: Vec<String>) -> Self {
        Self { allowed_origins }
    }
}

impl Default for HttpServerConfig {
    /// Local-development default: allows `http://localhost:3000`.
    ///
    /// **Do not use this in production.** See the type-level docs.
    fn default() -> Self {
        Self {
            allowed_origins: vec!["http://localhost:3000".to_string()],
        }
    }
}

/// Build a [`CorsLayer`] from an [`HttpServerConfig`].
///
/// # Errors
///
/// Returns [`PjsError::HttpError`] if:
/// - `allowed_origins` is a mix of `"*"` and explicit origins
/// - any origin string fails to parse as a valid `HeaderValue`
fn build_cors_layer(config: &HttpServerConfig) -> Result<CorsLayer, PjsError> {
    build_cors_layer_from_origins(&config.allowed_origins)
}

/// Build a [`CorsLayer`] from a raw allowed-origins list.
///
/// Shared validated-allowlist logic behind both [`build_cors_layer`] (used by
/// [`create_pjs_router_with_config`]) and `axum_extension::PjsExtension`'s
/// own opt-in `allowed_origins` config — see that module for why it needs
/// its own CORS layer rather than always relying on [`build_cors_layer`]'s
/// caller.
///
/// # Matching semantics
///
/// - `[]` (empty) — deny all cross-origin requests (fail-closed)
/// - `["*"]` — allow any origin (passes through to `tower_http::cors::Any`)
/// - Mixing `"*"` with explicit origins is rejected at construction time
/// - Explicit origins are matched against the request's `Origin` header by
///   **case-sensitive byte equality** (`tower_http::cors::AllowOrigin::list`
///   behavior, not RFC 6454 §6's case-insensitive scheme/host comparison —
///   write origins in lowercase, which matches all real browser traffic)
///
/// # Errors
///
/// Returns [`PjsError::HttpError`] if:
/// - `allowed_origins` is a mix of `"*"` and explicit origins
/// - any origin string fails to parse as a valid `HeaderValue`
pub(crate) fn build_cors_layer_from_origins(
    allowed_origins: &[String],
) -> Result<CorsLayer, PjsError> {
    // We intentionally do NOT call .allow_credentials(true).
    // PJS does not use cookie-based auth; the Authorization header works without
    // credentials mode. allow_credentials(true) is incompatible with allow_origin(Any),
    // which would forbid the `["*"]` config path.
    let base = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([CONTENT_TYPE, AUTHORIZATION])
        .max_age(std::time::Duration::from_secs(3600));

    let has_wildcard = allowed_origins.iter().any(|o| o == "*");
    let has_explicit = allowed_origins.iter().any(|o| o != "*");

    let layer = match (allowed_origins.is_empty(), has_wildcard, has_explicit) {
        (true, _, _) => base.allow_origin(AllowOrigin::list(std::iter::empty::<HeaderValue>())),
        (_, true, true) => {
            return Err(PjsError::HttpError(
                "CORS: wildcard '*' cannot be combined with explicit origins".into(),
            ));
        }
        (_, true, false) => base.allow_origin(tower_http::cors::Any),
        (_, false, _) => {
            let origins: Vec<HeaderValue> = allowed_origins
                .iter()
                .map(|o| {
                    o.parse::<HeaderValue>()
                        .map_err(|e| PjsError::HttpError(format!("invalid CORS origin {o:?}: {e}")))
                })
                .collect::<Result<_, _>>()?;
            base.allow_origin(AllowOrigin::list(origins))
        }
    };
    Ok(layer)
}

/// Axum application state with PJS GAT-based handlers.
///
/// All fields are `pub(crate)` so the route handlers in
/// [`crate::infrastructure::http::handlers`] can access them without
/// exposing them as public API.
pub struct PjsAppState<R, P, S, F = InMemoryFrameStore>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
    F: FrameStoreGat + Send + Sync + 'static,
{
    pub(crate) command_handler: Arc<SessionCommandHandler<R, P, F>>,
    pub(crate) session_query_handler: Arc<SessionQueryHandler<R>>,
    pub(crate) stream_query_handler: Arc<StreamQueryHandler<R, S, F>>,
    pub(crate) system_handler: Arc<SystemQueryHandler<R>>,
    pub(crate) dictionary_store: Arc<dyn DictionaryStore>,
}

impl<R, P, S, F> Clone for PjsAppState<R, P, S, F>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
    F: FrameStoreGat + Send + Sync + 'static,
{
    fn clone(&self) -> Self {
        Self {
            command_handler: self.command_handler.clone(),
            session_query_handler: self.session_query_handler.clone(),
            stream_query_handler: self.stream_query_handler.clone(),
            system_handler: self.system_handler.clone(),
            dictionary_store: self.dictionary_store.clone(),
        }
    }
}

impl<R, P, S> PjsAppState<R, P, S, InMemoryFrameStore>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    /// Create a new application state with default [`NoopDictionaryStore`] and
    /// an in-memory frame store.
    ///
    /// The `/pjs/sessions/{id}/dictionary` endpoint will return 404 until
    /// you upgrade to [`PjsAppState::with_dictionary_store`] with a concrete
    /// implementation such as [`crate::infrastructure::repositories::InMemoryDictionaryStore`].
    ///
    /// Records the current instant as the process start time for uptime reporting.
    pub fn new(repository: Arc<R>, event_publisher: Arc<P>, stream_store: Arc<S>) -> Self {
        Self::with_dictionary_store(
            repository,
            event_publisher,
            stream_store,
            Arc::new(NoopDictionaryStore),
        )
    }

    /// Create a new application state with a custom [`DictionaryStore`] and an
    /// in-memory frame store.
    ///
    /// Pass `Arc::new(InMemoryDictionaryStore::new(...))` to enable end-to-end
    /// dictionary training and serving.
    pub fn with_dictionary_store(
        repository: Arc<R>,
        event_publisher: Arc<P>,
        stream_store: Arc<S>,
        dictionary_store: Arc<dyn DictionaryStore>,
    ) -> Self {
        Self::with_stores(
            repository,
            event_publisher,
            stream_store,
            dictionary_store,
            Arc::new(InMemoryFrameStore::new()),
        )
    }
}

impl<R, P, S, F> PjsAppState<R, P, S, F>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
    F: FrameStoreGat + Send + Sync + 'static,
{
    /// Create a new application state with custom [`DictionaryStore`] and
    /// [`FrameStoreGat`] implementations.
    pub fn with_stores(
        repository: Arc<R>,
        event_publisher: Arc<P>,
        stream_store: Arc<S>,
        dictionary_store: Arc<dyn DictionaryStore>,
        frame_store: Arc<F>,
    ) -> Self {
        let started_at = Instant::now();
        Self {
            command_handler: Arc::new(SessionCommandHandler::with_stores(
                repository.clone(),
                event_publisher,
                dictionary_store.clone(),
                frame_store.clone(),
            )),
            session_query_handler: Arc::new(SessionQueryHandler::new(repository.clone())),
            stream_query_handler: Arc::new(StreamQueryHandler::new(
                repository.clone(),
                stream_store,
                frame_store,
            )),
            system_handler: Arc::new(SystemQueryHandler::with_start_time(repository, started_at)),
            dictionary_store,
        }
    }
}

/// Request to create a new streaming session
///
/// `max_concurrent_streams: 0`, `timeout_seconds: 0`, or a `timeout_seconds`
/// above [`crate::domain::config::limits::MAX_SESSION_TIMEOUT_SECONDS`] (7
/// days) are rejected with `400 Bad Request` before the session is created.
#[derive(Debug, Deserialize)]
pub struct CreateSessionRequest {
    /// Maximum number of streams the session is allowed to host concurrently.
    pub max_concurrent_streams: Option<usize>,
    /// Idle timeout for the session, in seconds.
    pub timeout_seconds: Option<u64>,
    /// Optional human-readable client identifier.
    pub client_info: Option<String>,
}

/// Response for session creation
#[derive(Debug, Serialize)]
pub struct CreateSessionResponse {
    /// Newly assigned session identifier.
    pub session_id: String,
    /// Wall-clock instant after which the session expires.
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

/// Request to start streaming data
#[derive(Debug, Deserialize)]
pub struct StartStreamRequest {
    /// JSON payload to be decomposed into priority frames.
    ///
    /// A `null` payload is rejected with `400 Bad Request` before the
    /// session is looked up.
    pub data: JsonData,
    /// Minimum frame priority to emit; lower-priority frames are dropped.
    pub priority_threshold: Option<u8>,
    /// Maximum number of frames to emit before the stream is closed.
    pub max_frames: Option<usize>,
}

/// Stream response parameters
#[derive(Debug, Deserialize)]
pub struct StreamParams {
    /// Identifier of the streaming session.
    pub session_id: String,
    /// Optional minimum priority filter applied to emitted frames.
    pub priority: Option<u8>,
    /// Optional response format selector (for example, `"json"` or `"sse"`).
    pub format: Option<String>,
}

/// Request body for generating priority-filtered frames on an existing stream.
///
/// Both fields are optional; defaults match the lowest-cost configuration that
/// still drives the priority pipeline:
/// - `priority_threshold` defaults to [`crate::domain::value_objects::Priority::BACKGROUND`] (10) — accepts every frame.
/// - `max_frames` defaults to 16 — bounded so a single request cannot emit an
///   unbounded number of frames.
///
/// An explicit `max_frames` of `0` or above
/// [`crate::domain::config::limits::MAX_FRAMES_PER_REQUEST`] (1000) is
/// rejected with `400 Bad Request`.
#[derive(Debug, Default, Deserialize)]
pub struct GenerateFramesRequest {
    /// Minimum frame priority to emit; lower-priority frames are dropped.
    pub priority_threshold: Option<u8>,
    /// Maximum number of frames to emit in this request.
    pub max_frames: Option<usize>,
}

/// Response body for `POST .../streams/{stream_id}/generate-frames`.
///
/// Returns the frames produced by the stream's priority extractor, in the
/// same shape as `GET .../frames` but freshly generated (and fed into the
/// per-session dictionary training corpus when the `compression` feature
/// is enabled).
#[derive(Debug, Serialize)]
pub struct GenerateFramesResponse {
    /// Frames produced by the priority extractor in this request.
    pub frames: Vec<Frame>,
    /// Number of frames returned (always equal to `frames.len()`).
    pub frame_count: usize,
}

/// Session health response
#[derive(Debug, Serialize)]
pub struct SessionHealthResponse {
    /// Aggregate health flag derived from rates and recent activity.
    pub is_healthy: bool,
    /// Number of streams currently in an active state.
    pub active_streams: usize,
    /// Number of streams that have terminated with an error.
    pub failed_streams: usize,
    /// Whether the session has passed its expiry instant.
    pub is_expired: bool,
    /// Number of seconds since the session was created.
    pub uptime_seconds: i64,
}

impl From<SessionHealth> for SessionHealthResponse {
    fn from(health: SessionHealth) -> Self {
        Self {
            is_healthy: health.is_healthy,
            active_streams: health.active_streams,
            failed_streams: health.failed_streams,
            is_expired: health.is_expired,
            uptime_seconds: health.uptime_seconds,
        }
    }
}

/// Create PJS-enabled Axum router with the default CORS configuration.
///
/// Uses [`HttpServerConfig::default`] which allows `http://localhost:3000`.
///
/// # Security Note
///
/// This is suitable for local development only. For production, use
/// [`create_pjs_router_with_config`] with an explicit [`HttpServerConfig`], and
/// apply authentication via [`create_pjs_router_with_auth`] or
/// [`create_pjs_router_with_rate_limit_and_auth`] — API key and JWT layers are
/// available in [`crate::infrastructure::http::auth`]
/// (`ApiKeyAuthLayer`, `JwtAuthLayer`).
pub fn create_pjs_router<R, P, S>() -> Router<PjsAppState<R, P, S>>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    create_pjs_router_with_config::<R, P, S>(&HttpServerConfig::default())
        .expect("default HttpServerConfig must always produce a valid CORS layer")
}

/// Create PJS-enabled Axum router with a custom [`HttpServerConfig`].
///
/// # Errors
///
/// Returns [`PjsError::HttpError`] if `config` contains invalid CORS origins —
/// specifically, when `allowed_origins` mixes `"*"` with explicit origins, or
/// any origin string fails to parse as a valid `HeaderValue`.
///
/// # Examples
///
/// ```rust,ignore
/// use pjson_rs::infrastructure::http::{HttpServerConfig, create_pjs_router_with_config};
///
/// let config = HttpServerConfig::new(vec!["https://app.example.com".to_string()]);
/// let router = create_pjs_router_with_config::<R, P, S>(&config)?;
/// ```
pub fn create_pjs_router_with_config<R, P, S>(
    config: &HttpServerConfig,
) -> Result<Router<PjsAppState<R, P, S>>, PjsError>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    let all_routes = public_routes::<R, P, S>().merge(protected_routes::<R, P, S>());
    apply_common_layers(all_routes, config, None)
}

/// Create PJS-enabled Axum router with rate limiting and the default CORS configuration.
///
/// Adds rate limiting middleware to protect against DoS attacks.
/// Default: 100 requests per minute per IP address.
///
/// Uses [`HttpServerConfig::default`] which allows `http://localhost:3000`.
/// For production, use [`create_pjs_router_with_rate_limit_and_config`].
///
/// # Security Note
///
/// Rate limiting is applied globally to all endpoints, keyed on the real TCP
/// peer address by default — the router must be served with
/// `into_make_service_with_connect_info::<std::net::SocketAddr>()` (as done for
/// the WebSocket upgrade handler) so that peer address is populated; otherwise
/// every request falls back to the same key (`127.0.0.1`). To trust
/// `X-Forwarded-For`/`X-Real-IP` behind a reverse proxy, opt in via
/// [`RateLimitConfig::with_trusted_proxies`](crate::infrastructure::http::middleware::RateLimitConfig::with_trusted_proxies).
/// Returns 429 Too Many Requests with Retry-After header when limit exceeded.
/// Adds X-RateLimit-* headers per RFC 6585.
pub fn create_pjs_router_with_rate_limit<R, P, S>(
    rate_limit_middleware: RateLimitMiddleware,
) -> Router<PjsAppState<R, P, S>>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    create_pjs_router_with_rate_limit_and_config::<R, P, S>(
        &HttpServerConfig::default(),
        rate_limit_middleware,
    )
    .expect("default HttpServerConfig must always produce a valid CORS layer")
}

/// Create PJS-enabled Axum router with rate limiting and a custom [`HttpServerConfig`].
///
/// `rate_limit_middleware` is threaded into the crate's common middleware stack:
/// inside `security_middleware`/`CorsLayer`/`TraceLayer`, so a `429` still gets
/// security headers, CORS headers, and shows up in traces, but outside the global
/// concurrency limiter, so a `429` never consumes a permit.
///
/// # Errors
///
/// Returns [`PjsError::HttpError`] if `config` contains invalid CORS origins.
pub fn create_pjs_router_with_rate_limit_and_config<R, P, S>(
    config: &HttpServerConfig,
    rate_limit_middleware: RateLimitMiddleware,
) -> Result<Router<PjsAppState<R, P, S>>, PjsError>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    let all_routes = public_routes::<R, P, S>().merge(protected_routes::<R, P, S>());
    apply_common_layers(all_routes, config, Some(rate_limit_middleware))
}

/// Create PJS-enabled Axum router with API key authentication and a custom [`HttpServerConfig`].
///
/// The health endpoint (`/pjs/health`) is **not** protected by auth — it lives in a
/// separate public sub-router that is merged without the auth layer. All other routes
/// require a valid API key.
///
/// # Errors
///
/// Returns [`PjsError::HttpError`] if `config` contains invalid CORS origins.
///
/// # Examples
///
/// ```rust,ignore
/// use pjson_rs::infrastructure::http::{
///     HttpServerConfig, auth::{ApiKeyConfig, ApiKeyAuthLayer},
///     create_pjs_router_with_auth,
/// };
///
/// let api_config = ApiKeyConfig::new(&["my-api-key"])?;
/// let auth_layer = ApiKeyAuthLayer::new(api_config);
/// let config = HttpServerConfig::default();
/// let router = create_pjs_router_with_auth::<R, P, S>(&config, auth_layer)?;
/// ```
#[cfg(feature = "http-server")]
pub fn create_pjs_router_with_auth<R, P, S>(
    config: &HttpServerConfig,
    auth: crate::infrastructure::http::auth::ApiKeyAuthLayer,
) -> Result<Router<PjsAppState<R, P, S>>, PjsError>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    // Auth wraps only the protected sub-router. Public routes (health, metrics) are
    // merged separately so there is zero path-string comparison logic in the auth layer.
    let protected = protected_routes::<R, P, S>().layer(auth);
    let merged = public_routes::<R, P, S>().merge(protected);
    apply_common_layers(merged, config, None)
}

/// Create PJS-enabled Axum router with both rate limiting and API key authentication.
///
/// Layer ordering (axum's `Router::layer` makes the last `.layer()` call outermost):
/// ```text
/// TraceLayer                  ← outermost: distributed tracing
/// TimeoutLayer                ← whole-request timeout
/// ResponseBodyTimeoutLayer    ← per-frame idle timeout on the response body
/// CorsLayer                   ← CORS
/// DefaultBodyLimit            ← body size guard
/// security_middleware         ← security headers
/// rate_limit                  ← rejects with 429 before a concurrency permit, but
///                                after security/CORS/trace so 429s keep all three
///   GlobalConcurrencyLimitLayer ← global in-flight request cap
///     public_routes (no auth)
///     protected_routes
///       auth    ← innermost: wraps only protected routes
///       handlers
/// ```
///
/// Rate limiting is applied to **both** the public and protected sub-routers (DoS
/// protection for `/pjs/health` is still desirable). Rate limit sits *outside* auth
/// (auth is applied to the protected sub-router before this router ever reaches
/// the common middleware stack, so it ends up innermost of everything) — every
/// request, authenticated or not, consumes rate-limit quota before auth gets a
/// chance to reject the unauthenticated ones. This is an intentional trade-off, not
/// an oversight: it is what lets the same rate limiter also protect the
/// unauthenticated `/pjs/health` route, at the cost of an unauthenticated flood
/// being able to consume quota that would otherwise be available to legitimate
/// authenticated clients.
///
/// # Errors
///
/// Returns [`PjsError::HttpError`] if `config` contains invalid CORS origins.
#[cfg(feature = "http-server")]
pub fn create_pjs_router_with_rate_limit_and_auth<R, P, S>(
    config: &HttpServerConfig,
    rate_limit: RateLimitMiddleware,
    auth: crate::infrastructure::http::auth::ApiKeyAuthLayer,
) -> Result<Router<PjsAppState<R, P, S>>, PjsError>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    let protected = protected_routes::<R, P, S>().layer(auth);
    let merged = public_routes::<R, P, S>().merge(protected);
    apply_common_layers(merged, config, Some(rate_limit))
}

// ── Route table helpers ────────────────────────────────────────────────────────────

/// Routes that are always public — no authentication applied.
///
/// Currently: `/pjs/health` and (when the `metrics` feature is enabled) `/metrics`.
fn public_routes<R, P, S>() -> Router<PjsAppState<R, P, S>>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    let router = Router::new().route("/pjs/health", get(system_health));

    #[cfg(feature = "metrics")]
    let router = router.route(
        "/metrics",
        get(crate::infrastructure::http::metrics::metrics_handler),
    );

    router
}

/// Routes that require authentication when an auth layer is applied.
fn protected_routes<R, P, S>() -> Router<PjsAppState<R, P, S>>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    let router = Router::new()
        .route("/pjs/sessions", post(create_session::<R, P, S>))
        .route("/pjs/sessions/{session_id}", get(get_session::<R, P, S>))
        .route(
            "/pjs/sessions/{session_id}/health",
            get(session_health::<R, P, S>),
        )
        .route(
            "/pjs/sessions/{session_id}/stats",
            get(get_session_stats::<R, P, S>),
        )
        .route(
            "/pjs/sessions/{session_id}/streams",
            post(create_stream::<R, P, S>),
        )
        .route(
            "/pjs/sessions/{session_id}/streams/{stream_id}/start",
            post(start_stream::<R, P, S>),
        )
        .route(
            "/pjs/sessions/{session_id}/streams/{stream_id}/generate-frames",
            post(generate_frames::<R, P, S>),
        )
        .route(
            "/pjs/sessions/{session_id}/streams/{stream_id}",
            get(get_stream::<R, P, S>),
        )
        .route(
            "/pjs/sessions/{session_id}/streams/{stream_id}/frames",
            get(get_stream_frames::<R, P, S>),
        )
        .route(
            "/pjs/sessions/{session_id}/streams/{stream_id}/frames/stream",
            get(stream_stream_frames::<R, P, S>),
        )
        .route("/pjs/sessions/search", get(search_sessions::<R, P, S>))
        .route("/pjs/sessions", get(list_sessions::<R, P, S>))
        .route("/pjs/stats", get(get_system_stats::<R, P, S>));

    #[cfg(all(feature = "compression", not(target_arch = "wasm32")))]
    let router = router.route(
        "/pjs/sessions/{session_id}/dictionary",
        get(get_session_dictionary::<R, P, S>),
    );

    router
}

/// Global cap on concurrent in-flight requests, independent of
/// [`RateLimitMiddleware`]'s per-client, per-window token bucket.
///
/// Enforced via [`GlobalConcurrencyLimitLayer`], not the plain (non-`Global`)
/// `ConcurrencyLimitLayer`: axum's `Router::layer` applies a layer once per matched
/// route in the routing table (`PathRouter::layer` calls `layer.clone()` per route),
/// and `ConcurrencyLimitLayer::layer()` constructs a brand new `Semaphore` on every
/// call — so using it here would silently produce one independent semaphore *per
/// route* (an effective ceiling of `MAX_CONCURRENT_REQUESTS * route_count`, not a
/// real global cap). `GlobalConcurrencyLimitLayer` holds a single `Arc<Semaphore>`
/// in the layer itself and clones the `Arc` (not the semaphore) on each per-route
/// application, so every route actually shares one pool.
///
/// This bounds handler *execution* concurrency only — not connections, sockets, or
/// parsed-request memory. Axum's `Router::poll_ready` always returns `Ready`, and
/// hyper's `TowerToHyperService` wraps every request in a fresh `Oneshot`, so hyper
/// never observes tower-stack readiness and keeps accepting, reading, and parsing
/// requests regardless of how many permits are free. A request over
/// `MAX_CONCURRENT_REQUESTS` is already fully accepted/read/parsed by the time it
/// reaches this layer; it then parks waiting for a permit inside its own
/// per-request future, bounded only by the outer `TimeoutLayer` (`REQUEST_TIMEOUT`
/// -> `408`) rather than being deferred at the connection level. For the streaming
/// route (`GET .../frames/stream`), the permit is released as soon as the handler
/// returns its `Response` — i.e. once the streaming body starts, not once it
/// finishes — so this bounds concurrent *request handling*, not concurrent open
/// streaming bodies; there is currently no mechanism here that bounds the latter
/// (see [`RESPONSE_BODY_IDLE_TIMEOUT`]'s doc for why that gap remains open).
const MAX_CONCURRENT_REQUESTS: usize = 512;

/// Whole-request timeout: bounds the time from receiving a request to the handler
/// producing a `Response` (headers + body constructor), after which the client gets
/// `408 Request Timeout`.
///
/// [`TimeoutLayer`] times the `Service::call` future only — for the streaming route
/// that future resolves as soon as `create_streaming_response` builds the chunked
/// `Response`, before any frame is written, so this never cuts off an
/// already-streaming connection. It exists to bound the (normally sub-second)
/// query/domain-lookup phase common to every route, and — because it sits outer of
/// [`MAX_CONCURRENT_REQUESTS`]'s layer in [`apply_common_layers`] — also bounds how
/// long a request can queue waiting for a concurrency permit before failing with
/// `408` instead of queueing indefinitely.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Idle timeout applied to every response body: [`TimeoutBody`]'s deadline resets
/// each time the body is *polled* and yields a frame, so this bounds a stall on the
/// *producer* side (a source that stops yielding frames), not total transfer time.
///
/// [`TimeoutBody`]: tower_http::timeout::TimeoutBody
///
/// This does **not** protect against a slow or non-reading *consumer* — the
/// scenario the original #515 report was about. `TimeoutBody` only resets its clock
/// when polled, and hyper stops polling a response body once its outbound buffer
/// fills waiting on the client to read the socket, so a client that stops reading
/// entirely is never caught by this layer; that requires connection/socket-level
/// accounting in whatever owns the `TcpListener` and calls `axum::serve` (currently
/// `pjs-demo`, not `pjs-core` — this crate only builds `Router`s), tracked as a
/// follow-up rather than fixed here.
///
/// On the current streaming route (`GET .../frames/stream`, #511) this layer is
/// close to a no-op even for the producer-stall case it does cover:
/// `stream_stream_frames` fully materializes its frames into a `Vec` (bounded by
/// `MAX_PAGINATION_LIMIT`) before streaming begins, and `BatchFrameStream` then
/// iterates that already-in-memory `Vec` — there is no upstream source that can
/// actually stall mid-stream on this route today. This layer still has value for
/// any future or other route whose data source can genuinely stall while producing
/// (e.g. a backpressured or slow upstream), and is retained as a defensible general
/// mitigation rather than removed.
const RESPONSE_BODY_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// Apply the cross-cutting middleware stack shared by all router variants.
///
/// `rate_limit` is optional because only the `*_with_rate_limit_*` router
/// constructors have one to apply; `None` simply omits that layer from the stack.
///
/// Order — axum's `Router::layer` re-wraps whatever was built by earlier `.layer()`
/// calls, so the **last** `.layer()` call ends up outermost (sees the request
/// first, the response last):
/// ```text
/// TraceLayer                  ← distributed tracing (outermost)
/// TimeoutLayer                ← whole-request timeout (pre-response phase only)
/// ResponseBodyTimeoutLayer    ← per-frame idle timeout on the response body
/// CorsLayer                   ← CORS (outside auth, so preflight is answered before auth)
/// DefaultBodyLimit            ← body size guard
/// security_middleware         ← security headers
/// rate_limit                  ← per-client quota (only when `Some`)
/// GlobalConcurrencyLimitLayer ← global in-flight request cap (innermost)
/// ```
///
/// `rate_limit` sits *inside* `security_middleware`/`CorsLayer`/`TraceLayer` and
/// *outside* `GlobalConcurrencyLimitLayer`, which is deliberate on both sides:
/// - Inside security/CORS/trace: a `429` still gets security headers
///   (`X-Content-Type-Options`, `X-Frame-Options`, CSP), a browser making a
///   cross-origin request still gets a readable `429`+`Retry-After` instead of an
///   opaque CORS network error, and rate-limit rejections still show up in request
///   traces — losing any of these on a rejection path defeats the point of a DoS
///   mitigation feature. An earlier revision of this function had `rate_limit`
///   applied by the caller after this function returned (i.e. outermost of
///   everything), which regressed exactly these three properties; that version is
///   not what ships.
/// - Outside the concurrency limiter: a request the rate limiter rejects with
///   `429` never reaches (and never consumes) a `GlobalConcurrencyLimitLayer`
///   permit, and `GlobalConcurrencyLimitLayer` being innermost overall means a
///   request that *does* pass the rate limiter but then queues for a permit is
///   still bounded by the outer `TimeoutLayer`'s 30s deadline, so a saturated pool
///   degrades to `408`s instead of queueing forever.
///
/// Relative order between `ResponseBodyTimeoutLayer` and `TimeoutLayer` does not
/// affect correctness despite `TimeoutLayer` requiring its inner response body to
/// implement `Default`: axum's `Route` re-boxes every layer's output back into the
/// canonical `axum::body::Body`-based `Response` (`Route::new`'s `MapIntoResponse`)
/// before the next `.layer()` call ever sees it, so each `.layer()` call always
/// observes a plain, `Default`-implementing `axum::body::Body`, regardless of what
/// came before it in this list.
fn apply_common_layers<R, P, S>(
    router: Router<PjsAppState<R, P, S>>,
    config: &HttpServerConfig,
    rate_limit: Option<RateLimitMiddleware>,
) -> Result<Router<PjsAppState<R, P, S>>, PjsError>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    let cors = build_cors_layer(config)?;
    let router = router.layer(GlobalConcurrencyLimitLayer::new(MAX_CONCURRENT_REQUESTS));
    let router = match rate_limit {
        Some(rate_limit) => router.layer(rate_limit),
        None => router,
    };
    Ok(router
        .layer(middleware::from_fn(security_middleware))
        .layer(DefaultBodyLimit::max(10 * 1024 * 1024))
        .layer(cors)
        .layer(ResponseBodyTimeoutLayer::new(RESPONSE_BODY_IDLE_TIMEOUT))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            REQUEST_TIMEOUT,
        ))
        .layer(TraceLayer::new_for_http()))
}

/// Parse a raw path segment into a [`SessionId`], mapping failure to [`PjsError::InvalidSessionId`].
pub(crate) fn parse_session_id(raw: String) -> Result<SessionId, PjsError> {
    SessionId::from_string(&raw).map_err(|_| PjsError::InvalidSessionId(raw))
}

/// Parse raw `(session_id, stream_id)` path segments, mapping failures to the matching
/// [`PjsError::InvalidSessionId`] / [`PjsError::InvalidStreamId`] variant.
pub(crate) fn parse_session_and_stream_id(
    session_raw: String,
    stream_raw: String,
) -> Result<(SessionId, StreamId), PjsError> {
    let session_id = parse_session_id(session_raw)?;
    let stream_id =
        StreamId::from_string(&stream_raw).map_err(|_| PjsError::InvalidStreamId(stream_raw))?;
    Ok((session_id, stream_id))
}

/// Parse a raw `state` query-string value into a [`SessionState`], mapping failure to
/// [`PjsError::InvalidSessionState`].
///
/// Kept as a raw `String` on [`SearchSessionsParams`] (rather than typing the field itself
/// as `SessionState`) so a bad value is rejected here, inside the handler, with the API's
/// standard JSON error envelope — not by axum's `Query` extractor, which fails before the
/// handler runs and responds with a plain-text body inconsistent with every other 4xx this
/// API returns. Accepts only the exact spellings [`SessionState`] serializes as (e.g.
/// `"Active"`), matching [`SessionState::as_str`] — lowercase or mixed-case input is
/// rejected, unlike the pre-#414 substring/case-insensitive repository match.
pub(crate) fn parse_session_state(raw: String) -> Result<SessionState, PjsError> {
    serde_json::from_value(serde_json::Value::String(raw.clone()))
        .map_err(|_| PjsError::InvalidSessionState(raw))
}

/// Parse a raw `sort_by` query-string value into a [`SessionSortField`], mapping failure to
/// [`PjsError::InvalidSortField`].
///
/// Kept as a raw `String` on [`SearchSessionsParams`] for the same reason as
/// [`parse_session_state`]: a bad value is rejected here, inside the handler, with the API's
/// standard JSON error envelope rather than axum's `Query` extractor's plain-text rejection.
/// Delegates to [`SessionSortField`]'s `#[serde(rename_all = "snake_case")]` derive, matching
/// its exact serialized spellings (e.g. `created_at`).
pub(crate) fn parse_sort_field(raw: String) -> Result<SessionSortField, PjsError> {
    serde_json::from_value(serde_json::Value::String(raw.clone()))
        .map_err(|_| PjsError::InvalidSortField(raw))
}

/// Parse a raw `sort_order` query-string value into a [`SortOrder`], mapping failure to
/// [`PjsError::InvalidSortOrder`].
///
/// Kept as a raw `String` on [`SearchSessionsParams`] for the same reason as
/// [`parse_sort_field`]: a bad value is rejected here, inside the handler, with the API's
/// standard JSON error envelope rather than axum's `Query` extractor's plain-text rejection.
/// Delegates to [`SortOrder`]'s `#[serde(rename_all = "snake_case")]` derive, which also
/// carries `#[serde(alias = "asc")]`/`#[serde(alias = "desc")]` on its variants — so both the
/// long (`ascending`/`descending`) and short (`asc`/`desc`) spellings are accepted.
pub(crate) fn parse_sort_order(raw: String) -> Result<SortOrder, PjsError> {
    serde_json::from_value(serde_json::Value::String(raw.clone()))
        .map_err(|_| PjsError::InvalidSortOrder(raw))
}

/// Pagination parameters
#[derive(Debug, Deserialize)]
pub struct PaginationParams {
    /// Maximum number of items to return.
    pub limit: Option<usize>,
    /// Number of items to skip before returning results.
    pub offset: Option<usize>,
}

/// Query parameters for session search endpoint.
#[derive(Debug, Deserialize)]
pub struct SearchSessionsParams {
    /// Match sessions whose state equals this value.
    ///
    /// Must be one of [`SessionState`]'s exact serialized spellings, case-sensitive —
    /// `Initializing`, `Active`, `Closing`, `Completed`, or `Failed` — or the request is
    /// rejected with `400`. Parsed via an internal helper rather than typed directly, so
    /// the rejection goes through the API's standard JSON error envelope instead of axum's
    /// raw `Query`-extractor rejection body.
    pub state: Option<String>,
    /// Field name to sort by. Must be one of [`SessionSortField`]'s exact serialized
    /// spellings — `created_at`, `updated_at`, `stream_count`, `total_bytes` — or the
    /// request is rejected with `400`. Parsed via `parse_sort_field` rather than typed
    /// directly, so the rejection goes through the API's standard JSON error envelope
    /// instead of axum's raw `Query`-extractor rejection body. An empty value (`?sort_by=`)
    /// is also rejected with `400`, same as `parse_session_state` treats an empty
    /// `?state=` — it is not treated as "absent". Omitting the parameter entirely still
    /// yields `None` (no sort applied), which continues to return `200`.
    pub sort_by: Option<String>,
    /// Sort direction. Must be `"asc"`, `"ascending"`, `"desc"`, or `"descending"`,
    /// case-sensitive.
    ///
    /// Same treatment as `sort_by`: parsed via `parse_sort_order` rather than typed
    /// directly, so an unrecognized or empty value is rejected with `400` through the
    /// API's standard JSON error envelope instead of being silently ignored or falling
    /// through axum's raw `Query`-extractor rejection body. Omitting the parameter
    /// entirely still yields `None` (default sort order), which continues to return `200`.
    pub sort_order: Option<String>,
    /// Maximum number of sessions to return.
    pub limit: Option<usize>,
    /// Number of sessions to skip before returning results.
    pub offset: Option<usize>,
}

/// Query parameters for frame listing
#[derive(Debug, Deserialize)]
pub struct FrameQueryParams {
    /// Return only frames whose sequence number is greater than this value.
    pub since_sequence: Option<u64>,
    /// Return only frames whose priority satisfies this filter.
    pub priority: Option<u8>,
    /// Maximum number of frames to return.
    pub limit: Option<usize>,
}

// HTTP rate limiting is implemented by `RateLimitMiddleware`
// (crate::infrastructure::http::middleware), wired in via
// `create_pjs_router_with_rate_limit[_and_config]` and
// `create_pjs_router_with_rate_limit_and_auth` above. It keys on the real
// ConnectInfo<SocketAddr> peer address by default; see
// `RateLimitConfig::with_trusted_proxies` to opt in to trusting
// X-Forwarded-For/X-Real-IP behind a known reverse proxy.

/// PJS-specific errors for HTTP endpoints
#[derive(Debug, thiserror::Error)]
pub enum PjsError {
    /// Wraps an application-layer error returned by a CQRS handler.
    #[error("Application error: {0}")]
    Application(#[from] crate::application::ApplicationError),

    /// Provided session identifier is malformed or not a valid UUID.
    #[error("Invalid session ID: {0}")]
    InvalidSessionId(String),

    /// Provided stream identifier is malformed or not a valid UUID.
    #[error("Invalid stream ID: {0}")]
    InvalidStreamId(String),

    /// Priority value is out of range or otherwise invalid.
    #[error("Invalid priority: {0}")]
    InvalidPriority(String),

    /// Provided session state filter does not match any `SessionState` variant.
    #[error(
        "Invalid session state: {0} (expected one of: Initializing, Active, Closing, Completed, Failed)"
    )]
    InvalidSessionState(String),

    /// Provided `sort_by` value does not match any `SessionSortField` variant.
    #[error(
        "Invalid sort field: {0} (expected one of: created_at, updated_at, stream_count, total_bytes)"
    )]
    InvalidSortField(String),

    /// Provided `sort_order` value does not match any recognized sort direction.
    #[error("Invalid sort order: {0} (expected one of: asc, ascending, desc, descending)")]
    InvalidSortOrder(String),

    /// Generic HTTP-layer error not covered by other variants.
    ///
    /// # Invariant
    ///
    /// For any construction site reachable while handling a request (i.e.
    /// the error can end up in a response sent to an HTTP client), the
    /// wrapped `String` **must never** carry the `Display` output of a
    /// wrapped or foreign error — that text may contain paths, connection
    /// details, or other internals. Log the real error server-side (e.g. via
    /// `tracing::error!`) and construct this variant with a generic,
    /// client-safe message instead. Known channels that can leak the
    /// wrapped string to a client: this type's `IntoResponse` implementation
    /// (below), and any handler that builds a response body directly from
    /// the error (e.g. `metrics_handler` in
    /// `crate::infrastructure::http::metrics`, which bypasses
    /// `IntoResponse`).
    ///
    /// The construction sites in `build_cors_layer` (private, this module)
    /// are the intentional exemption: they run at router-build time from
    /// operator-supplied config, before any request is served, and their
    /// `PjsError` is never routed into a response — a build failure aborts
    /// server startup.
    #[error("HTTP error: {0}")]
    HttpError(String),
}

impl IntoResponse for PjsError {
    fn into_response(self) -> Response {
        let (status, error_message) = match &self {
            PjsError::Application(app_err) => {
                use crate::application::ApplicationError;
                let status = match app_err {
                    ApplicationError::NotFound(_) => StatusCode::NOT_FOUND,
                    ApplicationError::Validation(_) => StatusCode::BAD_REQUEST,
                    ApplicationError::Authorization(_) => StatusCode::UNAUTHORIZED,
                    ApplicationError::Concurrency(_) | ApplicationError::Conflict(_) => {
                        StatusCode::CONFLICT
                    }
                    ApplicationError::Domain(_) | ApplicationError::Logic(_) => {
                        StatusCode::INTERNAL_SERVER_ERROR
                    }
                };
                (status, self.to_string())
            }
            PjsError::InvalidSessionId(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            PjsError::InvalidStreamId(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            PjsError::InvalidPriority(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            PjsError::InvalidSessionState(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            PjsError::InvalidSortField(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            PjsError::InvalidSortOrder(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            PjsError::HttpError(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };

        let body = Json(serde_json::json!({
            "error": error_message
        }));

        (status, body).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::header;

    // --- build_cors_layer unit tests ---

    #[test]
    fn cors_empty_origins_denies_all() {
        let config = HttpServerConfig {
            allowed_origins: vec![],
        };
        // Empty list must succeed (returns a layer that denies all origins).
        let result = build_cors_layer(&config);
        assert!(
            result.is_ok(),
            "empty origins should return Ok (deny-all layer)"
        );
    }

    #[test]
    fn cors_wildcard_only_is_ok() {
        let config = HttpServerConfig {
            allowed_origins: vec!["*".to_string()],
        };
        let result = build_cors_layer(&config);
        assert!(result.is_ok(), "wildcard-only should return Ok");
    }

    #[test]
    fn cors_mixed_wildcard_and_explicit_is_err() {
        let config = HttpServerConfig {
            allowed_origins: vec!["*".to_string(), "http://example.com".to_string()],
        };
        let result = build_cors_layer(&config);
        assert!(
            result.is_err(),
            "mixing wildcard with explicit origins must fail"
        );
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("wildcard"),
            "error message should mention wildcard: {msg}"
        );
    }

    #[test]
    fn cors_valid_single_origin_is_ok() {
        let config = HttpServerConfig {
            allowed_origins: vec!["http://example.com".to_string()],
        };
        assert!(build_cors_layer(&config).is_ok());
    }

    #[test]
    fn cors_valid_multiple_origins_is_ok() {
        let config = HttpServerConfig {
            allowed_origins: vec![
                "https://app.example.com".to_string(),
                "https://admin.example.com".to_string(),
            ],
        };
        assert!(build_cors_layer(&config).is_ok());
    }

    #[test]
    fn cors_invalid_origin_string_is_err() {
        let config = HttpServerConfig {
            // HeaderValue rejects strings containing control characters / invalid bytes.
            allowed_origins: vec!["not a\nvalid header".to_string()],
        };
        let result = build_cors_layer(&config);
        assert!(result.is_err(), "invalid origin string must return Err");
    }

    #[test]
    fn default_config_is_valid() {
        // Guarantees that the expect() in create_pjs_router / create_pjs_router_with_rate_limit
        // will never panic at runtime.
        assert!(
            build_cors_layer(&HttpServerConfig::default()).is_ok(),
            "default HttpServerConfig must produce a valid CORS layer"
        );
    }

    // --- parse_session_id / parse_session_and_stream_id unit tests ---

    #[test]
    fn parse_session_id_valid_roundtrips() {
        let id = SessionId::new();
        let parsed = parse_session_id(id.to_string()).expect("valid uuid must parse");
        assert_eq!(parsed, id);
    }

    #[test]
    fn parse_session_id_invalid_returns_invalid_session_id_error() {
        let raw = "not-a-valid-uuid".to_string();
        let err = parse_session_id(raw.clone()).unwrap_err();
        match err {
            PjsError::InvalidSessionId(msg) => assert_eq!(msg, raw),
            other => panic!("expected InvalidSessionId, got {other:?}"),
        }
    }

    #[test]
    fn parse_session_and_stream_id_valid_roundtrips() {
        let session_id = SessionId::new();
        let stream_id = StreamId::new();
        let (parsed_session, parsed_stream) =
            parse_session_and_stream_id(session_id.to_string(), stream_id.to_string())
                .expect("valid uuids must parse");
        assert_eq!(parsed_session, session_id);
        assert_eq!(parsed_stream, stream_id);
    }

    #[test]
    fn parse_session_and_stream_id_invalid_session_short_circuits() {
        let raw_session = "bad-session".to_string();
        let err = parse_session_and_stream_id(raw_session.clone(), StreamId::new().to_string())
            .unwrap_err();
        match err {
            PjsError::InvalidSessionId(msg) => assert_eq!(msg, raw_session),
            other => panic!("expected InvalidSessionId, got {other:?}"),
        }
    }

    #[test]
    fn parse_session_and_stream_id_invalid_stream_returns_invalid_stream_id_error() {
        let raw_stream = "bad-stream".to_string();
        let err = parse_session_and_stream_id(SessionId::new().to_string(), raw_stream.clone())
            .unwrap_err();
        match err {
            PjsError::InvalidStreamId(msg) => assert_eq!(msg, raw_stream),
            other => panic!("expected InvalidStreamId, got {other:?}"),
        }
    }

    // --- existing integration tests ---

    use crate::domain::{
        entities::Stream,
        events::DomainEvent,
        ports::{
            EventPublisherGat, PriorityDistribution, StreamFilter, StreamStatistics, StreamStatus,
            StreamStoreGat,
        },
        value_objects::{SessionId, StreamId},
    };
    use crate::test_support::MockRepository;
    use chrono::Utc;

    struct MockEventPublisher;

    impl EventPublisherGat for MockEventPublisher {
        type PublishFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<()>> + Send + 'a
        where
            Self: 'a;

        type PublishBatchFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<()>> + Send + 'a
        where
            Self: 'a;

        fn publish(&self, _event: DomainEvent) -> Self::PublishFuture<'_> {
            async move { Ok(()) }
        }

        fn publish_batch(&self, _events: Vec<DomainEvent>) -> Self::PublishBatchFuture<'_> {
            async move { Ok(()) }
        }
    }

    struct MockStreamStore;

    impl StreamStoreGat for MockStreamStore {
        type StoreStreamFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<()>> + Send + 'a
        where
            Self: 'a;

        type GetStreamFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<Option<Stream>>>
            + Send
            + 'a
        where
            Self: 'a;

        type DeleteStreamFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<()>> + Send + 'a
        where
            Self: 'a;

        type ListStreamsForSessionFuture<'a>
            =
            impl std::future::Future<Output = crate::domain::DomainResult<Vec<Stream>>> + Send + 'a
        where
            Self: 'a;

        type FindStreamsBySessionFuture<'a>
            =
            impl std::future::Future<Output = crate::domain::DomainResult<Vec<Stream>>> + Send + 'a
        where
            Self: 'a;

        type UpdateStreamStatusFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<()>> + Send + 'a
        where
            Self: 'a;

        type GetStreamStatisticsFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<StreamStatistics>>
            + Send
            + 'a
        where
            Self: 'a;

        fn store_stream(&self, _stream: Stream) -> Self::StoreStreamFuture<'_> {
            async move { Ok(()) }
        }

        fn get_stream(&self, _stream_id: StreamId) -> Self::GetStreamFuture<'_> {
            async move { Ok(None) }
        }

        fn delete_stream(&self, _stream_id: StreamId) -> Self::DeleteStreamFuture<'_> {
            async move { Ok(()) }
        }

        fn list_streams_for_session(
            &self,
            _session_id: SessionId,
        ) -> Self::ListStreamsForSessionFuture<'_> {
            async move { Ok(vec![]) }
        }

        fn find_streams_by_session(
            &self,
            _session_id: SessionId,
            _filter: StreamFilter,
        ) -> Self::FindStreamsBySessionFuture<'_> {
            async move { Ok(vec![]) }
        }

        fn update_stream_status(
            &self,
            _stream_id: StreamId,
            _status: StreamStatus,
        ) -> Self::UpdateStreamStatusFuture<'_> {
            async move { Ok(()) }
        }

        fn get_stream_statistics(
            &self,
            _stream_id: StreamId,
        ) -> Self::GetStreamStatisticsFuture<'_> {
            async move {
                Ok(StreamStatistics {
                    total_frames: 0,
                    total_bytes: 0,
                    priority_distribution: PriorityDistribution::default(),
                    avg_frame_size: 0.0,
                    creation_time: Utc::now(),
                    completion_time: None,
                    processing_duration: None,
                })
            }
        }
    }

    #[tokio::test]
    async fn test_system_health() {
        let response = system_health().await;
        let health_data: serde_json::Value = response.0;

        assert_eq!(health_data["status"], "healthy");
        assert!(!health_data["features"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_app_state_creation() {
        let repository = Arc::new(MockRepository::new());
        let event_publisher = Arc::new(MockEventPublisher);
        let stream_store = Arc::new(MockStreamStore);

        let _state = PjsAppState::new(repository, event_publisher, stream_store);
    }

    #[tokio::test]
    async fn test_get_system_stats_returns_real_uptime() {
        use crate::application::handlers::QueryHandlerGat;
        use crate::application::handlers::query_handlers::SystemQueryHandler;
        use crate::application::queries::GetSystemStatsQuery;
        use std::time::{Duration, Instant};

        let repository = Arc::new(MockRepository::new());
        // Simulate a handler that started 5 seconds ago.
        let started_at = Instant::now() - Duration::from_secs(5);
        let handler = SystemQueryHandler::with_start_time(repository, started_at);

        let query = GetSystemStatsQuery {
            include_historical: false,
        };
        let result = QueryHandlerGat::handle(&handler, query).await.unwrap();

        // uptime must reflect the real elapsed time, not a hard-coded value.
        assert!(
            result.uptime_seconds >= 5,
            "uptime_seconds should be at least 5, got {}",
            result.uptime_seconds
        );
        // Must not be the old placeholder value (3600).
        assert_ne!(
            result.uptime_seconds, 3600,
            "uptime_seconds must not be the hard-coded placeholder 3600"
        );
    }

    #[cfg(feature = "metrics")]
    #[tokio::test]
    async fn test_metrics_endpoint_returns_prometheus_format() {
        use crate::infrastructure::http::metrics::install_global_recorder;

        // Install the recorder and verify the handle renders text/plain output.
        let handle = install_global_recorder().expect("recorder install should succeed");
        let rendered = handle.render();
        // Prometheus text format: empty registry produces an empty string or
        // comment lines; never a JSON error body.
        assert!(
            !rendered.contains("{\"error\""),
            "rendered metrics should not be a JSON error: {rendered}"
        );

        // Calling again must be idempotent.
        let handle2 = install_global_recorder().expect("second call must not fail");
        assert_eq!(
            handle.render(),
            handle2.render(),
            "both handles must render the same metrics"
        );
    }

    #[cfg(feature = "metrics")]
    #[test]
    fn test_metrics_router_has_metrics_route() {
        // Verify that the router includes /metrics by exercising the route builder.
        // We check this at compile time through the feature-gated code path.
        let _router =
            create_pjs_router_with_config::<MockRepository, MockEventPublisher, MockStreamStore>(
                &HttpServerConfig::default(),
            )
            .expect("router should build successfully with metrics feature");
    }

    /// Also guards the genuinely-absent `sort_by` path: omitting the query param
    /// entirely must still return `200` (i.e. `parse_sort_field` is never invoked, and
    /// `SearchSessionsQuery.sort_by` resolves to `None`) — a regression check against a
    /// future `unwrap_or_default`-style refactor of `search_sessions` breaking this case.
    /// See [`search_sessions_route_rejects_empty_sort_by`] for the present-but-empty case.
    #[tokio::test]
    async fn search_sessions_route_returns_ok() {
        use axum::http::Request;
        use tower::ServiceExt;

        let repository = Arc::new(MockRepository::new());
        let event_publisher = Arc::new(MockEventPublisher);
        let stream_store = Arc::new(MockStreamStore);
        let state = PjsAppState::new(repository, event_publisher, stream_store);

        let router =
            create_pjs_router_with_config::<MockRepository, MockEventPublisher, MockStreamStore>(
                &HttpServerConfig::default(),
            )
            .expect("router should build")
            .with_state(state);

        let req = Request::builder()
            .uri("/pjs/sessions/search")
            .body(axum::body::Body::empty())
            .unwrap();

        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    /// #414: an exact, correctly-cased `state` value is accepted end-to-end through the
    /// real router — the `Query<SearchSessionsParams>` extractor plus `parse_session_state`
    /// inside `search_sessions` must not reject a value matching `SessionState::as_str()`.
    #[tokio::test]
    async fn search_sessions_route_accepts_valid_state() {
        use axum::http::Request;
        use tower::ServiceExt;

        let repository = Arc::new(MockRepository::new());
        let event_publisher = Arc::new(MockEventPublisher);
        let stream_store = Arc::new(MockStreamStore);
        let state = PjsAppState::new(repository, event_publisher, stream_store);

        let router =
            create_pjs_router_with_config::<MockRepository, MockEventPublisher, MockStreamStore>(
                &HttpServerConfig::default(),
            )
            .expect("router should build")
            .with_state(state);

        let req = Request::builder()
            .uri("/pjs/sessions/search?state=Active")
            .body(axum::body::Body::empty())
            .unwrap();

        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    /// #414: an unrecognized `state` value must be rejected with a `400` carrying the
    /// API's standard `{"error": ...}` JSON envelope (via `PjsError::InvalidSessionState`),
    /// not axum's raw `Query`-extractor rejection body — see impl-critic gap S2. Also
    /// exercises the case-sensitivity break called out in the CHANGELOG: `"active"`
    /// (lowercase) previously matched via the repository's case-insensitive comparison
    /// and now must be rejected, since `SessionState` only deserializes its exact
    /// spellings (e.g. `"Active"`).
    #[tokio::test]
    async fn search_sessions_route_rejects_unknown_state() {
        use axum::body::to_bytes;
        use axum::http::Request;
        use tower::ServiceExt;

        let repository = Arc::new(MockRepository::new());
        let event_publisher = Arc::new(MockEventPublisher);
        let stream_store = Arc::new(MockStreamStore);
        let state = PjsAppState::new(repository, event_publisher, stream_store);

        let router =
            create_pjs_router_with_config::<MockRepository, MockEventPublisher, MockStreamStore>(
                &HttpServerConfig::default(),
            )
            .expect("router should build")
            .with_state(state);

        let req = Request::builder()
            .uri("/pjs/sessions/search?state=active")
            .body(axum::body::Body::empty())
            .unwrap();

        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let content_type = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert!(
            content_type.starts_with("application/json"),
            "rejection must use the API's JSON envelope, got content-type: {content_type}"
        );

        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(
            json.get("error").is_some_and(|e| e.is_string()),
            "body must match the standard {{\"error\": ...}} envelope, got: {json}"
        );
    }

    /// #492/#494: an exact, correctly-spelled `sort_by` value is accepted end-to-end
    /// through the real router — the `Query<SearchSessionsParams>` extractor plus
    /// `parse_sort_field` inside `search_sessions` must not reject a value matching
    /// one of `SessionSortField`'s serialized spellings.
    #[tokio::test]
    async fn search_sessions_route_accepts_valid_sort_by() {
        use axum::http::Request;
        use tower::ServiceExt;

        let repository = Arc::new(MockRepository::new());
        let event_publisher = Arc::new(MockEventPublisher);
        let stream_store = Arc::new(MockStreamStore);
        let state = PjsAppState::new(repository, event_publisher, stream_store);

        let router =
            create_pjs_router_with_config::<MockRepository, MockEventPublisher, MockStreamStore>(
                &HttpServerConfig::default(),
            )
            .expect("router should build")
            .with_state(state);

        let req = Request::builder()
            .uri("/pjs/sessions/search?sort_by=created_at")
            .body(axum::body::Body::empty())
            .unwrap();

        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    /// #492: an unrecognized `sort_by` value must be rejected with a `400` carrying
    /// the API's standard `{"error": ...}` JSON envelope (via
    /// `PjsError::InvalidSortField`), not silently ignored — see #492 for the prior
    /// behavior of silently skipping an unknown sort field.
    #[tokio::test]
    async fn search_sessions_route_rejects_unknown_sort_by() {
        use axum::body::to_bytes;
        use axum::http::Request;
        use tower::ServiceExt;

        let repository = Arc::new(MockRepository::new());
        let event_publisher = Arc::new(MockEventPublisher);
        let stream_store = Arc::new(MockStreamStore);
        let state = PjsAppState::new(repository, event_publisher, stream_store);

        let router =
            create_pjs_router_with_config::<MockRepository, MockEventPublisher, MockStreamStore>(
                &HttpServerConfig::default(),
            )
            .expect("router should build")
            .with_state(state);

        let req = Request::builder()
            .uri("/pjs/sessions/search?sort_by=bogus")
            .body(axum::body::Body::empty())
            .unwrap();

        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let content_type = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert!(
            content_type.starts_with("application/json"),
            "rejection must use the API's JSON envelope, got content-type: {content_type}"
        );

        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(
            json.get("error").is_some_and(|e| e.is_string()),
            "body must match the standard {{\"error\": ...}} envelope, got: {json}"
        );
    }

    /// #494: exercises the missing-underscore spelling called out in the issue —
    /// `createdat` never matched the old hand-rolled `match` either (it fell through
    /// to `_ => None` and was silently ignored, never accepted); now it is rejected
    /// with `400` since `SessionSortField` only deserializes its exact `snake_case`
    /// spellings (e.g. `created_at`).
    #[tokio::test]
    async fn search_sessions_route_rejects_sort_by_missing_underscore() {
        use axum::http::Request;
        use tower::ServiceExt;

        let repository = Arc::new(MockRepository::new());
        let event_publisher = Arc::new(MockEventPublisher);
        let stream_store = Arc::new(MockStreamStore);
        let state = PjsAppState::new(repository, event_publisher, stream_store);

        let router =
            create_pjs_router_with_config::<MockRepository, MockEventPublisher, MockStreamStore>(
                &HttpServerConfig::default(),
            )
            .expect("router should build")
            .with_state(state);

        let req = Request::builder()
            .uri("/pjs/sessions/search?sort_by=createdat")
            .body(axum::body::Body::empty())
            .unwrap();

        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    /// #492: `?sort_by=` (present but empty) is rejected with `400`, the same treatment
    /// [`parse_session_state`] gives an empty `?state=` — an empty value is not treated
    /// as "absent". See [`search_sessions_route_returns_ok`] for the genuinely-absent
    /// case (no `sort_by` param at all), which must still return `200`.
    #[tokio::test]
    async fn search_sessions_route_rejects_empty_sort_by() {
        use axum::http::Request;
        use tower::ServiceExt;

        let repository = Arc::new(MockRepository::new());
        let event_publisher = Arc::new(MockEventPublisher);
        let stream_store = Arc::new(MockStreamStore);
        let state = PjsAppState::new(repository, event_publisher, stream_store);

        let router =
            create_pjs_router_with_config::<MockRepository, MockEventPublisher, MockStreamStore>(
                &HttpServerConfig::default(),
            )
            .expect("router should build")
            .with_state(state);

        let req = Request::builder()
            .uri("/pjs/sessions/search?sort_by=")
            .body(axum::body::Body::empty())
            .unwrap();

        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    /// #497: each spelling `parse_sort_order` accepts — both the short (`asc`/`desc`)
    /// and long (`ascending`/`descending`) forms — is accepted end-to-end through the
    /// real router.
    #[tokio::test]
    async fn search_sessions_route_accepts_valid_sort_order() {
        use axum::http::Request;
        use tower::ServiceExt;

        let repository = Arc::new(MockRepository::new());
        let event_publisher = Arc::new(MockEventPublisher);
        let stream_store = Arc::new(MockStreamStore);
        let state = PjsAppState::new(repository, event_publisher, stream_store);

        let router =
            create_pjs_router_with_config::<MockRepository, MockEventPublisher, MockStreamStore>(
                &HttpServerConfig::default(),
            )
            .expect("router should build")
            .with_state(state);

        for value in ["asc", "ascending", "desc", "descending"] {
            let req = Request::builder()
                .uri(format!("/pjs/sessions/search?sort_order={value}"))
                .body(axum::body::Body::empty())
                .unwrap();

            let resp = router.clone().oneshot(req).await.unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::OK,
                "value {value} should be accepted"
            );
        }
    }

    /// #497: an unrecognized `sort_order` value must be rejected with a `400` carrying
    /// the API's standard `{"error": ...}` JSON envelope (via `PjsError::InvalidSortOrder`),
    /// mirroring `sort_by`'s treatment — previously an unrecognized value was silently
    /// ignored, falling back to the default sort order.
    #[tokio::test]
    async fn search_sessions_route_rejects_unknown_sort_order() {
        use axum::body::to_bytes;
        use axum::http::Request;
        use tower::ServiceExt;

        let repository = Arc::new(MockRepository::new());
        let event_publisher = Arc::new(MockEventPublisher);
        let stream_store = Arc::new(MockStreamStore);
        let state = PjsAppState::new(repository, event_publisher, stream_store);

        let router =
            create_pjs_router_with_config::<MockRepository, MockEventPublisher, MockStreamStore>(
                &HttpServerConfig::default(),
            )
            .expect("router should build")
            .with_state(state);

        let req = Request::builder()
            .uri("/pjs/sessions/search?sort_order=bogus")
            .body(axum::body::Body::empty())
            .unwrap();

        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let content_type = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert!(
            content_type.starts_with("application/json"),
            "rejection must use the API's JSON envelope, got content-type: {content_type}"
        );

        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(
            json.get("error").is_some_and(|e| e.is_string()),
            "body must match the standard {{\"error\": ...}} envelope, got: {json}"
        );
    }

    /// #497: `?sort_order=` (present but empty) is rejected with `400`, the same
    /// treatment [`parse_sort_field`] gives an empty `?sort_by=` — an empty value is not
    /// treated as "absent". See [`search_sessions_route_returns_ok`] for the
    /// genuinely-absent case (no `sort_order` param at all), which must still return `200`.
    #[tokio::test]
    async fn search_sessions_route_rejects_empty_sort_order() {
        use axum::http::Request;
        use tower::ServiceExt;

        let repository = Arc::new(MockRepository::new());
        let event_publisher = Arc::new(MockEventPublisher);
        let stream_store = Arc::new(MockStreamStore);
        let state = PjsAppState::new(repository, event_publisher, stream_store);

        let router =
            create_pjs_router_with_config::<MockRepository, MockEventPublisher, MockStreamStore>(
                &HttpServerConfig::default(),
            )
            .expect("router should build")
            .with_state(state);

        let req = Request::builder()
            .uri("/pjs/sessions/search?sort_order=")
            .body(axum::body::Body::empty())
            .unwrap();

        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    /// #497: exercises the issue's literal repro — `?sort_order=decs` (a typo for `desc`)
    /// previously fell through the old hand-rolled `match`'s `_ => None` arm and was
    /// silently ignored rather than rejected; now it returns `400`.
    #[tokio::test]
    async fn search_sessions_route_rejects_sort_order_typo() {
        use axum::http::Request;
        use tower::ServiceExt;

        let repository = Arc::new(MockRepository::new());
        let event_publisher = Arc::new(MockEventPublisher);
        let stream_store = Arc::new(MockStreamStore);
        let state = PjsAppState::new(repository, event_publisher, stream_store);

        let router =
            create_pjs_router_with_config::<MockRepository, MockEventPublisher, MockStreamStore>(
                &HttpServerConfig::default(),
            )
            .expect("router should build")
            .with_state(state);

        let req = Request::builder()
            .uri("/pjs/sessions/search?sort_order=decs")
            .body(axum::body::Body::empty())
            .unwrap();

        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    /// #497: `sort_order` matching is case-sensitive, like `sort_by` and `state` — an
    /// otherwise-valid spelling in the wrong case (`"ASC"`) is rejected with `400` rather
    /// than being accepted or silently ignored.
    #[tokio::test]
    async fn search_sessions_route_rejects_uppercase_sort_order() {
        use axum::http::Request;
        use tower::ServiceExt;

        let repository = Arc::new(MockRepository::new());
        let event_publisher = Arc::new(MockEventPublisher);
        let stream_store = Arc::new(MockStreamStore);
        let state = PjsAppState::new(repository, event_publisher, stream_store);

        let router =
            create_pjs_router_with_config::<MockRepository, MockEventPublisher, MockStreamStore>(
                &HttpServerConfig::default(),
            )
            .expect("router should build")
            .with_state(state);

        let req = Request::builder()
            .uri("/pjs/sessions/search?sort_order=ASC")
            .body(axum::body::Body::empty())
            .unwrap();

        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    /// End-to-end HTTP smoke test for the frame-generation route added in issue #230.
    ///
    /// Drives `create-session → create-stream → start-stream → generate-frames`
    /// over the real Axum router and asserts each step succeeds. After issue
    /// #232 implemented `Stream::extract_patches` and its patch-to-frame
    /// batching (now `Stream::chunk_patches_for_commit`), the route now
    /// produces frames for non-empty source data — the assertion
    /// `frame_count > 0` verifies the full chain end-to-end.
    #[tokio::test]
    async fn generate_frames_route_dispatches_command_end_to_end() {
        use axum::body::to_bytes;
        use axum::http::{Method, Request};
        use tower::ServiceExt;

        let repository = Arc::new(MockRepository::new());
        let event_publisher = Arc::new(MockEventPublisher);
        let stream_store = Arc::new(MockStreamStore);
        let state = PjsAppState::new(repository, event_publisher, stream_store);

        let router =
            create_pjs_router_with_config::<MockRepository, MockEventPublisher, MockStreamStore>(
                &HttpServerConfig::default(),
            )
            .expect("router should build")
            .with_state(state);

        let create_session = Request::builder()
            .method(Method::POST)
            .uri("/pjs/sessions")
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from("{}"))
            .unwrap();
        let resp = router.clone().oneshot(create_session).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let session: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let session_id = session["session_id"].as_str().unwrap().to_string();

        let create_stream = Request::builder()
            .method(Method::POST)
            .uri(format!("/pjs/sessions/{session_id}/streams"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(
                serde_json::json!({ "data": { "items": [1, 2, 3] } }).to_string(),
            ))
            .unwrap();
        let resp = router.clone().oneshot(create_stream).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let stream: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let stream_id = stream["stream_id"].as_str().unwrap().to_string();

        let start = Request::builder()
            .method(Method::POST)
            .uri(format!(
                "/pjs/sessions/{session_id}/streams/{stream_id}/start"
            ))
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = router.clone().oneshot(start).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let generate = Request::builder()
            .method(Method::POST)
            .uri(format!(
                "/pjs/sessions/{session_id}/streams/{stream_id}/generate-frames"
            ))
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(
                serde_json::json!({ "max_frames": 4 }).to_string(),
            ))
            .unwrap();
        let resp = router.oneshot(generate).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "POST .../generate-frames must be reachable end-to-end"
        );
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(payload["frames"].is_array(), "response must carry frames[]");
        let frame_count = payload["frame_count"]
            .as_u64()
            .expect("response must carry numeric frame_count");
        assert!(
            frame_count > 0,
            "extract_patches must yield at least one patch frame for `{{\"items\": [1,2,3]}}` \
             — frame_count was {frame_count}"
        );
    }

    /// End-to-end dictionary path: drive `generate-frames` enough times to
    /// cross the `N_TRAIN` threshold, then assert the dictionary endpoint
    /// transitions from `404 Not Found` to `200 OK`. This is the chain that
    /// issues #224, #230, and #232 together claim to deliver.
    #[cfg(all(feature = "compression", not(target_arch = "wasm32")))]
    #[tokio::test]
    async fn dictionary_endpoint_becomes_reachable_after_training() {
        use crate::compression::zstd::N_TRAIN;
        use crate::infrastructure::repositories::InMemoryDictionaryStore;
        use crate::security::CompressionBombDetector;
        use axum::body::to_bytes;
        use axum::http::{Method, Request};
        use tower::ServiceExt;

        let repository = Arc::new(MockRepository::new());
        let event_publisher = Arc::new(MockEventPublisher);
        let stream_store = Arc::new(MockStreamStore);
        let dictionary_store = Arc::new(InMemoryDictionaryStore::new(
            Arc::new(CompressionBombDetector::default()),
            64 * 1024,
        ));
        let state = PjsAppState::with_dictionary_store(
            repository,
            event_publisher,
            stream_store,
            dictionary_store,
        );

        let router =
            create_pjs_router_with_config::<MockRepository, MockEventPublisher, MockStreamStore>(
                &HttpServerConfig::default(),
            )
            .expect("router should build")
            .with_state(state);

        let create_session = Request::builder()
            .method(Method::POST)
            .uri("/pjs/sessions")
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from("{}"))
            .unwrap();
        let resp = router.clone().oneshot(create_session).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let session: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let session_id = session["session_id"].as_str().unwrap().to_string();

        // Source data with N_TRAIN+ leaf patches keeps the test self-contained:
        // a single generate-frames call yields enough samples to cross the
        // training threshold.
        let mut payload = serde_json::Map::new();
        for i in 0..(N_TRAIN + 4) {
            payload.insert(
                format!("field_{i}"),
                serde_json::Value::String(format!("value_{i}")),
            );
        }
        let create_stream = Request::builder()
            .method(Method::POST)
            .uri(format!("/pjs/sessions/{session_id}/streams"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(
                serde_json::json!({ "data": serde_json::Value::Object(payload) }).to_string(),
            ))
            .unwrap();
        let resp = router.clone().oneshot(create_stream).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let stream: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let stream_id = stream["stream_id"].as_str().unwrap().to_string();

        let start = Request::builder()
            .method(Method::POST)
            .uri(format!(
                "/pjs/sessions/{session_id}/streams/{stream_id}/start"
            ))
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = router.clone().oneshot(start).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Before training: the dictionary endpoint must be 404.
        let dict_before = Request::builder()
            .method(Method::GET)
            .uri(format!("/pjs/sessions/{session_id}/dictionary"))
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = router.clone().oneshot(dict_before).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "dictionary endpoint must be 404 before N_TRAIN samples accumulate"
        );

        // Generate enough frames to cross N_TRAIN. With max_frames at least
        // N_TRAIN+4, every leaf patch lands in its own frame.
        let max_frames = N_TRAIN + 4;
        let generate = Request::builder()
            .method(Method::POST)
            .uri(format!(
                "/pjs/sessions/{session_id}/streams/{stream_id}/generate-frames"
            ))
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(
                serde_json::json!({ "max_frames": max_frames }).to_string(),
            ))
            .unwrap();
        let resp = router.clone().oneshot(generate).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let frame_count = payload["frame_count"].as_u64().unwrap();
        assert!(
            frame_count >= N_TRAIN as u64,
            "single generate-frames call must yield at least N_TRAIN ({}) frames \
             so train_if_ready triggers training; got {frame_count}",
            N_TRAIN
        );

        // After training: the dictionary endpoint must be 200.
        let dict_after = Request::builder()
            .method(Method::GET)
            .uri(format!("/pjs/sessions/{session_id}/dictionary"))
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = router.oneshot(dict_after).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "dictionary endpoint must transition to 200 OK once N_TRAIN samples have been fed"
        );
        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        assert!(
            !body.is_empty(),
            "trained dictionary body must be non-empty"
        );
    }

    /// `priority_threshold = 0` is invalid per `Priority::new` — the route
    /// must reject the request with `400 Bad Request` rather than reaching
    /// the command handler.
    #[tokio::test]
    async fn generate_frames_route_rejects_invalid_priority() {
        use axum::http::{Method, Request};
        use tower::ServiceExt;

        let repository = Arc::new(MockRepository::new());
        let event_publisher = Arc::new(MockEventPublisher);
        let stream_store = Arc::new(MockStreamStore);
        let state = PjsAppState::new(repository, event_publisher, stream_store);

        let router =
            create_pjs_router_with_config::<MockRepository, MockEventPublisher, MockStreamStore>(
                &HttpServerConfig::default(),
            )
            .expect("router should build")
            .with_state(state);

        let sid = SessionId::new();
        let stream_id = StreamId::new();
        let req = Request::builder()
            .method(Method::POST)
            .uri(format!(
                "/pjs/sessions/{sid}/streams/{stream_id}/generate-frames"
            ))
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(
                serde_json::json!({ "priority_threshold": 0 }).to_string(),
            ))
            .unwrap();
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
