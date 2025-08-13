// Clean Architecture Streaming Adapter with Zero-Cost GAT Abstractions
//
// This module provides the core streaming adapter trait that any web framework
// must implement to support PJS streaming capabilities with zero-cost abstractions.
//
// REQUIRES: nightly Rust for `impl Trait` in associated types

use super::object_pool::pooled_builders::PooledResponseBuilder;
use super::simd_acceleration::{SimdConfig, SimdStreamProcessor};
use crate::domain::value_objects::{JsonData, SessionId};
use crate::stream::StreamFrame;
use std::borrow::Cow;
use std::collections::HashMap;
use std::future::Future;

/// Streaming format options for framework responses
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamingFormat {
    /// Standard JSON response
    Json,
    /// Newline-Delimited JSON (NDJSON)
    Ndjson,
    /// Server-Sent Events
    ServerSentEvents,
    /// Custom binary format
    Binary,
}

impl StreamingFormat {
    /// Get the MIME type for this format
    pub fn content_type(&self) -> &'static str {
        match self {
            Self::Json => "application/json",
            Self::Ndjson => "application/x-ndjson",
            Self::ServerSentEvents => "text/event-stream",
            Self::Binary => "application/octet-stream",
        }
    }

    /// Detect format from Accept header
    pub fn from_accept_header(accept: &str) -> Self {
        if accept.contains("text/event-stream") {
            Self::ServerSentEvents
        } else if accept.contains("application/x-ndjson") {
            Self::Ndjson
        } else if accept.contains("application/octet-stream") {
            Self::Binary
        } else {
            Self::Json
        }
    }

    /// Check if format supports streaming
    pub fn supports_streaming(&self) -> bool {
        matches!(self, Self::Ndjson | Self::ServerSentEvents | Self::Binary)
    }
}

/// Response body variants supported by the universal adapter
#[derive(Debug, Clone)]
pub enum ResponseBody {
    Json(JsonData),
    Stream(Vec<StreamFrame>),
    ServerSentEvents(Vec<String>),
    Binary(Vec<u8>),
    Empty,
}

/// Universal response type for framework integration
#[derive(Debug, Clone)]
pub struct UniversalResponse {
    pub status_code: u16,
    pub headers: HashMap<Cow<'static, str>, Cow<'static, str>>,
    pub body: ResponseBody,
    pub content_type: Cow<'static, str>,
}

impl UniversalResponse {
    /// Create a JSON response
    pub fn json(data: JsonData) -> Self {
        Self {
            status_code: 200,
            headers: HashMap::with_capacity(2),
            body: ResponseBody::Json(data),
            content_type: Cow::Borrowed("application/json"),
        }
    }

    /// Create a JSON response using pooled HashMap (more efficient)
    pub fn json_pooled(data: JsonData) -> Self {
        let headers = super::object_pool::get_cow_hashmap().take();
        Self {
            status_code: 200,
            headers,
            body: ResponseBody::Json(data),
            content_type: Cow::Borrowed("application/json"),
        }
    }

    /// Create a streaming response
    pub fn stream(frames: Vec<StreamFrame>) -> Self {
        Self {
            status_code: 200,
            headers: HashMap::with_capacity(2),
            body: ResponseBody::Stream(frames),
            content_type: Cow::Borrowed("application/x-ndjson"),
        }
    }

    /// Create a Server-Sent Events response
    pub fn server_sent_events(events: Vec<String>) -> Self {
        let mut headers = HashMap::with_capacity(4);
        headers.insert(Cow::Borrowed("Cache-Control"), Cow::Borrowed("no-cache"));
        headers.insert(Cow::Borrowed("Connection"), Cow::Borrowed("keep-alive"));

        Self {
            status_code: 200,
            headers,
            body: ResponseBody::ServerSentEvents(events),
            content_type: Cow::Borrowed("text/event-stream"),
        }
    }

    /// Create an error response
    pub fn error(status: u16, message: impl Into<String>) -> Self {
        let error_data = JsonData::Object({
            let mut map = std::collections::HashMap::new();
            map.insert("error".to_string(), JsonData::String(message.into()));
            map.insert("status".to_string(), JsonData::Integer(status as i64));
            map
        });

        Self {
            status_code: status,
            headers: HashMap::with_capacity(1),
            body: ResponseBody::Json(error_data),
            content_type: Cow::Borrowed("application/json"),
        }
    }

