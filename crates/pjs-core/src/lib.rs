//! # PJS Core
//!
//! Core types and protocols for the Priority JSON Streaming Protocol.
//! This crate provides high-performance JSON parsing with SIMD optimizations,
//! zero-copy operations, and semantic type hints for automatic optimization.

#![feature(impl_trait_in_assoc_type)]
#![warn(rust_2018_idioms)]
#![deny(unsafe_op_in_unsafe_fn)]
// Allow specific clippy warnings that are intentional design choices
#![allow(clippy::manual_div_ceil)] // Performance: manual div_ceil is faster
#![allow(clippy::only_used_in_recursion)] // Recursive algorithms by design
// Note: dead_code is now handled per-item with targeted annotations

// Allocator FFI dependencies
#[cfg(feature = "mimalloc")]
extern crate libmimalloc_sys;
#[cfg(feature = "jemalloc")]
extern crate tikv_jemalloc_sys;

pub mod application;
pub mod compression;
pub mod config;
pub mod domain;
pub mod error;
pub mod frame;
pub mod infrastructure;
pub mod memory;
pub mod parser;
pub mod security;
pub mod semantic;
pub mod stream;

// Domain layer exports
pub use domain::{
    DomainError,
    DomainEvent,
    DomainResult,
    Frame as DomainFrame,
    JsonPath,
    Priority,
    SessionId,
    Stream,
    StreamId,
    StreamSession,
    // GAT-based domain ports (zero-cost async abstractions)
    ports::{
        EventPublisherGat, FrameSinkGat, FrameSinkGatExt, FrameSourceGat, StreamRepositoryGat,
        StreamStoreGat,
    },
    services::{
        GatOrchestratorFactory, GatStreamingOrchestrator, HealthStatus, OrchestratorConfig,
        ValidationService,
    },
    value_objects::{
        JsonData, Schema, SchemaId, SchemaType, SchemaValidationError, SchemaValidationResult,
    },
};

// Events exports
pub use domain::events::{PriorityDistribution, PriorityPercentages};

// Application layer exports
pub use application::{
    ApplicationError, ApplicationResult, commands,
    dto::{
        SchemaDefinitionDto, SchemaMetadataDto, SchemaRegistrationDto, ValidationErrorDto,
        ValidationRequestDto, ValidationResultDto,
    },
    queries,
};

// Configuration exports
pub use config::{
    ParserConfig, PjsConfig, SecurityConfig, SimdConfig, StreamingConfig,
    security::{BufferLimits, JsonLimits, NetworkLimits, RateLimitingConfig, SessionLimits},
};

// Compression exports
pub use compression::{
    CompressedData, CompressionConfig, CompressionStrategy, SchemaAnalyzer, SchemaCompressor,
    secure::{
        DecompressionContextStats, SecureCompressedData, SecureCompressor,
        SecureDecompressionContext,
    },
};

// Streaming exports
pub use error::{Error, Result};
pub use frame::{Frame, FrameFlags, FrameHeader};
#[cfg(any(feature = "websocket-client", feature = "websocket-server"))]
pub use infrastructure::websocket::SecureWebSocketHandler;
pub use memory::{ArenaJsonParser, CombinedArenaStats, JsonArena};
pub use parser::{
    LazyParser, ParseConfig, ParseStats, Parser, SimpleParser, SonicParser, ZeroCopyParser,
};
pub use security::{
    CompressionBombConfig, CompressionBombDetector, CompressionBombProtector,
    CompressionStats as BombCompressionStats, DepthTracker, RateLimitConfig, RateLimitError,
    RateLimitGuard, RateLimitStats, SecurityValidator, WebSocketRateLimiter,
};
pub use semantic::{SemanticMeta, SemanticType};
pub use stream::{
    CompressedFrame, CompressionStats, DecompressionMetadata, DecompressionStats,
    JsonReconstructor, PriorityStreamer, ProcessResult, StreamConfig, StreamFrame, StreamProcessor,
    StreamStats, StreamingCompressor, StreamingDecompressor,
};

/// Re-export commonly used types
pub mod prelude {
    pub use super::{
        ApplicationError, ApplicationResult, DomainError, DomainEvent, DomainFrame, DomainResult,
        Error, Frame, FrameFlags, FrameHeader, JsonData, JsonPath, JsonReconstructor, Priority,
        PriorityDistribution, PriorityPercentages, ProcessResult, Result, Schema, SchemaId,
        SchemaRepository, SchemaType, SchemaValidationError, SemanticMeta, SemanticType, SessionId,
        Stream, StreamId, StreamProcessor, StreamSession, ValidationService,
    };
}

// Infrastructure exports for schema validation
pub use infrastructure::SchemaRepository;
