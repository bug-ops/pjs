//! Axum HTTP server adapter for PJS streaming

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path as AxumPath, Query, State},
    http::{
        HeaderValue, Method, StatusCode,
        header::{self, AUTHORIZATION, CONTENT_TYPE},
    },
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::{sync::Arc, time::Instant};
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    trace::TraceLayer,
};

use crate::{
    application::{
        commands::*,
        dto::PriorityDto,
        handlers::{
            CommandHandlerGat, QueryHandlerGat,
            command_handlers::SessionCommandHandler,
            query_handlers::{SessionQueryHandler, StreamQueryHandler, SystemQueryHandler},
        },
        queries::*,
    },
    domain::{
        aggregates::stream_session::{SessionConfig, SessionHealth},
        entities::Frame,
        ports::{
            DictionaryStore, EventPublisherGat, FrameStoreGat, NoopDictionaryStore,
            StreamRepositoryGat, StreamStoreGat,
        },
        value_objects::{Priority, SessionId, StreamId},
    },
    infrastructure::{
        adapters::InMemoryFrameStore,
        http::middleware::{RateLimitMiddleware, security_middleware},
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
    // We intentionally do NOT call .allow_credentials(true).
    // PJS does not use cookie-based auth; the Authorization header works without
    // credentials mode. allow_credentials(true) is incompatible with allow_origin(Any),
    // which would forbid the `["*"]` config path.
    let base = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([CONTENT_TYPE, AUTHORIZATION])
        .max_age(std::time::Duration::from_secs(3600));

    let has_wildcard = config.allowed_origins.iter().any(|o| o == "*");
    let has_explicit = config.allowed_origins.iter().any(|o| o != "*");

    let layer = match (
        config.allowed_origins.is_empty(),
        has_wildcard,
        has_explicit,
    ) {
        (true, _, _) => base.allow_origin(AllowOrigin::list(std::iter::empty::<HeaderValue>())),
        (_, true, true) => {
            return Err(PjsError::HttpError(
                "CORS: wildcard '*' cannot be combined with explicit origins".into(),
            ));
        }
        (_, true, false) => base.allow_origin(tower_http::cors::Any),
        (_, false, _) => {
            let origins: Vec<HeaderValue> = config
                .allowed_origins
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
/// The `dictionary_store` field is `pub(crate)` so the dictionary handler can
/// access it without exposing it as a public API.
pub struct PjsAppState<R, P, S, F = InMemoryFrameStore>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
    F: FrameStoreGat + Send + Sync + 'static,
{
    command_handler: Arc<SessionCommandHandler<R, P, F>>,
    session_query_handler: Arc<SessionQueryHandler<R>>,
    stream_query_handler: Arc<StreamQueryHandler<R, S, F>>,
    system_handler: Arc<SystemQueryHandler<R>>,
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
#[derive(Debug, Deserialize)]
pub struct CreateSessionRequest {
    pub max_concurrent_streams: Option<usize>,
    pub timeout_seconds: Option<u64>,
    pub client_info: Option<String>,
}

/// Response for session creation
#[derive(Debug, Serialize)]
pub struct CreateSessionResponse {
    pub session_id: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

/// Request to start streaming data
#[derive(Debug, Deserialize)]
pub struct StartStreamRequest {
    pub data: JsonValue,
    pub priority_threshold: Option<u8>,
    pub max_frames: Option<usize>,
}

/// Stream response parameters
#[derive(Debug, Deserialize)]
pub struct StreamParams {
    pub session_id: String,
    pub priority: Option<u8>,
    pub format: Option<String>,
}

/// Request body for generating priority-filtered frames on an existing stream.
///
/// Both fields are optional; defaults match the lowest-cost configuration that
/// still drives the priority pipeline:
/// - `priority_threshold` defaults to [`Priority::BACKGROUND`] (10) — accepts every frame.
/// - `max_frames` defaults to 16 — bounded so a single request cannot emit an
///   unbounded number of frames.
#[derive(Debug, Default, Deserialize)]
pub struct GenerateFramesRequest {
    pub priority_threshold: Option<u8>,
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
    pub frames: Vec<Frame>,
    pub frame_count: usize,
}

/// Session health response
#[derive(Debug, Serialize)]
pub struct SessionHealthResponse {
    pub is_healthy: bool,
    pub active_streams: usize,
    pub failed_streams: usize,
    pub is_expired: bool,
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
/// [`create_pjs_router_with_config`] with an explicit [`HttpServerConfig`].
///
/// TODO: Implement authentication strategy before production deployment.
/// Options: API keys, JWT tokens, OAuth2/OIDC
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
    apply_common_layers(all_routes, config)
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
/// Rate limiting is applied globally to all endpoints.
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
    let all_routes = public_routes::<R, P, S>()
        .merge(protected_routes::<R, P, S>())
        .layer(rate_limit_middleware);
    apply_common_layers(all_routes, config)
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
    apply_common_layers(merged, config)
}

/// Create PJS-enabled Axum router with both rate limiting and API key authentication.
///
/// Layer ordering (Tower applies layers outer-to-inner):
/// ```text
/// rate_limit  ← outermost: wraps both public and protected sub-routers
///   public_routes (no auth)
///   protected_routes
///     auth    ← inner: wraps only protected routes; unauthenticated requests are
///               rejected before consuming rate-limit quota for protected paths
///     handlers
/// ```
///
/// Rate limiting is applied to **both** the public and protected sub-routers (DoS
/// protection for `/pjs/health` is still desirable).
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
    // Auth runs before rate limit on the protected sub-router so that unauthenticated
    // traffic does not consume rate-limit quota and cannot starve legitimate clients.
    let protected = protected_routes::<R, P, S>().layer(auth);
    let merged = public_routes::<R, P, S>()
        .merge(protected)
        .layer(rate_limit);
    apply_common_layers(merged, config)
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
        .route("/pjs/sessions/search", get(search_sessions::<R, P, S>))
        .route("/pjs/sessions", get(list_sessions::<R, P, S>))
        .route("/pjs/stats", get(get_system_stats::<R, P, S>));

    #[cfg(all(feature = "compression", not(target_arch = "wasm32")))]
    let router = router.route(
        "/pjs/sessions/{session_id}/dictionary",
        get(crate::infrastructure::http::dictionary::get_session_dictionary::<R, P, S>),
    );

    router
}

/// Apply the cross-cutting middleware stack shared by all router variants.
///
/// Order (Tower applies outer-to-inner):
/// ```text
/// security_middleware   ← security headers
/// DefaultBodyLimit      ← body size guard
/// CorsLayer             ← CORS (outside auth, so preflight is answered before auth)
/// TraceLayer            ← distributed tracing
/// ```
fn apply_common_layers<R, P, S>(
    router: Router<PjsAppState<R, P, S>>,
    config: &HttpServerConfig,
) -> Result<Router<PjsAppState<R, P, S>>, PjsError>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    let cors = build_cors_layer(config)?;
    Ok(router
        .layer(middleware::from_fn(security_middleware))
        .layer(DefaultBodyLimit::max(10 * 1024 * 1024))
        .layer(cors)
        .layer(TraceLayer::new_for_http()))
}

/// Create a new streaming session
async fn create_session<R, P, S>(
    State(state): State<PjsAppState<R, P, S>>,
    headers: axum::http::HeaderMap,
    Json(request): Json<CreateSessionRequest>,
) -> Result<Json<CreateSessionResponse>, PjsError>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    let config = SessionConfig {
        max_concurrent_streams: request.max_concurrent_streams.unwrap_or(10),
        session_timeout_seconds: request.timeout_seconds.unwrap_or(3600),
        default_stream_config: Default::default(),
        enable_compression: true,
        metadata: Default::default(),
    };

    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|h| h.to_str().ok())
        .map(String::from);

    let command = CreateSessionCommand {
        config,
        client_info: request.client_info,
        user_agent,
        ip_address: None,
    };

    let session_id: SessionId = CommandHandlerGat::handle(&*state.command_handler, command)
        .await
        .map_err(PjsError::Application)?;

    let expires_at = chrono::Utc::now()
        + chrono::Duration::seconds(request.timeout_seconds.unwrap_or(3600) as i64);

    Ok(Json(CreateSessionResponse {
        session_id: session_id.to_string(),
        expires_at,
    }))
}

/// Get session information
async fn get_session<R, P, S>(
    State(state): State<PjsAppState<R, P, S>>,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<SessionResponse>, PjsError>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    let session_id =
        SessionId::from_string(&session_id).map_err(|_| PjsError::InvalidSessionId(session_id))?;

    let query = GetSessionQuery {
        session_id: session_id.into(),
    };

    let response = <SessionQueryHandler<R> as QueryHandlerGat<GetSessionQuery>>::handle(
        &*state.session_query_handler,
        query,
    )
    .await
    .map_err(PjsError::Application)?;

    Ok(Json(response))
}

/// Get session health status
async fn session_health<R, P, S>(
    State(state): State<PjsAppState<R, P, S>>,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<SessionHealthResponse>, PjsError>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    let session_id =
        SessionId::from_string(&session_id).map_err(|_| PjsError::InvalidSessionId(session_id))?;

    let query = GetSessionHealthQuery {
        session_id: session_id.into(),
    };

    let response = <SessionQueryHandler<R> as QueryHandlerGat<GetSessionHealthQuery>>::handle(
        &*state.session_query_handler,
        query,
    )
    .await
    .map_err(PjsError::Application)?;

    Ok(Json(SessionHealthResponse::from(response.health)))
}

/// Create a new stream within a session
///
/// TODO(CQ-007): Optimize double JSON processing
/// Current: serde_json::Value -> JsonDataDto -> JsonData
/// Optimization: Direct JsonData deserialization or use sonic-rs
async fn create_stream<R, P, S>(
    State(state): State<PjsAppState<R, P, S>>,
    AxumPath(session_id): AxumPath<String>,
    Json(request): Json<StartStreamRequest>,
) -> Result<Json<serde_json::Value>, PjsError>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    let session_id =
        SessionId::from_string(&session_id).map_err(|_| PjsError::InvalidSessionId(session_id))?;

    let command = CreateStreamCommand {
        session_id: session_id.into(),
        source_data: request.data,
        config: None,
    };

    let stream_id: StreamId = CommandHandlerGat::handle(&*state.command_handler, command)
        .await
        .map_err(PjsError::Application)?;

    Ok(Json(serde_json::json!({
        "stream_id": stream_id.to_string(),
        "status": "created"
    })))
}

/// Start streaming for a specific stream
async fn start_stream<R, P, S>(
    State(state): State<PjsAppState<R, P, S>>,
    AxumPath((session_id, stream_id)): AxumPath<(String, String)>,
) -> Result<Json<serde_json::Value>, PjsError>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    let session_id = SessionId::from_string(&session_id)
        .map_err(|_| PjsError::InvalidSessionId(session_id.clone()))?;
    let stream_id =
        StreamId::from_string(&stream_id).map_err(|_| PjsError::InvalidStreamId(stream_id))?;

    let command = StartStreamCommand {
        session_id: session_id.into(),
        stream_id: stream_id.into(),
    };

    <SessionCommandHandler<R, P> as CommandHandlerGat<StartStreamCommand>>::handle(
        &*state.command_handler,
        command,
    )
    .await
    .map_err(PjsError::Application)?;

    Ok(Json(serde_json::json!({
        "stream_id": stream_id.to_string(),
        "status": "started"
    })))
}

