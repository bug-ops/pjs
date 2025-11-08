// Clean Universal Framework Integration Layer
//
// This module provides a unified interface for integrating PJS with any
// Rust web framework through zero-cost GAT abstractions.
//
// ARCHITECTURE: Clean and simple - no over-engineering, just what's needed.

// Core streaming adapter - contains all consolidated types
pub mod streaming_adapter;

// Framework-specific adapters
pub mod universal_adapter;

// Performance optimizations (imported by streaming_adapter)
pub mod object_pool;
pub mod simd_acceleration;

// Re-export all core types and traits from streaming_adapter
pub use streaming_adapter::{
    IntegrationError, IntegrationResult, ResponseBody, StreamingAdapter, StreamingAdapterExt,
    StreamingFormat, UniversalRequest, UniversalResponse, streaming_helpers,
};

// Re-export universal adapter types
pub use universal_adapter::{AdapterConfig, UniversalAdapter};
