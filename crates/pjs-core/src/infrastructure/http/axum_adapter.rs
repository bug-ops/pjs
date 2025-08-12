//! Axum HTTP server adapter for PJS streaming

use axum::{
    extract::{Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{Response, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::{
    collections::HashMap,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use crate::{
    application::{
        services::{SessionService, StreamingService},
        handlers::{CommandHandler, QueryHandler},
        commands::*,
        queries::*,
    },
    domain::{
        value_objects::{SessionId, StreamId, Priority},
        aggregates::stream_session::SessionConfig,
        entities::Frame,
        services::connection_manager::ConnectionManager,
    },
};

/// Axum application state with PJS services
#[derive(Clone)]
pub struct PjsAppState<CH, QH>
where
    CH: CommandHandler<CreateSessionCommand, SessionId> + 
         CommandHandler<CreateStreamCommand, StreamId> +
         CommandHandler<StartStreamCommand, ()> +
         CommandHandler<CompleteStreamCommand, ()> +
         CommandHandler<CloseSessionCommand, ()> +
         CommandHandler<GenerateFramesCommand, Vec<Frame>> +
         CommandHandler<BatchGenerateFramesCommand, Vec<Frame>> +
         CommandHandler<AdjustPriorityThresholdCommand, ()> +
         Clone + Send + Sync + 'static,
    QH: QueryHandler<GetSessionQuery, SessionResponse> +
         QueryHandler<GetSessionHealthQuery, HealthResponse> +
         QueryHandler<GetActiveSessionsQuery, SessionsResponse> +
         Clone + Send + Sync + 'static,
{
    session_service: Arc<SessionService<CH, QH>>,
    streaming_service: Arc<StreamingService<CH>>,
    connection_manager: Arc<ConnectionManager>,
}

impl<CH, QH> PjsAppState<CH, QH>
where
    CH: CommandHandler<CreateSessionCommand, SessionId> + 
         CommandHandler<CreateStreamCommand, StreamId> +
         CommandHandler<StartStreamCommand, ()> +
         CommandHandler<CompleteStreamCommand, ()> +
         CommandHandler<CloseSessionCommand, ()> +
         CommandHandler<GenerateFramesCommand, Vec<Frame>> +
         CommandHandler<BatchGenerateFramesCommand, Vec<Frame>> +
         CommandHandler<AdjustPriorityThresholdCommand, ()> +
         Clone + Send + Sync + 'static,
    QH: QueryHandler<GetSessionQuery, SessionResponse> +
         QueryHandler<GetSessionHealthQuery, HealthResponse> +
         QueryHandler<GetActiveSessionsQuery, SessionsResponse> +
         Clone + Send + Sync + 'static,
{
    pub fn new(
        session_service: Arc<SessionService<CH, QH>>,
        streaming_service: Arc<StreamingService<CH>>,
        connection_manager: Arc<ConnectionManager>,
    ) -> Self {
        Self {
            session_service,
            streaming_service,
            connection_manager,
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
    pub format: Option<String>, // json, ndjson, sse
}

/// Create PJS-enabled Axum router
pub fn create_pjs_router<CH, QH>() -> Router<PjsAppState<CH, QH>>
where
    CH: CommandHandler<CreateSessionCommand, SessionId> + 
         CommandHandler<CreateStreamCommand, StreamId> +
         CommandHandler<StartStreamCommand, ()> +
         CommandHandler<CompleteStreamCommand, ()> +
         CommandHandler<CloseSessionCommand, ()> +
         CommandHandler<GenerateFramesCommand, Vec<Frame>> +
         CommandHandler<BatchGenerateFramesCommand, Vec<Frame>> +
         CommandHandler<AdjustPriorityThresholdCommand, ()> +
         Clone + Send + Sync + 'static,
    QH: QueryHandler<GetSessionQuery, SessionResponse> +
         QueryHandler<GetSessionHealthQuery, HealthResponse> +
         QueryHandler<GetActiveSessionsQuery, SessionsResponse> +
         Clone + Send + Sync + 'static,
{
    Router::new()
        // Session management
        .route("/pjs/sessions", post(create_session::<CH, QH>))
        .route("/pjs/sessions/{session_id}", get(get_session::<CH, QH>))
        .route("/pjs/sessions/{session_id}/health", get(session_health::<CH, QH>))
        
        // Streaming endpoints
        .route("/pjs/stream/{session_id}", post(start_stream::<CH, QH>))
        .route("/pjs/stream/{session_id}/frames", get(stream_frames::<CH, QH>))
        .route("/pjs/stream/{session_id}/sse", get(stream_server_sent_events::<CH, QH>))
        
        // System endpoints
        .route("/pjs/health", get(system_health))
        .route("/pjs/connections", get(connection_stats::<CH, QH>))
        
        // Middleware
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
}

/// Create a new streaming session
async fn create_session<CH, QH>(
    State(state): State<PjsAppState<CH, QH>>,
    headers: HeaderMap,
    Json(request): Json<CreateSessionRequest>,
) -> Result<Json<CreateSessionResponse>, PjsError>
where
    CH: CommandHandler<CreateSessionCommand, SessionId> + 
         CommandHandler<CreateStreamCommand, StreamId> +
         CommandHandler<StartStreamCommand, ()> +
         CommandHandler<CompleteStreamCommand, ()> +
         CommandHandler<CloseSessionCommand, ()> +
         CommandHandler<GenerateFramesCommand, Vec<Frame>> +
         CommandHandler<BatchGenerateFramesCommand, Vec<Frame>> +
         CommandHandler<AdjustPriorityThresholdCommand, ()> +
         Clone + Send + Sync + 'static,
    QH: QueryHandler<GetSessionQuery, SessionResponse> +
         QueryHandler<GetSessionHealthQuery, HealthResponse> +
         QueryHandler<GetActiveSessionsQuery, SessionsResponse> +
         Clone + Send + Sync + 'static,
{
    let config = SessionConfig {
        max_concurrent_streams: request.max_concurrent_streams.unwrap_or(10),
        session_timeout_seconds: request.timeout_seconds.unwrap_or(3600),
        default_stream_config: Default::default(),
        enable_compression: true,
        metadata: HashMap::new(),
    };
    
    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|h| h.to_str().ok())
        .map(String::from);
    
    let session_id = state.session_service
        .create_and_activate_session(
            config,
            request.client_info,
            user_agent,
            None, // IP address would be extracted from connection
        )
        .await
        .map_err(PjsError::Application)?;
    
    // Register connection with connection manager
    if let Err(e) = state.connection_manager.register_connection(session_id.clone()).await {
        tracing::warn!("Failed to register connection: {}", e);
    }
    
    // Calculate expiration time
    let expires_at = chrono::Utc::now() + chrono::Duration::hours(1);
    
    Ok(Json(CreateSessionResponse {
        session_id: session_id.to_string(),
        expires_at,
    }))
}

/// Get session information
async fn get_session<CH, QH>(
    State(state): State<PjsAppState<CH, QH>>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
) -> Result<Json<SessionResponse>, PjsError>
where
    CH: CommandHandler<CreateSessionCommand, SessionId> + 
         CommandHandler<CreateStreamCommand, StreamId> +
         CommandHandler<StartStreamCommand, ()> +
         CommandHandler<CompleteStreamCommand, ()> +
         CommandHandler<CloseSessionCommand, ()> +
         CommandHandler<GenerateFramesCommand, Vec<Frame>> +
         CommandHandler<BatchGenerateFramesCommand, Vec<Frame>> +
         CommandHandler<AdjustPriorityThresholdCommand, ()> +
         Clone + Send + Sync + 'static,
    QH: QueryHandler<GetSessionQuery, SessionResponse> +
         QueryHandler<GetSessionHealthQuery, HealthResponse> +
         QueryHandler<GetActiveSessionsQuery, SessionsResponse> +
         Clone + Send + Sync + 'static,
{
    let session_id = SessionId::from_string(&session_id)
        .map_err(|_| PjsError::InvalidSessionId(session_id))?;
    
    let session_with_health = state.session_service
        .get_session_with_health(session_id)
        .await
        .map_err(PjsError::Application)?;
    
    Ok(Json(SessionResponse {
        session: session_with_health.session,
    }))
}

/// Get session health status
async fn session_health<CH, QH>(
    State(state): State<PjsAppState<CH, QH>>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
) -> Result<Json<HealthResponse>, PjsError>
where
    CH: CommandHandler<CreateSessionCommand, SessionId> + 
         CommandHandler<CreateStreamCommand, StreamId> +
         CommandHandler<StartStreamCommand, ()> +
         CommandHandler<CompleteStreamCommand, ()> +
         CommandHandler<CloseSessionCommand, ()> +
         CommandHandler<GenerateFramesCommand, Vec<Frame>> +
         CommandHandler<BatchGenerateFramesCommand, Vec<Frame>> +
         CommandHandler<AdjustPriorityThresholdCommand, ()> +
         Clone + Send + Sync + 'static,
    QH: QueryHandler<GetSessionQuery, SessionResponse> +
         QueryHandler<GetSessionHealthQuery, HealthResponse> +
         QueryHandler<GetActiveSessionsQuery, SessionsResponse> +
         Clone + Send + Sync + 'static,
{
    let session_id = SessionId::from_string(&session_id)
        .map_err(|_| PjsError::InvalidSessionId(session_id))?;
    
    let session_with_health = state.session_service
        .get_session_with_health(session_id)
        .await
        .map_err(PjsError::Application)?;
    
    Ok(Json(HealthResponse {
        health: session_with_health.health,
    }))
}

/// Start streaming data in a session
async fn start_stream<CH, QH>(
    State(state): State<PjsAppState<CH, QH>>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
    Json(request): Json<StartStreamRequest>,
) -> Result<Json<serde_json::Value>, PjsError>
where
    CH: CommandHandler<CreateSessionCommand, SessionId> + 
         CommandHandler<CreateStreamCommand, StreamId> +
         CommandHandler<StartStreamCommand, ()> +
         CommandHandler<CompleteStreamCommand, ()> +
         CommandHandler<CloseSessionCommand, ()> +
         CommandHandler<GenerateFramesCommand, Vec<Frame>> +
         CommandHandler<BatchGenerateFramesCommand, Vec<Frame>> +
         CommandHandler<AdjustPriorityThresholdCommand, ()> +
         Clone + Send + Sync + 'static,
    QH: QueryHandler<GetSessionQuery, SessionResponse> +
         QueryHandler<GetSessionHealthQuery, HealthResponse> +
         QueryHandler<GetActiveSessionsQuery, SessionsResponse> +
         Clone + Send + Sync + 'static,
{
    let session_id = SessionId::from_string(&session_id)
        .map_err(|_| PjsError::InvalidSessionId(session_id))?;
    
    // Update connection activity
    if let Err(e) = state.connection_manager.update_activity(&session_id).await {
        tracing::warn!("Failed to update connection activity: {}", e);
    }
    
    let stream_id = state.session_service
        .create_and_start_stream(session_id.clone(), request.data, None)
        .await
        .map_err(PjsError::Application)?;
    
    // Associate stream with connection
    if let Err(e) = state.connection_manager.set_stream(&session_id, stream_id.clone()).await {
        tracing::warn!("Failed to associate stream with connection: {}", e);
    }
    
    Ok(Json(serde_json::json!({
        "stream_id": stream_id.to_string(),
        "status": "started"
    })))
}

/// Stream frames as JSON responses
async fn stream_frames<CH, QH>(
    State(state): State<PjsAppState<CH, QH>>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
    Query(params): Query<StreamParams>,
) -> Result<Response, PjsError>
where
    CH: CommandHandler<CreateSessionCommand, SessionId> + 
         CommandHandler<CreateStreamCommand, StreamId> +
         CommandHandler<StartStreamCommand, ()> +
         CommandHandler<CompleteStreamCommand, ()> +
         CommandHandler<CloseSessionCommand, ()> +
         CommandHandler<GenerateFramesCommand, Vec<Frame>> +
         CommandHandler<BatchGenerateFramesCommand, Vec<Frame>> +
         CommandHandler<AdjustPriorityThresholdCommand, ()> +
         Clone + Send + Sync + 'static,
    QH: QueryHandler<GetSessionQuery, SessionResponse> +
         QueryHandler<GetSessionHealthQuery, HealthResponse> +
         QueryHandler<GetActiveSessionsQuery, SessionsResponse> +
         Clone + Send + Sync + 'static,
{
    let session_id = SessionId::from_string(&session_id)
        .map_err(|_| PjsError::InvalidSessionId(session_id.clone()))?;
    
    let priority_threshold = params.priority
        .map(Priority::new)
        .transpose()
        .map_err(|e| PjsError::InvalidPriority(e.to_string()))?
        .unwrap_or(Priority::MEDIUM);
    
    // Create streaming response
    let stream = FrameStream::new(state.session_service.clone(), session_id, priority_threshold);
    
    match params.format.as_deref() {
        Some("sse") => create_sse_response(stream),
        Some("ndjson") => create_ndjson_response(stream),
        _ => create_json_response(stream),
    }
}

/// Stream frames as Server-Sent Events
async fn stream_server_sent_events<CH, QH>(
    State(state): State<PjsAppState<CH, QH>>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
    Query(params): Query<StreamParams>,
) -> Result<Response, PjsError>
where
    CH: CommandHandler<CreateSessionCommand, SessionId> + 
         CommandHandler<CreateStreamCommand, StreamId> +
         CommandHandler<StartStreamCommand, ()> +
         CommandHandler<CompleteStreamCommand, ()> +
         CommandHandler<CloseSessionCommand, ()> +
         CommandHandler<GenerateFramesCommand, Vec<Frame>> +
         CommandHandler<BatchGenerateFramesCommand, Vec<Frame>> +
         CommandHandler<AdjustPriorityThresholdCommand, ()> +
         Clone + Send + Sync + 'static,
    QH: QueryHandler<GetSessionQuery, SessionResponse> +
         QueryHandler<GetSessionHealthQuery, HealthResponse> +
         QueryHandler<GetActiveSessionsQuery, SessionsResponse> +
         Clone + Send + Sync + 'static,
{
    let session_id = SessionId::from_string(&session_id)
        .map_err(|_| PjsError::InvalidSessionId(session_id.clone()))?;
    
    let priority_threshold = params.priority
        .map(Priority::new)
        .transpose()
        .map_err(|e| PjsError::InvalidPriority(e.to_string()))?
        .unwrap_or(Priority::MEDIUM);
    
    let stream = FrameStream::new(state.session_service.clone(), session_id, priority_threshold);
    create_sse_response(stream)
}

/// System health endpoint
async fn system_health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "healthy",
        "version": env!("CARGO_PKG_VERSION"),
        "features": ["pjs_streaming", "axum_integration"]
    }))
}

/// Frame streaming implementation
pub struct FrameStream<CH, QH> 
where
    CH: CommandHandler<CreateSessionCommand, SessionId> + 
         CommandHandler<CreateStreamCommand, StreamId> +
         CommandHandler<StartStreamCommand, ()> +
         CommandHandler<CompleteStreamCommand, ()> +
         CommandHandler<CloseSessionCommand, ()> +
         CommandHandler<GenerateFramesCommand, Vec<Frame>> +
         CommandHandler<BatchGenerateFramesCommand, Vec<Frame>> +
         CommandHandler<AdjustPriorityThresholdCommand, ()> +
         Clone + Send + Sync + 'static,
    QH: QueryHandler<GetSessionQuery, SessionResponse> +
         QueryHandler<GetSessionHealthQuery, HealthResponse> +
         QueryHandler<GetActiveSessionsQuery, SessionsResponse> +
         Clone + Send + Sync + 'static,
{
    session_service: Arc<SessionService<CH, QH>>,
    session_id: SessionId,
    priority_threshold: Priority,
    current_frame: usize,
}

impl<CH, QH> FrameStream<CH, QH> 
where
    CH: CommandHandler<CreateSessionCommand, SessionId> + 
         CommandHandler<CreateStreamCommand, StreamId> +
         CommandHandler<StartStreamCommand, ()> +
         CommandHandler<CompleteStreamCommand, ()> +
         CommandHandler<CloseSessionCommand, ()> +
         CommandHandler<GenerateFramesCommand, Vec<Frame>> +
         CommandHandler<BatchGenerateFramesCommand, Vec<Frame>> +
         CommandHandler<AdjustPriorityThresholdCommand, ()> +
         Clone + Send + Sync + 'static,
    QH: QueryHandler<GetSessionQuery, SessionResponse> +
         QueryHandler<GetSessionHealthQuery, HealthResponse> +
         QueryHandler<GetActiveSessionsQuery, SessionsResponse> +
         Clone + Send + Sync + 'static,
{
    pub fn new(
        session_service: Arc<SessionService<CH, QH>>,
        session_id: SessionId,
        priority_threshold: Priority,
    ) -> Self {
        Self {
            session_service,
            session_id,
            priority_threshold,
            current_frame: 0,
        }
    }
}

impl<CH, QH> Stream for FrameStream<CH, QH>
where
    CH: CommandHandler<CreateSessionCommand, SessionId> + 
         CommandHandler<CreateStreamCommand, StreamId> +
         CommandHandler<StartStreamCommand, ()> +
         CommandHandler<CompleteStreamCommand, ()> +
         CommandHandler<CloseSessionCommand, ()> +
         CommandHandler<GenerateFramesCommand, Vec<Frame>> +
         CommandHandler<BatchGenerateFramesCommand, Vec<Frame>> +
         CommandHandler<AdjustPriorityThresholdCommand, ()> +
         Clone + Send + Sync + 'static,
    QH: QueryHandler<GetSessionQuery, SessionResponse> +
         QueryHandler<GetSessionHealthQuery, HealthResponse> +
         QueryHandler<GetActiveSessionsQuery, SessionsResponse> +
         Clone + Send + Sync + 'static,
{
    type Item = Result<String, PjsError>;

    fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Simplified implementation - in production would be more sophisticated
        if self.current_frame < 5 {
            let frame_data = serde_json::json!({
                "frame": self.current_frame,
                "priority": self.priority_threshold.value(),
                "data": format!("Frame #{}", self.current_frame)
            });
            
            self.current_frame += 1;
            Poll::Ready(Some(Ok(frame_data.to_string())))
        } else {
            Poll::Ready(None)
        }
    }
}