/// Generate priority-filtered frames for an existing stream.
///
/// Dispatches [`GenerateFramesCommand`] so the produced frames are fed into
/// the per-session dictionary-training corpus (see
/// [`SessionCommandHandler::with_dictionary_store`]). Without this route the
/// `GET /pjs/sessions/{id}/dictionary` endpoint stays at `404 Not Found` for
/// HTTP-only clients regardless of how many sessions and streams they create.
async fn generate_frames<R, P, S>(
    State(state): State<PjsAppState<R, P, S>>,
    AxumPath((session_id, stream_id)): AxumPath<(String, String)>,
    request: Option<Json<GenerateFramesRequest>>,
) -> Result<Json<GenerateFramesResponse>, PjsError>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    let session_id = SessionId::from_string(&session_id)
        .map_err(|_| PjsError::InvalidSessionId(session_id.clone()))?;
    let stream_id =
        StreamId::from_string(&stream_id).map_err(|_| PjsError::InvalidStreamId(stream_id))?;

    let Json(request) = request.unwrap_or_default();

    let priority_value = request
        .priority_threshold
        .unwrap_or(Priority::BACKGROUND.value());
    let priority_threshold =
        PriorityDto::new(priority_value).map_err(|e| PjsError::InvalidPriority(e.to_string()))?;
    let max_frames = request.max_frames.unwrap_or(16);

    let command = GenerateFramesCommand {
        session_id: session_id.into(),
        stream_id: stream_id.into(),
        priority_threshold,
        max_frames,
    };

    let frames: Vec<Frame> = <SessionCommandHandler<R, P> as CommandHandlerGat<
        GenerateFramesCommand,
    >>::handle(&*state.command_handler, command)
    .await
    .map_err(PjsError::Application)?;

    let frame_count = frames.len();
    Ok(Json(GenerateFramesResponse {
        frames,
        frame_count,
    }))
}

