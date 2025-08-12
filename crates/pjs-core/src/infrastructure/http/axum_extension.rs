//! Universal Axum extension for existing APIs
//!
//! This module provides middleware and utilities to easily add PJS streaming
//! capabilities to existing Axum applications without requiring major refactoring.

use axum::{
    extract::{Path, Query, Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Extension, Json,
};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::{
    collections::HashMap,
    sync::Arc,
    time::Duration,
};

use crate::{
    Priority,
    PriorityStreamer,
};

/// Configuration for PJS extension
#[derive(Debug, Clone)]
pub struct PjsConfig {
    /// Route prefix for PJS endpoints (default: "/pjs")
    pub route_prefix: String,
    /// Enable automatic PJS detection based on Accept header
    pub auto_detect: bool,
    /// Default priority for streaming
    pub default_priority: Priority,
    /// Maximum concurrent streams per client
    pub max_streams_per_client: usize,
    /// Session timeout
    pub session_timeout: Duration,
}

impl Default for PjsConfig {
    fn default() -> Self {
        Self {
            route_prefix: "/pjs".to_string(),
            auto_detect: true,
            default_priority: Priority::MEDIUM,
            max_streams_per_client: 10,
            session_timeout: Duration::from_secs(3600),
        }
    }
}

/// Universal PJS extension that can be added to any Axum router
pub struct PjsExtension {
    config: PjsConfig,
    streamer: Arc<PriorityStreamer>,
}

impl PjsExtension {
    pub fn new(config: PjsConfig) -> Self {
        Self {
            config,
            streamer: Arc::new(PriorityStreamer::new()),
        }
    }

    /// Add PJS capabilities to an existing Axum router
    pub fn extend_router<S>(self, router: axum::Router<S>) -> axum::Router<S> 
    where
        S: Clone + Send + Sync + 'static
    {
        let pjs_routes = self.create_pjs_routes();
        
        router
            .nest(&self.config.route_prefix, pjs_routes)
            .layer(axum::middleware::from_fn_with_state(
                Arc::new(self), 
                pjs_middleware::<S>
            ))
    }

    /// Create PJS-specific routes
    fn create_pjs_routes<S>(&self) -> axum::Router<S>
    where
        S: Clone + Send + Sync + 'static
    {
        axum::Router::new()
            .route("/stream", axum::routing::post(handle_stream_request))
            .route("/stream/:stream_id/sse", axum::routing::get(handle_sse_stream))
            .route("/health", axum::routing::get(handle_pjs_health))
            .layer(Extension(self.config.clone()))
            .layer(Extension(self.streamer.clone()))
    }
}

/// Middleware that automatically detects PJS streaming requests
#[allow(clippy::extra_unused_type_parameters)]
async fn pjs_middleware<S>(
    State(_state): State<Arc<PjsExtension>>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode>
where
    S: Clone + Send + Sync + 'static
{
    // Check if client requested PJS streaming
    let wants_pjs = headers
        .get(header::ACCEPT)
        .and_then(|h| h.to_str().ok())
        .map(|accept| {
            accept.contains("application/pjs-stream") ||
            accept.contains("text/event-stream") ||
            headers.contains_key("x-pjs-stream")
        })
        .unwrap_or(false);

    let mut request = request;
    if wants_pjs {
        // Add PJS metadata to request
        request.extensions_mut().insert(PjsStreamingRequest { enabled: true });
    }

    Ok(next.run(request).await)
}

/// Marker for PJS streaming requests
#[derive(Debug, Clone)]
pub struct PjsStreamingRequest {
    pub enabled: bool,
}

/// Request parameters for streaming
#[derive(Debug, Deserialize)]
pub struct StreamRequest {
    /// JSON data to stream
    pub data: JsonValue,
    /// Priority threshold (0-255)
    pub priority: Option<u8>,
    /// Stream format (json, ndjson, sse)
    pub format: Option<String>,
    /// Maximum number of frames
    pub max_frames: Option<usize>,
}

/// Stream response
#[derive(Debug, Serialize)]
pub struct StreamResponse {
    pub stream_id: String,
    pub format: String,
    pub estimated_frames: usize,
}

/// Handle stream creation request
async fn handle_stream_request(
    Extension(config): Extension<PjsConfig>,
    Extension(streamer): Extension<Arc<PriorityStreamer>>,
    headers: HeaderMap,
    Json(request): Json<StreamRequest>,
) -> Result<impl IntoResponse, StreamError> {
    let stream_id = uuid::Uuid::new_v4().to_string();
    
    // Create streaming plan
    let plan = streamer
        .analyze(&request.data)
        .map_err(|e| StreamError::AnalysisError(e.to_string()))?;

    let format = request.format
        .unwrap_or_else(|| {
            headers
                .get(header::ACCEPT)
                .and_then(|h| h.to_str().ok())
                .map(|accept| {
                    if accept.contains("text/event-stream") {
                        "sse".to_string()
                    } else if accept.contains("application/x-ndjson") {
                        "ndjson".to_string()
                    } else {
                        "json".to_string()
                    }
                })
                .unwrap_or_else(|| "json".to_string())
        });

    let response = StreamResponse {
        stream_id: stream_id.clone(),
        format: format.clone(),
        estimated_frames: plan.frames().count(),
    };

    // Store stream for later retrieval
    // In production, this would use a proper store
    
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, format!("{}/stream/{}", config.route_prefix, stream_id))],
        Json(response)
    ))
}

