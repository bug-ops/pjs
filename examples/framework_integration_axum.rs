//! Example: Universal framework integration with Axum
//!
//! Demonstrates how to use the universal framework integration layer
//! to create PJS-enabled endpoints with Axum.

use pjson_rs::{
    infrastructure::integration::{
        StreamingAdapter, UniversalRequest, UniversalResponse, UniversalAdapter,
        IntegrationResult, StreamingFormat, HttpStatus, ResponseBuilder
    },
    domain::value_objects::{SessionId, JsonData},
    stream::StreamFrame,
    domain::Priority,
};
use axum::{
    extract::{Path, Query},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::net::TcpListener;

/// Axum-specific adapter implementation
pub struct AxumAdapter;

#[async_trait]
impl StreamingAdapter for AxumAdapter {
    type Request = axum::extract::Request;
    type Response = Response;
    type Error = axum::Error;

    fn from_request(&self, request: Self::Request) -> IntegrationResult<UniversalRequest> {
        let (parts, body) = request.into_parts();
        
        // Extract headers
        let mut headers = HashMap::new();
        for (name, value) in parts.headers.iter() {
            if let Ok(value_str) = value.to_str() {
                headers.insert(name.to_string(), value_str.to_string());
            }
        }

        // Extract query parameters from URI
        let query_params = if let Some(query) = parts.uri.query() {
            query.split('&')
                .filter_map(|pair| {
                    let mut split = pair.split('=');
                    let key = split.next()?;
                    let value = split.next().unwrap_or("");
                    Some((
                        urlencoding::decode(key).ok()?.to_string(),
                        urlencoding::decode(value).ok()?.to_string(),
                    ))
                })
                .collect()
        } else {
            HashMap::new()
        };

        // Create universal request
        let universal_req = UniversalRequest::new(parts.method.to_string(), parts.uri.path())
            .with_body(axum::body::to_bytes(body, usize::MAX).await.map_err(|e| {
                crate::infrastructure::integration::IntegrationError::RequestConversion(e.to_string())
            })?.to_vec());

        Ok(universal_req)
    }

    fn to_response(&self, response: UniversalResponse) -> IntegrationResult<Self::Response> {
        let mut builder = Response::builder()
            .status(StatusCode::from_u16(response.status_code).unwrap_or(StatusCode::OK));

        // Add headers
        for (name, value) in response.headers {
            builder = builder.header(name, value);
        }

        // Set content type
        builder = builder.header("content-type", response.content_type);

        // Create body based on response type
        let body = match response.body {
            crate::infrastructure::integration::ResponseBody::Json(data) => {
                let json_value = serde_json::to_value(&data)
                    .map_err(|e| crate::infrastructure::integration::IntegrationError::ResponseConversion(e.to_string()))?;
                axum::body::Body::from(serde_json::to_vec(&json_value).unwrap_or_default())
            }
            crate::infrastructure::integration::ResponseBody::Stream(frames) => {
                let ndjson = frames.iter()
                    .map(|frame| serde_json::to_string(frame).unwrap_or_default())
                    .collect::<Vec<_>>()
                    .join("\n");
                axum::body::Body::from(ndjson)
            }
            crate::infrastructure::integration::ResponseBody::ServerSentEvents(events) => {
                axum::body::Body::from(events.join(""))
            }
            crate::infrastructure::integration::ResponseBody::Binary(data) => {
                axum::body::Body::from(data)
            }
            crate::infrastructure::integration::ResponseBody::Empty => {
                axum::body::Body::empty()
            }
        };

        builder.body(body).map_err(|e| {
            crate::infrastructure::integration::IntegrationError::ResponseConversion(e.to_string())
        })
    }

    async fn create_streaming_response(
        &self,
        _session_id: SessionId,
        frames: Vec<StreamFrame>,
        format: StreamingFormat,
    ) -> IntegrationResult<Self::Response> {
        let response = match format {
            StreamingFormat::Json => {
                let json_frames: Vec<_> = frames.into_iter()
                    .map(|frame| serde_json::to_value(&frame).unwrap_or_default())
                    .collect();
                UniversalResponse::json(JsonData::Array(
                    json_frames.into_iter()
                        .map(JsonData::from)
                        .collect()
                ))
            }
            StreamingFormat::Ndjson => {
                let ndjson_lines: Vec<String> = frames.into_iter()
                    .map(|frame| serde_json::to_string(&frame).unwrap_or_default())
                    .collect();
                UniversalResponse {
                    status_code: 200,
                    headers: HashMap::new(),
                    body: crate::infrastructure::integration::ResponseBody::ServerSentEvents(ndjson_lines),
                    content_type: format.content_type().to_string(),
                }
            }
            StreamingFormat::ServerSentEvents => {
                return self.create_sse_response(_session_id, frames).await;
            }
            StreamingFormat::Binary => {
                let binary_data: Vec<u8> = frames.into_iter()
                    .flat_map(|frame| serde_json::to_vec(&frame).unwrap_or_default())
                    .collect();
                UniversalResponse {
                    status_code: 200,
                    headers: HashMap::new(),
                    body: crate::infrastructure::integration::ResponseBody::Binary(binary_data),
                    content_type: format.content_type().to_string(),
                }
            }
        };

        self.to_response(response)
    }

    fn framework_name(&self) -> &'static str {
        "axum"
    }
}