/// Get stream information
async fn get_stream<R, P, S>(
    State(state): State<PjsAppState<R, P, S>>,
    AxumPath((session_id, stream_id)): AxumPath<(String, String)>,
) -> Result<Json<StreamResponse>, PjsError>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    let session_id = SessionId::from_string(&session_id)
        .map_err(|_| PjsError::InvalidSessionId(session_id.clone()))?;
    let stream_id =
        StreamId::from_string(&stream_id).map_err(|_| PjsError::InvalidStreamId(stream_id))?;

    let query = GetStreamQuery {
        session_id: session_id.into(),
        stream_id: stream_id.into(),
    };

    let response = <StreamQueryHandler<R, S, InMemoryFrameStore> as QueryHandlerGat<
        GetStreamQuery,
    >>::handle(&*state.stream_query_handler, query)
    .await
    .map_err(PjsError::Application)?;

    Ok(Json(response))
}

/// List active sessions
async fn list_sessions<R, P, S>(
    State(state): State<PjsAppState<R, P, S>>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<SessionsResponse>, PjsError>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    let query = GetActiveSessionsQuery {
        limit: params.limit,
        offset: params.offset,
    };

    let response = <SessionQueryHandler<R> as QueryHandlerGat<GetActiveSessionsQuery>>::handle(
        &*state.session_query_handler,
        query,
    )
    .await
    .map_err(PjsError::Application)?;

    Ok(Json(response))
}

