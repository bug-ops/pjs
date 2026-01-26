//! Axum HTTP server adapter for PJS streaming

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path as AxumPath, Query, State},
    http::{
        Method, StatusCode,
        header::{self, AUTHORIZATION, CONTENT_TYPE},
    },
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::sync::Arc;
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use crate::{
    application::{
        commands::*,
        handlers::{
            CommandHandlerGat, QueryHandlerGat,
            command_handlers::SessionCommandHandler,
            query_handlers::{SessionQueryHandler, StreamQueryHandler},
        },
        queries::*,
    },
    domain::{
        aggregates::stream_session::{SessionConfig, SessionHealth},
        ports::{EventPublisherGat, StreamRepositoryGat, StreamStoreGat},
        value_objects::{SessionId, StreamId},
    },
    infrastructure::http::middleware::{RateLimitMiddleware, security_middleware},
};

/// Axum application state with PJS GAT-based handlers
pub struct PjsAppState<R, P, S>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    command_handler: Arc<SessionCommandHandler<R, P>>,
    session_query_handler: Arc<SessionQueryHandler<R>>,
    stream_query_handler: Arc<StreamQueryHandler<R, S>>,
}

impl<R, P, S> Clone for PjsAppState<R, P, S>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    fn clone(&self) -> Self {
        Self {
            command_handler: self.command_handler.clone(),
            session_query_handler: self.session_query_handler.clone(),
            stream_query_handler: self.stream_query_handler.clone(),
        }
    }
}

impl<R, P, S> PjsAppState<R, P, S>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    pub fn new(repository: Arc<R>, event_publisher: Arc<P>, stream_store: Arc<S>) -> Self {
        Self {
            command_handler: Arc::new(SessionCommandHandler::new(
                repository.clone(),
                event_publisher,
            )),
            session_query_handler: Arc::new(SessionQueryHandler::new(repository.clone())),
            stream_query_handler: Arc::new(StreamQueryHandler::new(repository, stream_store)),
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

/// Create PJS-enabled Axum router
///
/// # Security Note
/// TODO: Implement authentication strategy before production deployment.
/// Options: API keys, JWT tokens, OAuth2/OIDC
/// Current implementation is a public API - requires strict rate limiting
pub fn create_pjs_router<R, P, S>() -> Router<PjsAppState<R, P, S>>
where
    R: StreamRepositoryGat + Send + Sync + 'static,
    P: EventPublisherGat + Send + Sync + 'static,
    S: StreamStoreGat + Send + Sync + 'static,
{
    Router::new()
        .route("/pjs/sessions", post(create_session::<R, P, S>))
        .route("/pjs/sessions/{session_id}", get(get_session::<R, P, S>))
        .route(
            "/pjs/sessions/{session_id}/health",
            get(session_health::<R, P, S>),
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
            "/pjs/sessions/{session_id}/streams/{stream_id}",
            get(get_stream::<R, P, S>),
        )
        .route("/pjs/sessions", get(list_sessions::<R, P, S>))
        .route("/pjs/health", get(system_health))
        .layer(middleware::from_fn(security_middleware))
        .layer(DefaultBodyLimit::max(10 * 1024 * 1024))
        .layer(
            CorsLayer::new()
                .allow_origin(["http://localhost:3000"
                    .parse::<axum::http::HeaderValue>()
                    .unwrap()])
                .allow_methods([Method::GET, Method::POST])
                .allow_headers([CONTENT_TYPE, AUTHORIZATION])
                .max_age(std::time::Duration::from_secs(3600)),
        )
        .layer(TraceLayer::new_for_http())
}

/// Create PJS-enabled Axum router with rate limiting
///
/// Adds rate limiting middleware to protect against DoS attacks.
/// Default: 100 requests per minute per IP address.
///
/// # Security Note
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
    Router::new()
        .route("/pjs/sessions", post(create_session::<R, P, S>))
        .route("/pjs/sessions/{session_id}", get(get_session::<R, P, S>))
        .route(
            "/pjs/sessions/{session_id}/health",
            get(session_health::<R, P, S>),
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
            "/pjs/sessions/{session_id}/streams/{stream_id}",
            get(get_stream::<R, P, S>),
        )
        .route("/pjs/sessions", get(list_sessions::<R, P, S>))
        .route("/pjs/health", get(system_health))
        .layer(rate_limit_middleware)
        .layer(middleware::from_fn(security_middleware))
        .layer(DefaultBodyLimit::max(10 * 1024 * 1024))
        .layer(
            CorsLayer::new()
                .allow_origin(["http://localhost:3000"
                    .parse::<axum::http::HeaderValue>()
                    .unwrap()])
                .allow_methods([Method::GET, Method::POST])
                .allow_headers([CONTENT_TYPE, AUTHORIZATION])
                .max_age(std::time::Duration::from_secs(3600)),
        )
        .layer(TraceLayer::new_for_http())
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

    let response = <StreamQueryHandler<R, S> as QueryHandlerGat<GetStreamQuery>>::handle(
        &*state.stream_query_handler,
        query,
    )
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

/// Pagination parameters
#[derive(Debug, Deserialize)]
pub struct PaginationParams {
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

/// TODO(CQ-004): Implement HTTP rate limiting middleware
///
/// Recommended implementation:
/// - Add Arc<WebSocketRateLimiter> to PjsAppState
/// - Use 100 requests/minute per IP with burst allowance
/// - Extract IP from ConnectInfo<SocketAddr>
/// - Return 429 Too Many Requests on limit exceeded
///
/// Example:
/// ```ignore
/// async fn rate_limit_middleware(
///     State(limiter): State<Arc<WebSocketRateLimiter>>,
///     ConnectInfo(addr): ConnectInfo<SocketAddr>,
///     req: Request,
///     next: Next,
/// ) -> Result<Response, StatusCode> {
///     limiter.check_request(addr.ip())
///         .map_err(|_| StatusCode::TOO_MANY_REQUESTS)?;
///     Ok(next.run(req).await)
/// }
/// ```
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
            PjsError::Application(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
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
}
