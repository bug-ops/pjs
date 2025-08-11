//! # PJS Core
//!
//! Core types and protocols for the Priority JSON Streaming Protocol.
//! This crate provides high-performance JSON parsing with SIMD optimizations,
//! zero-copy operations, and semantic type hints for automatic optimization.

#![warn(rust_2018_idioms)]
#![deny(unsafe_op_in_unsafe_fn)]
// Temporarily allow missing docs while in development
#![allow(missing_docs)]
// Allow some non-critical clippy warnings for development
#![allow(clippy::clone_on_copy)]
#![allow(clippy::derivable_impls)]
#![allow(clippy::unwrap_or_default)]
#![allow(clippy::manual_div_ceil)]
#![allow(clippy::needless_range_loop)]
#![allow(clippy::explicit_auto_deref)]
#![allow(clippy::unnecessary_map_or)]
#![allow(clippy::while_let_on_iterator)]
#![allow(clippy::let_and_return)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::new_without_default)]
#![allow(clippy::map_clone)]
#![allow(clippy::only_used_in_recursion)]
// Allow dead code for fields and methods that will be used in the future
#![allow(dead_code)]

pub mod application;
pub mod compression;
pub mod domain;
pub mod error;
pub mod frame;
pub mod infrastructure;
pub mod parser;
pub mod semantic;
pub mod stream;

// Domain layer exports
pub use domain::{
    DomainError, DomainEvent, DomainResult, Frame as DomainFrame, JsonPath, Priority, SessionId,
    Stream, StreamId, StreamSession,
};

// Application layer exports
pub use application::{
    ApplicationError, ApplicationResult, commands,
    handlers::{CommandHandler, QueryHandler},
    queries,
    services::{SessionService, StreamingService},
};

// Compression exports
pub use compression::{
    CompressedData, CompressionStrategy, SchemaAnalyzer, SchemaCompressor,
};

// Streaming exports
pub use stream::{
    CompressedFrame, CompressionStats, DecompressionMetadata, DecompressionStats,
    ProcessResult, StreamConfig, StreamFrame, StreamProcessor, StreamStats,
    StreamingCompressor, StreamingDecompressor,
};
pub use error::{Error, Result};
pub use frame::{Frame, FrameFlags, FrameHeader};
pub use parser::{ParseConfig, ParseStats, Parser};
pub use semantic::{SemanticMeta, SemanticType};
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
        CommandHandler,
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
        // JsonReconstructor,
        Priority,
        // PriorityStreamer,
        ProcessResult,
        QueryHandler,
        Result,
        SemanticMeta,
        SemanticType,
        SessionId,
        SessionService,
        Stream,
        StreamId,
        StreamProcessor,
        StreamSession,
        StreamingService,
    };
}