/// Search sessions with filters and sorting.
async fn search_sessions<R, P, S>(
    State(state): State<PjsAppState<R, P, S>>,
    Query(params): Query<SearchSessionsParams>,
) -> Result<Json<SessionsResponse>, PjsError>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    let sort_by = params.sort_by.as_deref().and_then(|s| match s {
        "created_at" => Some(SessionSortField::CreatedAt),
        "updated_at" => Some(SessionSortField::UpdatedAt),
        "stream_count" => Some(SessionSortField::StreamCount),
        "total_bytes" => Some(SessionSortField::TotalBytes),
        _ => None,
    });
    let sort_order = params.sort_order.as_deref().and_then(|s| match s {
        "ascending" | "asc" => Some(SortOrder::Ascending),
        "descending" | "desc" => Some(SortOrder::Descending),
        _ => None,
    });
    let query = SearchSessionsQuery {
        filters: SessionFilters {
            state: params.state,
            created_after: None,
            created_before: None,
            client_info: None,
            has_active_streams: None,
        },
        sort_by,
        sort_order,
        limit: params.limit,
        offset: params.offset,
    };
    let response = <SessionQueryHandler<R> as QueryHandlerGat<SearchSessionsQuery>>::handle(
        &*state.session_query_handler,
        query,
    )
    .await
    .map_err(PjsError::Application)?;
    Ok(Json(response))
}

/// Pagination parameters
#[derive(Debug, Deserialize)]
pub struct PaginationParams {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

/// Query parameters for session search endpoint.
#[derive(Debug, Deserialize)]
pub struct SearchSessionsParams {
    pub state: Option<String>,
    pub sort_by: Option<String>,
    pub sort_order: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

/// System health endpoint
async fn system_health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "healthy",
        "version": env!("CARGO_PKG_VERSION"),
        "features": ["pjs_streaming", "axum_integration", "gat_handlers"]
    }))
}

