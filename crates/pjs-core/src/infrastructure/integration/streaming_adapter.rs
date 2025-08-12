// Core streaming adapter trait for framework integration
//
// This trait defines the interface that any web framework must implement
// to support PJS streaming capabilities.

use super::{UniversalRequest, UniversalResponse, IntegrationResult};
use super::simd_acceleration::{SimdStreamProcessor, SimdConfig};
use super::object_pool::pooled_builders::PooledResponseBuilder;
use crate::stream::StreamFrame;
use crate::domain::value_objects::{SessionId, JsonData};
use async_trait::async_trait;
use std::borrow::Cow;

/// Core trait for integrating PJS streaming with any web framework
#[async_trait]
pub trait StreamingAdapter: Send + Sync {
    /// The framework's native request type
    type Request;
    /// The framework's native response type  
    type Response;
    /// The framework's error type
    type Error: std::error::Error + Send + Sync + 'static;

    /// Convert framework request to universal format
    fn from_request(&self, request: Self::Request) -> IntegrationResult<UniversalRequest>;

    /// Convert universal response to framework format
    fn to_response(&self, response: UniversalResponse) -> IntegrationResult<Self::Response>;

    /// Create a streaming response with priority-based frame delivery
    async fn create_streaming_response(
        &self,
        session_id: SessionId,
        frames: Vec<StreamFrame>,
        format: StreamingFormat,
    ) -> IntegrationResult<Self::Response>;

    /// Create a Server-Sent Events response with SIMD acceleration
    async fn create_sse_response(
        &self,
        session_id: SessionId,
        frames: Vec<StreamFrame>,
    ) -> IntegrationResult<Self::Response> {
        // Use SIMD-accelerated serialization for better performance
        let config = SimdConfig::default();
        let mut processor = SimdStreamProcessor::new(config);
        
        match processor.process_to_sse(&frames) {
            Ok(sse_data) => {
                let sse_string = String::from_utf8(sse_data.to_vec())
                    .map_err(|e| super::IntegrationError::ResponseConversion(e.to_string()))?;
                
                let events = vec![sse_string];
                let response = UniversalResponse::server_sent_events(events)
                    .with_header(Cow::Borrowed("X-PJS-Session-ID"), Cow::Owned(session_id.to_string()));

                self.to_response(response)
            }
            Err(_e) => {
                // Fallback to standard serialization
                let events: Vec<String> = frames
                    .into_iter()
                    .map(|frame| format!("data: {}\n\n", serde_json::to_string(&frame).unwrap_or_default()))
                    .collect();

                let response = UniversalResponse::server_sent_events(events)
                    .with_header(Cow::Borrowed("X-PJS-Session-ID"), Cow::Owned(session_id.to_string()));

                self.to_response(response)
            }
        }
    }

    /// Create a JSON response with optional streaming
    async fn create_json_response(
        &self,
        data: JsonData,
        streaming: bool,
    ) -> IntegrationResult<Self::Response> {
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

        self.to_response(response)
    }

    /// Handle framework-specific middleware integration
    async fn apply_middleware(
        &self,
        _request: &UniversalRequest,
        response: UniversalResponse,
    ) -> IntegrationResult<UniversalResponse> {
        // Default implementation - no middleware
        Ok(response)
    }

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

/// Streaming format options
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
}

/// Extension trait providing additional convenience methods
#[async_trait]
pub trait StreamingAdapterExt: StreamingAdapter {
    /// Auto-detect streaming format from request and create appropriate response
    async fn auto_stream_response(
        &self,
        request: &UniversalRequest,
        session_id: SessionId,
        frames: Vec<StreamFrame>,
    ) -> IntegrationResult<Self::Response> {
        let format = if let Some(accept) = request.get_header("accept") {
            StreamingFormat::from_accept_header(accept)
        } else {
            StreamingFormat::Json
        };

        match format {
            StreamingFormat::ServerSentEvents => {
                self.create_sse_response(session_id, frames).await
            }
            _ => {
                self.create_streaming_response(session_id, frames, format).await
            }
        }
    }

    /// Create an error response
    async fn create_error_response(
        &self,
        status: u16,
        message: impl Into<String> + Send,
    ) -> IntegrationResult<Self::Response> {
        let error_data = JsonData::Object({
            let mut map = std::collections::HashMap::new();
            map.insert("error".to_string(), JsonData::String(message.into()));
            map.insert("status".to_string(), JsonData::Integer(status as i64));
            map
        });

        // Use pooled response builder for better performance
        let response = PooledResponseBuilder::new()
            .status(status)
            .json(error_data);

        self.to_response(response)
    }

    /// Create a health check response
    async fn create_health_response(&self) -> IntegrationResult<Self::Response> {
        let health_data = JsonData::Object({
            let mut map = std::collections::HashMap::new();
            map.insert("status".to_string(), JsonData::String("healthy".to_string()));
            map.insert("framework".to_string(), JsonData::String(self.framework_name().to_string()));
            map.insert("streaming_support".to_string(), JsonData::Bool(self.supports_streaming()));
            map.insert("sse_support".to_string(), JsonData::Bool(self.supports_sse()));
            map
        });

        // Use pooled response builder for better performance
        let response = PooledResponseBuilder::new()
            .header(Cow::Borrowed("X-Health-Check"), Cow::Borrowed("pjs"))
            .json(health_data);
        
        self.to_response(response)
    }
}

// Blanket implementation for all StreamingAdapter implementors
impl<T: StreamingAdapter> StreamingAdapterExt for T {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_streaming_format_content_types() {
        assert_eq!(StreamingFormat::Json.content_type(), "application/json");
        assert_eq!(StreamingFormat::Ndjson.content_type(), "application/x-ndjson");
        assert_eq!(StreamingFormat::ServerSentEvents.content_type(), "text/event-stream");
        assert_eq!(StreamingFormat::Binary.content_type(), "application/octet-stream");
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
}