/// Create JSON streaming response
fn create_json_response<S>(stream: S) -> Result<Response, PjsError>
where
    S: Stream<Item = Result<String, PjsError>> + Send + 'static,
{
    let body = axum::body::Body::from_stream(stream);
    
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(body)
        .map_err(|e| PjsError::HttpError(e.to_string()))
}

/// Create NDJSON streaming response
fn create_ndjson_response<S>(stream: S) -> Result<Response, PjsError>
where
    S: Stream<Item = Result<String, PjsError>> + Send + 'static,
{
    let body = axum::body::Body::from_stream(stream);
    
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/x-ndjson")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(body)
        .map_err(|e| PjsError::HttpError(e.to_string()))
}

/// Create Server-Sent Events response
fn create_sse_response<S>(stream: S) -> Result<Response, PjsError>
where
    S: Stream<Item = Result<String, PjsError>> + Send + 'static,
{
    let sse_stream = stream.map(|item| {
        match item {
            Ok(data) => Ok::<String, std::convert::Infallible>(format!("data: {data}\n\n")),
            Err(e) => Ok::<String, std::convert::Infallible>(format!("event: error\ndata: {e}\n\n")),
        }
    });
    
    let body = axum::body::Body::from_stream(sse_stream);
    
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::CONNECTION, "keep-alive")
        .body(body)
        .map_err(|e| PjsError::HttpError(e.to_string()))
}

