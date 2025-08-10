//! # SJSP Core
//! 
//! Core types and protocols for the Semantic JSON Streaming Protocol.
//! This crate provides high-performance JSON parsing with SIMD optimizations,
//! zero-copy operations, and semantic type hints for automatic optimization.

#![cfg_attr(target_arch = "x86_64", feature(stdsimd))]
#![warn(missing_docs, rust_2018_idioms)]
#![deny(unsafe_op_in_unsafe_fn)]

pub mod error;
pub mod frame;
pub mod parser;
pub mod semantic;
pub mod stream;

pub use error::{Error, Result};
pub use frame::{Frame, FrameFlags, FrameHeader};
pub use semantic::{SemanticType, SemanticMeta};
pub use parser::{Parser, ParseConfig, ParseStats};

/// Re-export commonly used types
pub mod prelude {
    pub use super::{
        Error, Result,
        Frame, FrameFlags, FrameHeader,
        SemanticType, SemanticMeta,
    };
}