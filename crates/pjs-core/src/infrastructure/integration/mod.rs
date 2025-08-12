// Universal Framework Integration Layer
//
// This module provides trait-based integration patterns that allow PJS
// to work with any Rust web framework through a unified interface.

pub mod streaming_adapter;
pub mod universal_adapter;
pub mod framework_helpers;
pub mod simd_acceleration;

pub use streaming_adapter::{StreamingAdapter, StreamingAdapterExt, StreamingFormat};
pub use universal_adapter::{UniversalAdapter, AdapterConfig};
pub use framework_helpers::*;
pub use simd_acceleration::{SimdFrameSerializer, SimdJsonProcessor, SimdStreamProcessor, SimdConfig};

use crate::stream::StreamFrame;
use crate::domain::value_objects::JsonData;
use std::collections::HashMap;
use std::borrow::Cow;

/// Common response format that can be converted to any framework's response type
#[derive(Debug, Clone)]
pub struct UniversalResponse {
    pub status_code: u16,
    pub headers: HashMap<Cow<'static, str>, Cow<'static, str>>,
    pub body: ResponseBody,
    pub content_type: Cow<'static, str>,
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

impl UniversalResponse {
    /// Create a JSON response
    pub fn json(data: JsonData) -> Self {
        Self {
            status_code: 200,
            headers: HashMap::with_capacity(2), // Pre-allocate for common usage
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
        let mut headers = HashMap::with_capacity(4); // Pre-allocate for SSE headers
        headers.insert(Cow::Borrowed("Cache-Control"), Cow::Borrowed("no-cache"));
        headers.insert(Cow::Borrowed("Connection"), Cow::Borrowed("keep-alive"));

        Self {
            status_code: 200,
            headers,
            body: ResponseBody::ServerSentEvents(events),
            content_type: Cow::Borrowed("text/event-stream"),
        }
    }

    /// Add a header to the response  
    pub fn with_header(mut self, name: impl Into<Cow<'static, str>>, value: impl Into<Cow<'static, str>>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    /// Set the status code
    pub fn with_status(mut self, status: u16) -> Self {
        self.status_code = status;
        self
    }
}

/// Common request format that can be created from any framework's request type
#[derive(Debug, Clone)]
pub struct UniversalRequest {
    pub method: Cow<'static, str>,
    pub path: String, // Path is usually dynamic, so keep as String
    pub headers: HashMap<Cow<'static, str>, Cow<'static, str>>,
    pub query_params: HashMap<String, String>, // Query params are typically dynamic
    pub body: Option<Vec<u8>>,
}

impl UniversalRequest {
    /// Create a new universal request
    pub fn new(method: impl Into<Cow<'static, str>>, path: impl Into<String>) -> Self {
        Self {
            method: method.into(),
            path: path.into(),
            headers: HashMap::with_capacity(4), // Pre-allocate for common headers
            query_params: HashMap::with_capacity(2), // Pre-allocate for common params
            body: None,
        }
    }

    /// Add a header
    pub fn with_header(mut self, name: impl Into<Cow<'static, str>>, value: impl Into<Cow<'static, str>>) -> Self {
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
        // Try both lowercase and capitalized variations
        if let Some(accept) = self.get_header("accept").or_else(|| self.get_header("Accept")) {
            accept.contains(content_type)
        } else {
            false
        }
    }
}

/// Errors that can occur during framework integration
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
}

pub type IntegrationResult<T> = Result<T, IntegrationError>;