/// Get connection statistics
async fn connection_stats<CH, QH>(
    State(state): State<PjsAppState<CH, QH>>,
) -> Json<serde_json::Value>
where
    CH: CommandHandler<CreateSessionCommand, SessionId> + 
         CommandHandler<CreateStreamCommand, StreamId> +
         CommandHandler<StartStreamCommand, ()> +
         CommandHandler<CompleteStreamCommand, ()> +
         CommandHandler<CloseSessionCommand, ()> +
         CommandHandler<GenerateFramesCommand, Vec<Frame>> +
         CommandHandler<BatchGenerateFramesCommand, Vec<Frame>> +
         CommandHandler<AdjustPriorityThresholdCommand, ()> +
         Clone + Send + Sync + 'static,
    QH: QueryHandler<GetSessionQuery, SessionResponse> +
         QueryHandler<GetSessionHealthQuery, HealthResponse> +
         QueryHandler<GetActiveSessionsQuery, SessionsResponse> +
         Clone + Send + Sync + 'static,
{
    let stats = state.connection_manager.get_statistics().await;
    
    Json(serde_json::json!({
        "total_connections": stats.total_connections,
        "active_connections": stats.active_connections,
        "inactive_connections": stats.inactive_connections,
        "total_bytes_sent": stats.total_bytes_sent,
        "total_bytes_received": stats.total_bytes_received,
    }))
}

/// PJS-specific errors for HTTP endpoints
#[derive(Debug, thiserror::Error)]
pub enum PjsError {
    #[error("Application error: {0}")]
    Application(#[from] crate::application::ApplicationError),
    
    #[error("Invalid session ID: {0}")]
    InvalidSessionId(String),
    
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
    
    #[tokio::test]
    async fn test_system_health() {
        let response = system_health().await;
        let health_data: serde_json::Value = response.0;
        
        assert_eq!(health_data["status"], "healthy");
        assert!(!health_data["features"].as_array().unwrap().is_empty());
    }
}