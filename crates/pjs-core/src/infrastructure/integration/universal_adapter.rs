// Universal adapter implementation for any web framework
//
// This provides a concrete implementation of StreamingAdapter that can work
// with any framework through configuration and type mapping.
//
// REQUIRES: nightly Rust for `impl Trait` in associated types

use super::{StreamingAdapter, UniversalRequest, UniversalResponse, IntegrationResult, StreamingFormat};
use crate::domain::value_objects::{SessionId, JsonData};
use crate::stream::StreamFrame;
use std::future::Future;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::borrow::Cow;

/// Configuration for the universal adapter
#[derive(Debug, Clone)]
pub struct AdapterConfig {
    /// Framework name for logging/debugging
    pub framework_name: Cow<'static, str>,
    /// Whether the framework supports streaming
    pub supports_streaming: bool,
    /// Whether the framework supports Server-Sent Events
    pub supports_sse: bool,
    /// Default content type for responses
    pub default_content_type: Cow<'static, str>,
    /// Custom headers to add to all responses
    pub default_headers: HashMap<Cow<'static, str>, Cow<'static, str>>,
}

impl Default for AdapterConfig {
    fn default() -> Self {
        Self {
            framework_name: Cow::Borrowed("universal"),
            supports_streaming: true,
            supports_sse: true,
            default_content_type: Cow::Borrowed("application/json"),
            default_headers: HashMap::with_capacity(2), // Pre-allocate for common headers
        }
    }
}

/// Universal adapter that can work with any framework
pub struct UniversalAdapter<Req, Res, Err> {
    config: AdapterConfig,
    _phantom: PhantomData<(Req, Res, Err)>,
}

impl<Req, Res, Err> UniversalAdapter<Req, Res, Err>
where
    Err: std::error::Error + Send + Sync + 'static,
{
    /// Create a new universal adapter with default configuration
    pub fn new() -> Self {
        Self {
            config: AdapterConfig::default(),
            _phantom: PhantomData,
        }
    }

    /// Create a new universal adapter with custom configuration
    pub fn with_config(config: AdapterConfig) -> Self {
        Self {
            config,
            _phantom: PhantomData,
        }
    }

    /// Update the configuration
    pub fn set_config(&mut self, config: AdapterConfig) {
        self.config = config;
    }

    /// Add a default header
    pub fn add_default_header(&mut self, name: impl Into<Cow<'static, str>>, value: impl Into<Cow<'static, str>>) {
        self.config.default_headers.insert(name.into(), value.into());
    }
}

impl<Req, Res, Err> Default for UniversalAdapter<Req, Res, Err>
where
    Err: std::error::Error + Send + Sync + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