/// Real-time system statistics: uptime, session counts, frame throughput.
async fn get_system_stats<R, P, S>(
    State(state): State<PjsAppState<R, P, S>>,
) -> Result<Json<SystemStatsResponse>, PjsError>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    let query = GetSystemStatsQuery {
        include_historical: false,
    };

    let response = <SystemQueryHandler<R> as QueryHandlerGat<GetSystemStatsQuery>>::handle(
        &*state.system_handler,
        query,
    )
    .await
    .map_err(PjsError::Application)?;

    Ok(Json(response))
}

/// Query parameters for frame listing
#[derive(Debug, Deserialize)]
pub struct FrameQueryParams {
    pub since_sequence: Option<u64>,
    pub priority: Option<u8>,
    pub limit: Option<usize>,
}

/// Get frames for a stream (currently returns empty; no persistent frame store exists yet)
async fn get_stream_frames<R, P, S>(
    State(state): State<PjsAppState<R, P, S>>,
    AxumPath((session_id, stream_id)): AxumPath<(String, String)>,
    Query(params): Query<FrameQueryParams>,
) -> Result<Json<FramesResponse>, PjsError>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    let session_id = SessionId::from_string(&session_id)
        .map_err(|_| PjsError::InvalidSessionId(session_id.clone()))?;
    let stream_id =
        StreamId::from_string(&stream_id).map_err(|_| PjsError::InvalidStreamId(stream_id))?;

    let priority_filter = params
        .priority
        .map(|p| Priority::new(p).map(Into::into))
        .transpose()
        .map_err(|e: crate::domain::DomainError| PjsError::InvalidPriority(e.to_string()))?;

    let query = GetStreamFramesQuery {
        session_id: session_id.into(),
        stream_id: stream_id.into(),
        since_sequence: params.since_sequence,
        priority_filter,
        limit: params.limit,
    };

    let response = <StreamQueryHandler<R, S, InMemoryFrameStore> as QueryHandlerGat<
        GetStreamFramesQuery,
    >>::handle(&*state.stream_query_handler, query)
    .await
    .map_err(PjsError::Application)?;

    Ok(Json(response))
}

/// Get statistics for a session
async fn get_session_stats<R, P, S>(
    State(state): State<PjsAppState<R, P, S>>,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<SessionStatsResponse>, PjsError>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    let session_id =
        SessionId::from_string(&session_id).map_err(|_| PjsError::InvalidSessionId(session_id))?;

    let query = GetSessionStatsQuery {
        session_id: session_id.into(),
    };

    let response = <SessionQueryHandler<R> as QueryHandlerGat<GetSessionStatsQuery>>::handle(
        &*state.session_query_handler,
        query,
    )
    .await
    .map_err(PjsError::Application)?;

    Ok(Json(response))
}

