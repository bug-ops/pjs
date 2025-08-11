//! # PJS Demo
//!
//! Interactive demonstration servers and clients for the Priority JSON Streaming Protocol.
//! This crate provides ready-to-use examples showcasing PJS capabilities in real-world scenarios.

// TODO: Fix JSON macro syntax errors in data generators before re-enabling
// pub mod data;
pub mod utils;

// TODO: Re-enable data generators after fixing compilation issues  
// pub use data::*;
pub use utils::*;