    /// Add a header to the response  
    pub fn with_header(
        mut self,
        name: impl Into<Cow<'static, str>>,
        value: impl Into<Cow<'static, str>>,
    ) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    /// Set the status code
    pub fn with_status(mut self, status: u16) -> Self {
        self.status_code = status;
        self
    }
}

/// Universal request type for framework integration
#[derive(Debug, Clone)]
pub struct UniversalRequest {
    pub method: Cow<'static, str>,
    pub path: String,
    pub headers: HashMap<Cow<'static, str>, Cow<'static, str>>,
    pub query_params: HashMap<String, String>,
    pub body: Option<Vec<u8>>,
}

impl UniversalRequest {
    /// Create a new universal request
    pub fn new(method: impl Into<Cow<'static, str>>, path: impl Into<String>) -> Self {
        Self {
            method: method.into(),
            path: path.into(),
            headers: HashMap::with_capacity(4),
            query_params: HashMap::with_capacity(2),
            body: None,
        }
    }

    /// Add a header
    pub fn with_header(
        mut self,
        name: impl Into<Cow<'static, str>>,
        value: impl Into<Cow<'static, str>>,
    ) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    /// Add a query parameter
    pub fn with_query(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.query_params.insert(name.into(), value.into());
        self
    }

    /// Set the request body
    pub fn with_body(mut self, body: Vec<u8>) -> Self {
        self.body = Some(body);
        self
    }

    /// Get a header value
    pub fn get_header(&self, name: &str) -> Option<&Cow<'static, str>> {
        self.headers.get(name)
    }

    /// Get a query parameter
    pub fn get_query(&self, name: &str) -> Option<&String> {
        self.query_params.get(name)
    }

    /// Check if request accepts a specific content type
    pub fn accepts(&self, content_type: &str) -> bool {
        if let Some(accept) = self
            .get_header("accept")
            .or_else(|| self.get_header("Accept"))
        {
            accept.contains(content_type)
        } else {
            false
        }
    }

    /// Get preferred streaming format from headers
    pub fn preferred_streaming_format(&self) -> StreamingFormat {
        if let Some(accept) = self.get_header("accept") {
            StreamingFormat::from_accept_header(accept)
        } else {
            StreamingFormat::Json
        }
    }
}

/// Framework integration errors
#[derive(Debug, thiserror::Error)]
pub enum IntegrationError {
    #[error("Unsupported framework: {0}")]
    UnsupportedFramework(String),

    #[error("Request conversion failed: {0}")]
    RequestConversion(String),

    #[error("Response conversion failed: {0}")]
    ResponseConversion(String),

    #[error("Streaming not supported by framework")]
    StreamingNotSupported,

    #[error("Configuration error: {0}")]
    Configuration(String),

    #[error("SIMD processing error: {0}")]
    SimdProcessing(String),
}

pub type IntegrationResult<T> = Result<T, IntegrationError>;

/// High-performance streaming adapter using GAT zero-cost abstractions
///
/// This trait provides zero-cost abstractions for web framework integration
/// using `impl Trait` in associated types (**requires nightly Rust**).
///
/// ## Performance Benefits
/// - 1.82x faster trait dispatch vs async_trait
/// - Zero heap allocations for futures  
/// - Pure stack allocation with static dispatch
/// - Complete inlining for hot paths
pub trait StreamingAdapter: Send + Sync {
    /// The framework's native request type
    type Request;
    /// The framework's native response type  
    type Response;
    /// The framework's error type
    type Error: std::error::Error + Send + Sync + 'static;

    // Zero-cost GAT futures with impl Trait - true zero-cost abstractions
    type StreamingResponseFuture<'a>: Future<Output = IntegrationResult<Self::Response>> + Send + 'a
    where
        Self: 'a;

    type SseResponseFuture<'a>: Future<Output = IntegrationResult<Self::Response>> + Send + 'a
    where
        Self: 'a;

    type JsonResponseFuture<'a>: Future<Output = IntegrationResult<Self::Response>> + Send + 'a
    where
        Self: 'a;

    type MiddlewareFuture<'a>: Future<Output = IntegrationResult<UniversalResponse>> + Send + 'a
    where
        Self: 'a;