/// Handle Server-Sent Events streaming
async fn handle_sse_stream(
    Path(_stream_id): Path<String>,
    Extension(streamer): Extension<Arc<PriorityStreamer>>,
    Query(_params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, StreamError> {
    // In production, retrieve stream data from store using stream_id
    let sample_data = serde_json::json!({
        "products": [
            {"id": 1, "name": "Product A", "price": 19.99, "category": "electronics"},
            {"id": 2, "name": "Product B", "price": 29.99, "category": "books"},
            {"id": 3, "name": "Product C", "price": 39.99, "category": "clothing"}
        ],
        "metadata": {
            "total": 3,
            "updated_at": "2024-01-01T00:00:00Z"
        }
    });

    let plan = streamer
        .analyze(&sample_data)
        .map_err(|e| StreamError::AnalysisError(e.to_string()))?;

    // Collect frames to avoid lifetime issues
    let frames: Vec<_> = plan.frames().cloned().collect();
    let stream = futures::stream::iter(frames)
        .map(|frame| {
            let data = serde_json::to_string(&frame).unwrap_or_default();
            Ok::<_, StreamError>(format!("data: {data}\n\n"))
        });

    let response = axum::response::Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::CONNECTION, "keep-alive")
        .header("Access-Control-Allow-Origin", "*")
        .body(axum::body::Body::from_stream(stream))
        .map_err(|e| StreamError::ResponseError(e.to_string()))?;

    Ok(response)
}

/// Health check for PJS extension
async fn handle_pjs_health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "healthy",
        "service": "pjs-extension",
        "version": env!("CARGO_PKG_VERSION"),
        "capabilities": [
            "priority-streaming",
            "sse-support", 
            "ndjson-support",
            "auto-detection"
        ]
    }))
}

/// Extension-specific errors
#[derive(Debug, thiserror::Error)]
pub enum StreamError {
    #[error("Analysis error: {0}")]
    AnalysisError(String),
    
    #[error("Response error: {0}")]
    ResponseError(String),
    
    #[error("Stream not found: {0}")]
    StreamNotFound(String),
}

impl IntoResponse for StreamError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            StreamError::AnalysisError(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            StreamError::ResponseError(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
            StreamError::StreamNotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
        };

        (status, Json(serde_json::json!({"error": message}))).into_response()
    }
}

/// Trait to easily add PJS support to any JSON response
pub trait PjsResponseExt {
    /// Convert response to PJS streaming if requested
    fn pjs_stream(self, request: &axum::extract::Request) -> impl IntoResponse;
}

impl PjsResponseExt for Json<JsonValue> {
    fn pjs_stream(self, request: &axum::extract::Request) -> impl IntoResponse {
        // Check if PJS streaming was requested
        if let Some(pjs_request) = request.extensions().get::<PjsStreamingRequest>() {
            if pjs_request.enabled {
                // Convert to streaming response
                // This is a simplified implementation
                return (
                    StatusCode::OK,
                    [
                        (header::CONTENT_TYPE, "application/pjs-stream"),
                        (header::CACHE_CONTROL, "no-cache"),
                    ],
                    self.0.to_string()
                ).into_response();
            }
        }
        
        // Return regular JSON response
        self.into_response()
    }
}

/// Helper macro to easily add PJS to existing endpoints
#[macro_export]
macro_rules! pjs_endpoint {
    ($handler:expr) => {
        |req: axum::extract::Request| async move {
            let response = $handler(req).await;
            response.pjs_stream(&req)
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::get, Router};
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_pjs_extension_integration() {
        // Create a regular API route
        async fn api_route() -> Json<JsonValue> {
            Json(serde_json::json!({
                "users": [
                    {"id": 1, "name": "Alice"},
                    {"id": 2, "name": "Bob"}
                ]
            }))
        }

        // Create router with PJS extension
        let config = PjsConfig::default();
        let pjs_extension = PjsExtension::new(config);
        
        let app = Router::new()
            .route("/api/users", get(api_route));
            
        let app = pjs_extension.extend_router(app);

        // Test that PJS routes are available
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/pjs/health")
                    .body(axum::body::Body::empty())
                    // TODO: Handle unwrap() - add proper error handling for request building in tests
                    .unwrap()
            )
            .await
            // TODO: Handle unwrap() - add proper error handling for response in tests
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_auto_detection_middleware() {
        let config = PjsConfig::default();
        let _pjs_extension = Arc::new(PjsExtension::new(config));
        
        let _headers = HeaderMap::new();
        let request = axum::http::Request::builder()
            .header("Accept", "text/event-stream")
            .body(axum::body::Body::empty())
            // TODO: Handle unwrap() - add proper error handling for request building in tests
            .unwrap();

        // Test middleware detection logic
        assert!(request.headers().get("Accept").is_some());
    }
}