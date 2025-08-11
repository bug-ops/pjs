//! # PJS Demo
//!
//! Interactive demonstration servers and clients for the Priority JSON Streaming Protocol.
//! This crate provides ready-to-use examples showcasing PJS capabilities in real-world scenarios.

// Re-enabling data module to fix compilation errors - will fix JSON macro issues
pub mod data;
pub mod utils;

pub use data::*;
pub use utils::*;