/// Example request/response types
#[derive(Debug, Deserialize)]
struct StreamRequest {
    data: serde_json::Value,
    priority_threshold: Option<u8>,
    format: Option<String>,
}

#[derive(Debug, Serialize)]
struct StreamResponse {
    session_id: String,
    message: String,
}

/// Health check handler using universal adapter
async fn health_handler() -> impl IntoResponse {
    let adapter = AxumAdapter;
    match adapter.create_health_response().await {
        Ok(response) => response,
        Err(e) => {
            eprintln!("Health check error: {e}");
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(axum::body::Body::from("Internal server error"))
                .unwrap()
        }
    }
}

/// Streaming handler using universal adapter
async fn stream_handler(
    Path(session_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    Json(request): Json<StreamRequest>,
) -> impl IntoResponse {
    let adapter = AxumAdapter;
    
    // Parse session ID
    let session_id = SessionId::from_string(&session_id);
    
    // Convert request data to frames
    let frame = StreamFrame {
        data: request.data,
        priority: Priority::HIGH,
        metadata: HashMap::new(),
    };
    
    // Determine format from query params or Accept header
    let format = params.get("format")
        .map(|f| match f.as_str() {
            "sse" => StreamingFormat::ServerSentEvents,
            "ndjson" => StreamingFormat::Ndjson,
            "binary" => StreamingFormat::Binary,
            _ => StreamingFormat::Json,
        })
        .unwrap_or(StreamingFormat::Json);
    
    match adapter.create_streaming_response(session_id, vec![frame], format).await {
        Ok(response) => response,
        Err(e) => {
            eprintln!("Streaming error: {e}");
            match adapter.create_error_response(500, format!("Streaming failed: {e}")).await {
                Ok(error_response) => error_response,
                Err(_) => Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(axum::body::Body::from("Internal server error"))
                    .unwrap()
            }
        }
    }
}

/// Error handler using universal adapter
async fn error_handler() -> impl IntoResponse {
    let adapter = AxumAdapter;
    match adapter.create_error_response(404, "Resource not found").await {
        Ok(response) => response,
        Err(_) => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(axum::body::Body::from("Not found"))
            .unwrap()
    }
}

/// Create router with universal PJS integration
fn create_router() -> Router {
    Router::new()
        .route("/health", get(health_handler))
        .route("/stream/{session_id}", post(stream_handler))
        .route("/error", get(error_handler))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::init();
    
    println!("🚀 Starting PJS Universal Framework Integration Demo (Axum)");
    println!("📡 Server will be available at http://127.0.0.1:3000");
    println!();
    println!("Available endpoints:");
    println!("  GET  /health                    - Health check with framework info");
    println!("  POST /stream/{{session_id}}       - Stream JSON with priority");
    println!("  GET  /error                     - Example error response");
    println!();
    println!("Query parameters for /stream:");
    println!("  ?format=json     - Standard JSON response");
    println!("  ?format=sse      - Server-Sent Events");
    println!("  ?format=ndjson   - Newline-Delimited JSON");
    println!("  ?format=binary   - Binary format");
    println!();

    let app = create_router();
    let listener = TcpListener::bind("127.0.0.1:3000").await?;
    
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_axum_adapter_creation() {
        let adapter = AxumAdapter;
        assert_eq!(adapter.framework_name(), "axum");
        assert!(adapter.supports_streaming());
        assert!(adapter.supports_sse());
    }
}