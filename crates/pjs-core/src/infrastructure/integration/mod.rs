//! Clean universal framework-integration layer.
//!
//! Provides a unified interface for integrating PJS with any Rust web
//! framework through zero-cost GAT abstractions.

/// Core streaming adapter — contains all consolidated framework-integration types.
pub mod streaming_adapter;

/// Framework-specific universal adapter implementation.
pub mod universal_adapter;

/// Object pooling utilities used by the streaming adapter.
pub mod object_pool;
/// SIMD-accelerated frame processing helpers used by the streaming adapter.
pub mod simd_acceleration;

// Re-export all core types and traits from streaming_adapter
pub use streaming_adapter::{
    IntegrationError, IntegrationResult, ResponseBody, StreamingAdapter, StreamingAdapterExt,
    StreamingFormat, UniversalRequest, UniversalResponse, streaming_helpers,
};

// Re-export universal adapter types
pub use universal_adapter::{AdapterConfig, UniversalAdapter};