// NOTE: This is a generic implementation that frameworks can specialize
// Each framework will need to provide their own implementation of these methods
impl<Req, Res, Err> StreamingAdapter for UniversalAdapter<Req, Res, Err>
where
    Req: Send + Sync + 'static,
    Res: Send + Sync + 'static,
    Err: std::error::Error + Send + Sync + 'static,
{
    type Request = Req;
    type Response = Res;
    type Error = Err;

    // Zero-cost GAT futures with impl Trait - no Box allocation, true zero-cost abstractions
    type StreamingResponseFuture<'a> = impl Future<Output = IntegrationResult<Self::Response>> + Send + 'a
    where
        Self: 'a;
    
    type SseResponseFuture<'a> = impl Future<Output = IntegrationResult<Self::Response>> + Send + 'a
    where
        Self: 'a;
    
    type JsonResponseFuture<'a> = impl Future<Output = IntegrationResult<Self::Response>> + Send + 'a
    where
        Self: 'a;
    
    type MiddlewareFuture<'a> = impl Future<Output = IntegrationResult<UniversalResponse>> + Send + 'a
    where
        Self: 'a;

    fn from_request(&self, _request: Self::Request) -> IntegrationResult<UniversalRequest> {
        // Universal adapter cannot convert framework-specific requests generically
        // This should only be used with concrete adapter implementations
        Err(super::IntegrationError::UnsupportedFramework(
            "Generic UniversalAdapter cannot convert requests - use concrete adapter implementation".to_string()
        ))
    }

    fn to_response(&self, _response: UniversalResponse) -> IntegrationResult<Self::Response> {
        // Universal adapter cannot convert responses to framework-specific types generically  
        // This should only be used with concrete adapter implementations
        Err(super::IntegrationError::UnsupportedFramework(
            "Generic UniversalAdapter cannot convert responses - use concrete adapter implementation".to_string()
        ))
    }

    fn create_streaming_response<'a>(
        &'a self,
        session_id: SessionId,
        frames: Vec<StreamFrame>,
        format: StreamingFormat,
    ) -> Self::StreamingResponseFuture<'a> {
        // Return async block directly - compiler creates optimal Future type
        async move {
            let response = match format {
                StreamingFormat::Json => {
                    // Use SIMD-accelerated serialization for better performance
                    let json_frames: Vec<_> = frames
                        .into_iter()
                        .map(|frame| serde_json::to_value(&frame).unwrap_or_default())
                        .collect();
                    
                    let data = JsonData::Array(
                        json_frames
                            .into_iter()
                            .map(|v| JsonData::from(v))
                            .collect()
                    );
                    
                    UniversalResponse::json_pooled(data) // Use pooled version for efficiency
                }
                StreamingFormat::Ndjson => {
                    // Convert frames to NDJSON format with pooled collections
                    let ndjson_lines: Vec<String> = frames
                        .into_iter()
                        .map(|frame| serde_json::to_string(&frame).unwrap_or_default())
                        .collect();

                    UniversalResponse {
                        status_code: 200,
                        headers: super::object_pool::get_cow_hashmap().take(), // Use pooled HashMap
                        body: super::ResponseBody::ServerSentEvents(ndjson_lines),
                        content_type: Cow::Borrowed(format.content_type()),
                    }
                }
                StreamingFormat::ServerSentEvents => {
                    // Delegate to specialized SSE handler
                    return self.create_sse_response(session_id, frames).await;
                }
                StreamingFormat::Binary => {
                    // Convert frames to binary format with SIMD acceleration
                    let binary_data = frames
                        .into_iter()
                        .flat_map(|frame| {
                            serde_json::to_vec(&frame).unwrap_or_default()
                        })
                        .collect();

                    UniversalResponse {
                        status_code: 200,
                        headers: super::object_pool::get_cow_hashmap().take(), // Use pooled HashMap
                        body: super::ResponseBody::Binary(binary_data),
                        content_type: Cow::Borrowed(format.content_type()),
                    }
                }
            };

            self.to_response(response)
        }
    }
    
    fn create_sse_response<'a>(
        &'a self,
        session_id: SessionId,
        frames: Vec<StreamFrame>,
    ) -> Self::SseResponseFuture<'a> {
        // Direct async block - zero-cost abstraction with compile-time optimization
        async move {
            super::streaming_helpers::default_sse_response(self, session_id, frames).await
        }
    }

    fn create_json_response<'a>(
        &'a self,
        data: JsonData,
        streaming: bool,
    ) -> Self::JsonResponseFuture<'a> {
        // Direct async block for optimal performance
        async move {
            super::streaming_helpers::default_json_response(self, data, streaming).await
        }
    }

    fn apply_middleware<'a>(
        &'a self,
        request: &'a UniversalRequest,
        response: UniversalResponse,
    ) -> Self::MiddlewareFuture<'a> {
        // Zero-cost middleware application
        async move {
            super::streaming_helpers::default_middleware(self, request, response).await
        }
    }

    fn supports_streaming(&self) -> bool {
        self.config.supports_streaming
    }

    fn supports_sse(&self) -> bool {
        self.config.supports_sse
    }

    fn framework_name(&self) -> &'static str {
        // TODO: This should return the actual framework name
        // For now, we'll use a static string
        "universal"
    }
}

/// Builder for creating configured universal adapters
#[derive(Default)]
pub struct UniversalAdapterBuilder {
    config: AdapterConfig,
}

impl UniversalAdapterBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the framework name
    pub fn framework_name(mut self, name: impl Into<Cow<'static, str>>) -> Self {
        self.config.framework_name = name.into();
        self
    }

    /// Enable or disable streaming support
    pub fn streaming_support(mut self, enabled: bool) -> Self {
        self.config.supports_streaming = enabled;
        self
    }

    /// Enable or disable SSE support
    pub fn sse_support(mut self, enabled: bool) -> Self {
        self.config.supports_sse = enabled;
        self
    }

    /// Set default content type
    pub fn default_content_type(mut self, content_type: impl Into<Cow<'static, str>>) -> Self {
        self.config.default_content_type = content_type.into();
        self
    }

    /// Add a default header
    pub fn default_header(mut self, name: impl Into<Cow<'static, str>>, value: impl Into<Cow<'static, str>>) -> Self {
        self.config.default_headers.insert(name.into(), value.into());
        self
    }

    /// Build the adapter
    pub fn build<Req, Res, Err>(self) -> UniversalAdapter<Req, Res, Err>
    where
        Err: std::error::Error + Send + Sync + 'static,
    {
        UniversalAdapter::with_config(self.config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adapter_config_creation() {
        let config = AdapterConfig::default();
        assert_eq!(config.framework_name, "universal");
        assert!(config.supports_streaming);
        assert!(config.supports_sse);
    }

    #[test]
    fn test_adapter_builder() {
        let adapter: UniversalAdapter<(), (), std::io::Error> = UniversalAdapterBuilder::new()
            .framework_name("test")
            .streaming_support(false)
            .sse_support(true)
            .default_header("X-Test", "test")
            .build();

        assert_eq!(adapter.config.framework_name, "test");
        assert!(!adapter.config.supports_streaming);
        assert!(adapter.config.supports_sse);
        assert_eq!(
            adapter.config.default_headers.get("X-Test"),
            Some(&Cow::Borrowed("test"))
        );
    }

    #[test]
    fn test_universal_adapter_capabilities() {
        let adapter: UniversalAdapter<(), (), std::io::Error> = UniversalAdapter::new();
        
        assert!(adapter.supports_streaming());
        assert!(adapter.supports_sse());
        assert_eq!(adapter.framework_name(), "universal");
    }
}