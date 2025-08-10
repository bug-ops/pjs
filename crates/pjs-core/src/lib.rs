//! # PJS Core
//! 
//! Core types and protocols for the Priority JSON Streaming Protocol.
//! This crate provides high-performance JSON parsing with SIMD optimizations,
//! zero-copy operations, and semantic type hints for automatic optimization.

#![cfg_attr(target_arch = "x86_64", feature(stdsimd))]
#![warn(missing_docs, rust_2018_idioms)]
#![deny(unsafe_op_in_unsafe_fn)]

pub mod application;
pub mod domain;
pub mod error;
pub mod frame;
pub mod parser;
pub mod semantic;
pub mod stream;

// Domain layer exports
pub use domain::{
    DomainResult, DomainError,
    SessionId, StreamId, JsonPath, Priority,
    Stream, Frame as DomainFrame, StreamSession, DomainEvent,
};

// Application layer exports  
pub use application::{
    ApplicationResult, ApplicationError,
    commands, queries,
    handlers::{CommandHandler, QueryHandler},
    services::{SessionService, StreamingService},
};
pub use error::{Error, Result};
pub use frame::{Frame, FrameFlags, FrameHeader};
pub use semantic::{SemanticType, SemanticMeta};
pub use parser::{Parser, ParseConfig, ParseStats};
pub use stream::{StreamProcessor, PriorityStreamer, StreamerConfig, Priority as StreamPriority, JsonPath as StreamJsonPath, StreamFrame, JsonReconstructor, ProcessResult};

/// Re-export commonly used types
pub mod prelude {
    pub use super::{
        // Domain layer
        DomainResult, DomainError,
        SessionId, StreamId, JsonPath, Priority,
        Stream, DomainFrame, StreamSession, DomainEvent,
        // Application layer
        ApplicationResult, ApplicationError,
        CommandHandler, QueryHandler,
        SessionService, StreamingService,
        // Core types
        Error, Result,
        Frame, FrameFlags, FrameHeader,
        SemanticType, SemanticMeta,
        StreamProcessor, PriorityStreamer,
        JsonReconstructor, ProcessResult,
    };
}