// TODO(CQ-004): Implement HTTP rate limiting middleware
//
// Recommended implementation:
// - Add Arc<WebSocketRateLimiter> to PjsAppState
// - Use 100 requests/minute per IP with burst allowance
// - Extract IP from ConnectInfo<SocketAddr>
// - Return 429 Too Many Requests on limit exceeded
//
// Example:
// ```ignore
// async fn rate_limit_middleware(
//     State(limiter): State<Arc<WebSocketRateLimiter>>,
//     ConnectInfo(addr): ConnectInfo<SocketAddr>,
//     req: Request,
//     next: Next,
// ) -> Result<Response, StatusCode> {
//     limiter.check_request(addr.ip())
//         .map_err(|_| StatusCode::TOO_MANY_REQUESTS)?;
//     Ok(next.run(req).await)
// }
// ```
/// PJS-specific errors for HTTP endpoints
#[derive(Debug, thiserror::Error)]
pub enum PjsError {
    #[error("Application error: {0}")]
    Application(#[from] crate::application::ApplicationError),

    #[error("Invalid session ID: {0}")]
    InvalidSessionId(String),

    #[error("Invalid stream ID: {0}")]
    InvalidStreamId(String),

    #[error("Invalid priority: {0}")]
    InvalidPriority(String),

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

    // --- existing integration tests ---

    use crate::domain::{
        aggregates::StreamSession,
        entities::Stream,
        events::DomainEvent,
        ports::{
            EventPublisherGat, Pagination, PriorityDistribution, SessionHealthSnapshot,
            SessionQueryCriteria, SessionQueryResult, StreamFilter, StreamRepositoryGat,
            StreamStatistics, StreamStatus, StreamStoreGat,
        },
        value_objects::{SessionId, StreamId},
    };
    use chrono::Utc;
    use std::collections::HashMap;

    struct MockRepository {
        sessions: parking_lot::Mutex<HashMap<SessionId, StreamSession>>,
    }

    impl MockRepository {
        fn new() -> Self {
            Self {
                sessions: parking_lot::Mutex::new(HashMap::new()),
            }
        }
    }

    impl StreamRepositoryGat for MockRepository {
        type FindSessionFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<Option<StreamSession>>>
            + Send
            + 'a
        where
            Self: 'a;

        type SaveSessionFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<()>> + Send + 'a
        where
            Self: 'a;

        type RemoveSessionFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<()>> + Send + 'a
        where
            Self: 'a;

        type FindActiveSessionsFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<Vec<StreamSession>>>
            + Send
            + 'a
        where
            Self: 'a;

        type FindSessionsByCriteriaFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<SessionQueryResult>>
            + Send
            + 'a
        where
            Self: 'a;

        type GetSessionHealthFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<SessionHealthSnapshot>>
            + Send
            + 'a
        where
            Self: 'a;

        type SessionExistsFuture<'a>
            = impl std::future::Future<Output = crate::domain::DomainResult<bool>> + Send + 'a
        where
            Self: 'a;

        fn find_session(&self, session_id: SessionId) -> Self::FindSessionFuture<'_> {
            async move { Ok(self.sessions.lock().get(&session_id).cloned()) }
        }

        fn save_session(&self, session: StreamSession) -> Self::SaveSessionFuture<'_> {
            async move {
                self.sessions.lock().insert(session.id(), session);
                Ok(())
            }
        }

        fn remove_session(&self, session_id: SessionId) -> Self::RemoveSessionFuture<'_> {
            async move {
                self.sessions.lock().remove(&session_id);
                Ok(())
            }
        }

        fn find_active_sessions(&self) -> Self::FindActiveSessionsFuture<'_> {
            async move { Ok(self.sessions.lock().values().cloned().collect()) }
        }

        fn find_sessions_by_criteria(
            &self,
            _criteria: SessionQueryCriteria,
            pagination: Pagination,
        ) -> Self::FindSessionsByCriteriaFuture<'_> {
            async move {
                let sessions: Vec<_> = self.sessions.lock().values().cloned().collect();
                let total_count = sessions.len();
                let paginated: Vec<_> = sessions
                    .into_iter()
                    .skip(pagination.offset)
                    .take(pagination.limit)
                    .collect();
                let has_more = pagination.offset + paginated.len() < total_count;
                Ok(SessionQueryResult {
                    sessions: paginated,
                    total_count,
                    has_more,
                    query_duration_ms: 0,
                    scan_limit_reached: false,
                })
            }
        }

        fn get_session_health(&self, session_id: SessionId) -> Self::GetSessionHealthFuture<'_> {
            async move {
                Ok(SessionHealthSnapshot {
                    session_id,
                    is_healthy: true,
                    active_streams: 0,
                    total_frames: 0,
                    last_activity: Utc::now(),
                    error_rate: 0.0,
                    metrics: HashMap::new(),
                })
            }
        }

        fn session_exists(&self, session_id: SessionId) -> Self::SessionExistsFuture<'_> {
            async move { Ok(self.sessions.lock().contains_key(&session_id)) }
        }
    }

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

    /// End-to-end HTTP smoke test for the frame-generation route added in issue #230.
    ///
    /// Drives `create-session → create-stream → start-stream → generate-frames`
    /// over the real Axum router and asserts each step succeeds. After issue
    /// #232 implemented `Stream::extract_patches` and `batch_patches_into_frames`,
    /// the route now produces frames for non-empty source data — the assertion
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
