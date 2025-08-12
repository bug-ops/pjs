// Universal Framework Integration Layer
//
// This module provides trait-based integration patterns that allow PJS
// to work with any Rust web framework through a unified interface.

pub mod streaming_adapter;
pub mod universal_adapter;
pub mod framework_helpers;

pub use streaming_adapter::{StreamingAdapter, StreamingAdapterExt, StreamingFormat};
pub use universal_adapter::{UniversalAdapter, AdapterConfig};
pub use framework_helpers::*;

use crate::stream::StreamFrame;
use crate::domain::value_objects::JsonData;
use std::collections::HashMap;

/// Common response format that can be converted to any framework's response type
#[derive(Debug, Clone)]
pub struct UniversalResponse {
    pub status_code: u16,
    pub headers: HashMap<String, String>,
    pub body: ResponseBody,
    pub content_type: String,
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
            headers: HashMap::new(),
            body: ResponseBody::Json(data),
            content_type: "application/json".to_string(),
        }
    }

    /// Create a streaming response
    pub fn stream(frames: Vec<StreamFrame>) -> Self {
        Self {
            status_code: 200,
            headers: HashMap::new(),
            body: ResponseBody::Stream(frames),
            content_type: "application/x-ndjson".to_string(),
        }
    }

    /// Create a Server-Sent Events response
    pub fn server_sent_events(events: Vec<String>) -> Self {
        let mut headers = HashMap::new();
        headers.insert("Cache-Control".to_string(), "no-cache".to_string());
        headers.insert("Connection".to_string(), "keep-alive".to_string());

        Self {
            status_code: 200,
            headers,
            body: ResponseBody::ServerSentEvents(events),
            content_type: "text/event-stream".to_string(),
        }
    }

    /// Add a header to the response
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
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
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub query_params: HashMap<String, String>,
    pub body: Option<Vec<u8>>,
}

impl UniversalRequest {
    /// Create a new universal request
    pub fn new(method: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            method: method.into(),
            path: path.into(),
            headers: HashMap::new(),
            query_params: HashMap::new(),
            body: None,
        }
    }

    /// Add a header
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
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
    pub fn get_header(&self, name: &str) -> Option<&String> {
        self.headers.get(name)
    }

    /// Get a query parameter
    pub fn get_query(&self, name: &str) -> Option<&String> {
        self.query_params.get(name)
    }

    /// Check if request accepts a specific content type
    pub fn accepts(&self, content_type: &str) -> bool {
        if let Some(accept) = self.get_header("accept") {
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