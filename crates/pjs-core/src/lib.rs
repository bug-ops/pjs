//! # PJS Core
//!
//! Core types and protocols for the Priority JSON Streaming Protocol.
//! This crate provides high-performance JSON parsing with SIMD optimizations,
//! zero-copy operations, and semantic type hints for automatic optimization.

#![feature(impl_trait_in_assoc_type)]
#![warn(rust_2018_idioms)]
#![deny(unsafe_op_in_unsafe_fn)]
// Allow some non-critical clippy warnings for production code
#![allow(clippy::manual_div_ceil)]
#![allow(clippy::only_used_in_recursion)]
// Allow dead code for fields and methods that will be used in future features
#![allow(dead_code)]

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
    },
};

// Events exports
pub use domain::events::{PriorityDistribution, PriorityPercentages};

// Application layer exports (some temporarily disabled for GAT migration)
pub use application::{
    ApplicationError,
    ApplicationResult,
    commands,
    // handlers::{CommandHandler, QueryHandler}, // Disabled: migrate to GAT when implementing CQRS
    queries,
    // services::{SessionService, StreamingService}, // Disabled: replaced by SessionManager
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
// Legacy stream exports (will be deprecated)
// pub use stream::{
//     JsonPath as StreamJsonPath, JsonReconstructor, Priority as StreamPriority, PriorityStreamer,
//     ProcessResult, StreamFrame, StreamProcessor, StreamerConfig,
// };

/// Re-export commonly used types
pub mod prelude {
    pub use super::{
        ApplicationError,
        // Application layer
        ApplicationResult,
        // CommandHandler, // TODO: migrate to GAT
        DomainError,
        DomainEvent,
        DomainFrame,
        // Domain layer
        DomainResult,
        // Core types
        Error,
        Frame,
        FrameFlags,
        FrameHeader,
        JsonPath,
        // TODO: Re-add when legacy modules are reconciled
        JsonReconstructor,
        Priority,
        PriorityDistribution,
        PriorityPercentages,
        // PriorityStreamer,
        ProcessResult,
        // QueryHandler, // TODO: migrate to GAT
        Result,
        SemanticMeta,
        SemanticType,
        SessionId,
        // SessionService, // TODO: migrate to GAT
        Stream,
        StreamId,
        StreamProcessor,
        StreamSession,
        // StreamingService, // TODO: migrate to GAT
    };
}