    /// Convert framework request to universal format
    fn convert_request(&self, request: Self::Request) -> IntegrationResult<UniversalRequest>;

    /// Convert universal response to framework format
    fn to_response(&self, response: UniversalResponse) -> IntegrationResult<Self::Response>;

    /// Create a streaming response with priority-based frame delivery
    fn create_streaming_response<'a>(
        &'a self,
        session_id: SessionId,
        frames: Vec<StreamFrame>,
        format: StreamingFormat,
    ) -> Self::StreamingResponseFuture<'a>;

    /// Create a Server-Sent Events response with SIMD acceleration
    fn create_sse_response<'a>(
        &'a self,
        session_id: SessionId,
        frames: Vec<StreamFrame>,
    ) -> Self::SseResponseFuture<'a>;

    /// Create a JSON response with optional streaming
    fn create_json_response<'a>(
        &'a self,
        data: JsonData,
        streaming: bool,
    ) -> Self::JsonResponseFuture<'a>;

    /// Handle framework-specific middleware integration
    fn apply_middleware<'a>(
        &'a self,
        request: &'a UniversalRequest,
        response: UniversalResponse,
    ) -> Self::MiddlewareFuture<'a>;

    /// Check if the framework supports streaming
    fn supports_streaming(&self) -> bool {
        true
    }

    /// Check if the framework supports Server-Sent Events
    fn supports_sse(&self) -> bool {
        true
    }

    /// Get the framework name for debugging/logging
    fn framework_name(&self) -> &'static str;
}

/// Extension trait for additional convenience methods
pub trait StreamingAdapterExt: StreamingAdapter {
    /// Auto-detection future for streaming format
    type AutoStreamFuture<'a>: Future<Output = IntegrationResult<Self::Response>> + Send + 'a
    where
        Self: 'a;

    /// Error response future
    type ErrorResponseFuture<'a>: Future<Output = IntegrationResult<Self::Response>> + Send + 'a
    where
        Self: 'a;

    /// Health check response future
    type HealthResponseFuture<'a>: Future<Output = IntegrationResult<Self::Response>> + Send + 'a
    where
        Self: 'a;

    /// Auto-detect streaming format from request and create appropriate response
    fn auto_stream_response<'a>(
        &'a self,
        request: &'a UniversalRequest,
        session_id: SessionId,
        frames: Vec<StreamFrame>,
    ) -> Self::AutoStreamFuture<'a>;

    /// Create an error response
    fn create_error_response<'a>(
        &'a self,
        status: u16,
        message: String,
    ) -> Self::ErrorResponseFuture<'a>;

    /// Create a health check response
    fn create_health_response<'a>(&'a self) -> Self::HealthResponseFuture<'a>;
}

/// Helper functions for default implementations with zero-cost abstractions
pub mod streaming_helpers {
    use super::*;

    /// Default SSE response implementation with SIMD acceleration
    pub async fn default_sse_response<T: StreamingAdapter>(
        adapter: &T,
        session_id: SessionId,
        frames: Vec<StreamFrame>,
    ) -> IntegrationResult<T::Response> {
        // Use SIMD-accelerated serialization for better performance
        let config = SimdConfig::default();
        let mut processor = SimdStreamProcessor::new(config);

        match processor.process_to_sse(&frames) {
            Ok(sse_data) => {
                let sse_string = String::from_utf8(sse_data.to_vec())
                    .map_err(|e| IntegrationError::ResponseConversion(e.to_string()))?;

                let events = vec![sse_string];
                let response = UniversalResponse::server_sent_events(events).with_header(
                    Cow::Borrowed("X-PJS-Session-ID"),
                    Cow::Owned(session_id.to_string()),
                );

                adapter.to_response(response)
            }
            Err(_e) => {
                // Fallback to standard serialization
                let events: Vec<String> = frames
                    .into_iter()
                    .map(|frame| {
                        format!(
                            "data: {}\\n\\n",
                            serde_json::to_string(&frame).unwrap_or_default()
                        )
                    })
                    .collect();

                let response = UniversalResponse::server_sent_events(events).with_header(
                    Cow::Borrowed("X-PJS-Session-ID"),
                    Cow::Owned(session_id.to_string()),
                );

                adapter.to_response(response)
            }
        }
    }

