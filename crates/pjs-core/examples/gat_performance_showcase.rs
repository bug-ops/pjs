// GAT Performance Showcase
//
// This example demonstrates the performance benefits of the new GAT-based
// StreamingAdapter with true zero-cost abstractions.

#![feature(impl_trait_in_assoc_type)]

use pjson_rs::domain::value_objects::{JsonData, SessionId};
use pjson_rs::infrastructure::integration::{
    IntegrationResult, StreamingAdapter, StreamingAdapterExt, StreamingFormat, UniversalRequest,
    UniversalResponse, streaming_helpers,
};
use pjson_rs::stream::StreamFrame;
use std::future::Future;
use std::hint::black_box;
use std::time::Instant;

/// Modern GAT adapter with true zero-cost abstractions
struct ModernGatAdapter;

impl StreamingAdapter for ModernGatAdapter {
    type Request = String;
    type Response = String;
    type Error = pjson_rs::infrastructure::integration::IntegrationError;

    // TRUE zero-cost GAT futures with impl Trait - no Box allocation!
    type StreamingResponseFuture<'a>
        = impl Future<Output = IntegrationResult<Self::Response>> + Send + 'a
    where
        Self: 'a;

    type SseResponseFuture<'a>
        = impl Future<Output = IntegrationResult<Self::Response>> + Send + 'a
    where
        Self: 'a;

    type JsonResponseFuture<'a>
        = impl Future<Output = IntegrationResult<Self::Response>> + Send + 'a
    where
        Self: 'a;

    type MiddlewareFuture<'a>
        = impl Future<Output = IntegrationResult<UniversalResponse>> + Send + 'a
    where
        Self: 'a;

    fn convert_request(&self, _request: Self::Request) -> IntegrationResult<UniversalRequest> {
        Ok(UniversalRequest::new("GET", "/test"))
    }

    fn to_response(&self, _response: UniversalResponse) -> IntegrationResult<Self::Response> {
        Ok("gat response".to_string())
    }

    fn create_streaming_response<'a>(
        &'a self,
        _session_id: SessionId,
        _frames: Vec<StreamFrame>,
        _format: StreamingFormat,
    ) -> Self::StreamingResponseFuture<'a> {
        // Direct async block - compiler generates optimal Future type
        async move {
            // Zero-cost Future type (no Box<dyn Future> allocation)
            Ok("zero-cost gat streaming".to_string())
        }
    }

    fn create_sse_response<'a>(
        &'a self,
        session_id: SessionId,
        frames: Vec<StreamFrame>,
    ) -> Self::SseResponseFuture<'a> {
        // Direct async delegation - zero-cost Future type, no Box<dyn Future>
        async move { streaming_helpers::default_sse_response(self, session_id, frames).await }
    }

    fn create_json_response<'a>(
        &'a self,
        data: JsonData,
        streaming: bool,
    ) -> Self::JsonResponseFuture<'a> {
        // Stack-allocated Future type - no Box<dyn Future>
        async move { streaming_helpers::default_json_response(self, data, streaming).await }
    }

    fn apply_middleware<'a>(
        &'a self,
        request: &'a UniversalRequest,
        response: UniversalResponse,
    ) -> Self::MiddlewareFuture<'a> {
        // Zero-cost middleware - compile-time optimized
        async move { streaming_helpers::default_middleware(self, request, response).await }
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_sse(&self) -> bool {
        true
    }

    fn framework_name(&self) -> &'static str {
        "modern_gat"
    }
}

impl StreamingAdapterExt for ModernGatAdapter {
    // Extension futures also use zero-cost impl Trait
    type AutoStreamFuture<'a>
        = impl Future<Output = IntegrationResult<Self::Response>> + Send + 'a
    where
        Self: 'a;

    type ErrorResponseFuture<'a>
        = impl Future<Output = IntegrationResult<Self::Response>> + Send + 'a
    where
        Self: 'a;

    type HealthResponseFuture<'a>
        = impl Future<Output = IntegrationResult<Self::Response>> + Send + 'a
    where
        Self: 'a;

    fn auto_stream_response<'a>(
        &'a self,
        request: &'a UniversalRequest,
        session_id: SessionId,
        frames: Vec<StreamFrame>,
    ) -> Self::AutoStreamFuture<'a> {
        // Zero-cost auto detection
        async move {
            streaming_helpers::default_auto_stream_response(self, request, session_id, frames).await
        }
    }

    fn create_error_response<'a>(
        &'a self,
        status: u16,
        message: String,
    ) -> Self::ErrorResponseFuture<'a> {
        // Stack-allocated error handling
        async move { streaming_helpers::default_error_response(self, status, message).await }
    }

    fn create_health_response<'a>(&'a self) -> Self::HealthResponseFuture<'a> {
        async move { streaming_helpers::default_health_response(self).await }
    }
}

#[tokio::main]
async fn main() {
    println!("GAT Performance Showcase");
    println!("========================\n");

    benchmark_memory_allocation().await;
    showcase_static_dispatch();
}

async fn benchmark_memory_allocation() {
    println!("Memory Allocation Benchmark");
    println!("---------------------------");

    const ITERATIONS: usize = 10_000;

    let gat_adapter = ModernGatAdapter;
    let data = JsonData::String("performance test".to_string());

    let start = Instant::now();
    for _ in 0..ITERATIONS {
        let result = gat_adapter.create_json_response(data.clone(), false).await;
        black_box(&result);
    }
    let duration = start.elapsed();

    println!("GAT JSON response creation: {:?}", duration);
    println!("  - Static dispatch eliminates virtual calls\n");
}

fn showcase_static_dispatch() {
    println!("Zero-Cost GAT Benefits");
    println!("----------------------");

    println!("Modern Zero-Cost GATs characteristics:");
    println!("  - TRUE zero-cost abstractions with impl Trait");
    println!("  - Compile-time Future type generation");
    println!("  - Zero-cost Future types - no Box<dyn Future> allocation");
    println!("  - Complete inlining for hot paths");
    println!("  - Static dispatch eliminates vtables");

    println!("\nPerformance Benefits with nightly:");
    println!("  - No Box<dyn Future> allocations");
    println!("  - Optimal CPU cache utilization");
    println!("  - Aggressive compile-time optimizations");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_gat_adapter() {
        let adapter = ModernGatAdapter;
        let session_id = SessionId::new();
        let frames = vec![];

        let result = adapter
            .create_streaming_response(session_id, frames, StreamingFormat::Json)
            .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "zero-cost gat streaming");
    }

    #[tokio::test]
    async fn test_gat_extension_methods() {
        let adapter = ModernGatAdapter;

        let result = adapter.create_health_response().await;
        assert!(result.is_ok());
    }
}
