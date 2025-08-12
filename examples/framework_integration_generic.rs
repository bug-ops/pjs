//! Example: Universal framework integration with any generic framework
//!
//! Shows how to implement StreamingAdapter for any web framework
//! using the universal integration layer.

use pjson_rs::{
    infrastructure::integration::{
        StreamingAdapter, UniversalRequest, UniversalResponse, 
        IntegrationResult, StreamingFormat, HttpStatus, ResponseBuilder,
        HttpMethod, IntoUniversalRequest, FromUniversalResponse,
        RequestExtractor, default_cors_headers, default_security_headers,
    },
    domain::value_objects::{SessionId, JsonData},
    stream::StreamFrame,
    domain::Priority,
};
use async_trait::async_trait;
use std::collections::HashMap;

/// Generic framework request representation
#[derive(Debug, Clone)]
pub struct GenericRequest {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: Option<Vec<u8>>,
}

impl GenericRequest {
    pub fn new(method: &str, path: &str) -> Self {
        Self {
            method: method.to_string(),
            path: path.to_string(),
            headers: HashMap::new(),
            body: None,
        }
    }

    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.insert(name.to_string(), value.to_string());
        self
    }

    pub fn with_body(mut self, body: Vec<u8>) -> Self {
        self.body = Some(body);
        self
    }
}

impl IntoUniversalRequest for GenericRequest {
    fn into_universal(self) -> IntegrationResult<UniversalRequest> {
        let query_params = RequestExtractor::extract_query_params(&self.path);
        let path = RequestExtractor::extract_path(&self.path);
        
        Ok(UniversalRequest {
            method: self.method,
            path,
            headers: self.headers,
            query_params,
            body: self.body,
        })
    }
}

/// Generic framework response representation  
#[derive(Debug, Clone)]
pub struct GenericResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl GenericResponse {
    pub fn new(status: u16) -> Self {
        Self {
            status,
            headers: HashMap::new(),
            body: Vec::new(),
        }
    }
}

impl FromUniversalResponse<GenericResponse> for GenericResponse {
    fn from_universal(response: UniversalResponse) -> IntegrationResult<GenericResponse> {
        let body = match response.body {
            crate::infrastructure::integration::ResponseBody::Json(data) => {
                serde_json::to_vec(&serde_json::to_value(&data).unwrap_or_default())
                    .unwrap_or_default()
            }
            crate::infrastructure::integration::ResponseBody::Stream(frames) => {
                frames.into_iter()
                    .map(|frame| serde_json::to_string(&frame).unwrap_or_default())
                    .collect::<Vec<_>>()
                    .join("\n")
                    .into_bytes()
            }
            crate::infrastructure::integration::ResponseBody::ServerSentEvents(events) => {
                events.join("").into_bytes()
            }
            crate::infrastructure::integration::ResponseBody::Binary(data) => data,
            crate::infrastructure::integration::ResponseBody::Empty => Vec::new(),
        };

        Ok(GenericResponse {
            status: response.status_code,
            headers: response.headers,
            body,
        })
    }
}

/// Generic framework adapter implementation
pub struct GenericFrameworkAdapter {
    name: String,
    enable_cors: bool,
    enable_security_headers: bool,
}

impl GenericFrameworkAdapter {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            enable_cors: true,
            enable_security_headers: true,
        }
    }

    pub fn with_cors(mut self, enabled: bool) -> Self {
        self.enable_cors = enabled;
        self
    }

    pub fn with_security_headers(mut self, enabled: bool) -> Self {
        self.enable_security_headers = enabled;
        self
    }
}

#[async_trait]
impl StreamingAdapter for GenericFrameworkAdapter {
    type Request = GenericRequest;
    type Response = GenericResponse;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn from_request(&self, request: Self::Request) -> IntegrationResult<UniversalRequest> {
        request.into_universal()
    }

    fn to_response(&self, mut response: UniversalResponse) -> IntegrationResult<Self::Response> {
        // Add CORS headers if enabled
        if self.enable_cors {
            for (key, value) in default_cors_headers() {
                response.headers.insert(key, value);
            }
        }

        // Add security headers if enabled
        if self.enable_security_headers {
            for (key, value) in default_security_headers() {
                response.headers.insert(key, value);
            }
        }

        GenericResponse::from_universal(response)
    }