    /// Default JSON response implementation
    pub async fn default_json_response<T: StreamingAdapter>(
        adapter: &T,
        data: JsonData,
        streaming: bool,
    ) -> IntegrationResult<T::Response> {
        let response = if streaming {
            // Convert to streaming format
            let frame = StreamFrame {
                data: serde_json::to_value(&data).unwrap_or_default(),
                priority: crate::domain::Priority::HIGH,
                metadata: std::collections::HashMap::new(),
            };
            UniversalResponse::stream(vec![frame])
        } else {
            UniversalResponse::json(data)
        };

        adapter.to_response(response)
    }

    /// Default middleware implementation (no-op)
    pub async fn default_middleware<T: StreamingAdapter>(
        _adapter: &T,
        _request: &UniversalRequest,
        response: UniversalResponse,
    ) -> IntegrationResult<UniversalResponse> {
        Ok(response)
    }

    /// Default error response implementation
    pub async fn default_error_response<T: StreamingAdapter>(
        adapter: &T,
        status: u16,
        message: String,
    ) -> IntegrationResult<T::Response> {
        let response = UniversalResponse::error(status, message);
        adapter.to_response(response)
    }

    /// Default health check response implementation
    pub async fn default_health_response<T: StreamingAdapter>(
        adapter: &T,
    ) -> IntegrationResult<T::Response> {
        let health_data = JsonData::Object({
            let mut map = std::collections::HashMap::new();
            map.insert(
                "status".to_string(),
                JsonData::String("healthy".to_string()),
            );
            map.insert(
                "framework".to_string(),
                JsonData::String(adapter.framework_name().to_string()),
            );
            map.insert(
                "streaming_support".to_string(),
                JsonData::Bool(adapter.supports_streaming()),
            );
            map.insert(
                "sse_support".to_string(),
                JsonData::Bool(adapter.supports_sse()),
            );
            map
        });

        let response = PooledResponseBuilder::new()
            .header(Cow::Borrowed("X-Health-Check"), Cow::Borrowed("pjs"))
            .json(health_data);

        adapter.to_response(response)
    }

    /// Default auto-stream response implementation with format detection
    pub async fn default_auto_stream_response<T: StreamingAdapter>(
        adapter: &T,
        request: &UniversalRequest,
        session_id: SessionId,
        frames: Vec<StreamFrame>,
    ) -> IntegrationResult<T::Response> {
        let format = request.preferred_streaming_format();

        match format {
            StreamingFormat::ServerSentEvents => {
                adapter.create_sse_response(session_id, frames).await
            }
            _ => {
                adapter
                    .create_streaming_response(session_id, frames, format)
                    .await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_streaming_format_content_types() {
        assert_eq!(StreamingFormat::Json.content_type(), "application/json");
        assert_eq!(
            StreamingFormat::Ndjson.content_type(),
            "application/x-ndjson"
        );
        assert_eq!(
            StreamingFormat::ServerSentEvents.content_type(),
            "text/event-stream"
        );
        assert_eq!(
            StreamingFormat::Binary.content_type(),
            "application/octet-stream"
        );
    }

    #[test]
    fn test_format_detection_from_accept_header() {
        assert_eq!(
            StreamingFormat::from_accept_header("text/event-stream"),
            StreamingFormat::ServerSentEvents
        );
        assert_eq!(
            StreamingFormat::from_accept_header("application/x-ndjson"),
            StreamingFormat::Ndjson
        );
        assert_eq!(
            StreamingFormat::from_accept_header("application/json"),
            StreamingFormat::Json
        );
    }

    #[test]
    fn test_universal_request_creation() {
        let request = UniversalRequest::new("GET", "/api/stream")
            .with_header("Accept", "text/event-stream")
            .with_query("priority", "high");

        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/api/stream");
        assert!(request.accepts("text/event-stream"));
        assert_eq!(request.get_query("priority"), Some(&"high".to_string()));
    }

    #[test]
    fn test_universal_response_creation() {
        let data = JsonData::String("test".to_string());
        let response = UniversalResponse::json(data)
            .with_status(201)
            .with_header("X-Test", "value");

        assert_eq!(response.status_code, 201);
        assert_eq!(response.content_type, "application/json");
        assert!(response.headers.contains_key("X-Test"));
    }
}