    async fn create_streaming_response(
        &self,
        _session_id: SessionId,
        frames: Vec<StreamFrame>,
        format: StreamingFormat,
    ) -> IntegrationResult<Self::Response> {
        let response = match format {
            StreamingFormat::Json => {
                let frames_json: Vec<serde_json::Value> = frames.into_iter()
                    .map(|frame| serde_json::to_value(&frame).unwrap_or_default())
                    .collect();
                
                UniversalResponse::json(JsonData::Array(
                    frames_json.into_iter()
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

    async fn apply_middleware(
        &self,
        _request: &UniversalRequest,
        response: UniversalResponse,
    ) -> IntegrationResult<UniversalResponse> {
        // Add custom middleware logic here
        println!("🔧 Applying {} middleware", self.name);
        Ok(response)
    }

    fn framework_name(&self) -> &'static str {
        // Note: This should ideally return the actual framework name
        // For a real implementation, you'd want to store this as a &'static str
        "generic"
    }
}

/// Example usage of the generic framework adapter
async fn example_usage() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Generic Framework Integration Example");
    
    // Create adapter for your framework
    let adapter = GenericFrameworkAdapter::new("MyCustomFramework")
        .with_cors(true)
        .with_security_headers(true);
    
    // Create a sample request
    let request = GenericRequest::new("POST", "/api/stream/session123?format=sse")
        .with_header("Content-Type", "application/json")
        .with_header("Accept", "text/event-stream")
        .with_body(b"{\"message\": \"Hello, PJS!\"}".to_vec());
    
    // Convert to universal format
    let universal_request = adapter.from_request(request)?;
    println!("📥 Received request: {} {}", universal_request.method, universal_request.path);
    
    // Create sample streaming data
    let session_id = SessionId::from_string("session123");
    let frames = vec![
        StreamFrame {
            data: serde_json::json!({"id": 1, "message": "First frame", "priority": "critical"}),
            priority: Priority::CRITICAL,
            metadata: HashMap::new(),
        },
        StreamFrame {
            data: serde_json::json!({"id": 2, "message": "Second frame", "priority": "high"}),
            priority: Priority::HIGH,
            metadata: HashMap::new(),
        },
        StreamFrame {
            data: serde_json::json!({"id": 3, "message": "Third frame", "priority": "medium"}),
            priority: Priority::MEDIUM,
            metadata: HashMap::new(),
        },
    ];
    
    // Create streaming response
    let format = if universal_request.accepts("text/event-stream") {
        StreamingFormat::ServerSentEvents
    } else {
        StreamingFormat::Json
    };
    
    let response = adapter.create_streaming_response(session_id, frames, format).await?;
    println!("📤 Created response with status: {}", response.status);
    println!("📋 Response headers: {:?}", response.headers);
    println!("📄 Response body length: {} bytes", response.body.len());
    
    // Test health check
    let health_response = adapter.create_health_response().await?;
    println!("❤️  Health check status: {}", health_response.status);
    
    // Test error response
    let error_response = adapter.create_error_response(404, "Resource not found").await?;
    println!("❌ Error response status: {}", error_response.status);
    
    println!("✅ Generic framework integration example completed successfully!");
    
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    example_usage().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_generic_adapter() {
        let adapter = GenericFrameworkAdapter::new("TestFramework");
        assert_eq!(adapter.framework_name(), "generic");
        assert!(adapter.supports_streaming());
        assert!(adapter.supports_sse());
    }

    #[tokio::test]
    async fn test_request_conversion() {
        let adapter = GenericFrameworkAdapter::new("Test");
        let request = GenericRequest::new("GET", "/api/test?param=value")
            .with_header("Content-Type", "application/json");
        
        let universal = adapter.from_request(request).unwrap();
        assert_eq!(universal.method, "GET");
        assert_eq!(universal.path, "/api/test");
        assert_eq!(universal.get_query("param"), Some(&"value".to_string()));
    }

    #[tokio::test]
    async fn test_streaming_formats() {
        let adapter = GenericFrameworkAdapter::new("Test");
        let session_id = SessionId::from_string("test");
        let frames = vec![
            StreamFrame {
                data: serde_json::json!({"test": "data"}),
                priority: Priority::HIGH,
                metadata: HashMap::new(),
            }
        ];

        // Test JSON format
        let response = adapter.create_streaming_response(
            session_id.clone(),
            frames.clone(),
            StreamingFormat::Json
        ).await.unwrap();
        assert_eq!(response.status, 200);

        // Test SSE format
        let response = adapter.create_streaming_response(
            session_id,
            frames,
            StreamingFormat::ServerSentEvents
        ).await.unwrap();
        assert_eq!(response.status, 200);
    }
}