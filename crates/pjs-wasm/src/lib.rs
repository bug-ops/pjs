//! PJS WebAssembly Bindings
//!
//! Provides WASM-compatible interface for PJS domain logic.
//!
//! # Overview
//!
//! This crate provides WebAssembly bindings for the PJS (Priority JSON Streaming)
//! protocol. It exposes the core domain logic through a JavaScript-friendly API
//! that can be used in browsers and Node.js environments.
//!
//! # Features
//!
//! - **PriorityStream API**: Callback-based streaming with progressive frame delivery
//! - **SecurityConfig**: Built-in DoS protection (size limits, depth limits)
//! - **Zero-copy JSON parsing** where possible
//! - **Priority-based streaming** with semantic field prioritization
//! - **Schema validation** support
//! - **Optimized bundle size**: ~70KB gzipped
//!
//! # Example: PriorityStream API (Recommended)
//!
//! ```javascript
//! import init, { PriorityStream, PriorityConstants } from 'pjs-wasm';
//!
//! await init();
//!
//! const stream = new PriorityStream();
//! stream.setMinPriority(PriorityConstants.MEDIUM());
//!
//! stream.onFrame((frame) => {
//!     console.log(`${frame.type} [${frame.priority}]: ${frame.payload}`);
//! });
//!
//! stream.onComplete((stats) => {
//!     console.log(`Completed: ${stats.totalFrames} frames`);
//! });
//!
//! stream.start(JSON.stringify({ id: 123, name: "Alice" }));
//! ```
//!
//! # Example: Simple Parser API
//!
//! ```javascript
//! import init, { PjsParser, PriorityConstants } from 'pjs-wasm';
//!
//! await init();
//! const parser = new PjsParser();
//! const frames = parser.generateFrames(
//!     JSON.stringify({ name: "test", value: 42 }),
//!     PriorityConstants.MEDIUM()
//! );
//! frames.forEach(frame => console.log(frame.priority, frame.data));
//! ```
//!
//! # Security Configuration
//!
//! ```javascript
//! import { PriorityStream, SecurityConfig } from 'pjs-wasm';
//!
//! const security = new SecurityConfig()
//!     .setMaxJsonSize(5 * 1024 * 1024)  // 5 MB limit
//!     .setMaxDepth(32);                  // 32 levels max
//!
//! const stream = PriorityStream.withSecurityConfig(security);
//! ```

use wasm_bindgen::prelude::*;

mod parser;
mod priority_assignment;
mod priority_config;
mod priority_constants;
pub mod security;
mod streaming;
mod utils;

pub use parser::PjsParser;
pub use priority_config::PriorityConfigBuilder;
pub use priority_constants::PriorityConstants;
pub use security::SecurityConfig;
pub use streaming::{FrameData, PriorityStream, StreamStats};

/// Initialize WASM module.
///
/// This function sets up panic hooks for better error messages in the browser
/// and performs any other necessary initialization.
///
/// # Automatic Initialization
///
/// This function is marked with `#[wasm_bindgen(start)]` which means it will
/// be called automatically when the WASM module is loaded.
#[wasm_bindgen(start)]
pub fn init() {
    utils::set_panic_hook();
}

/// Get the version of the PJS WASM bindings.
///
/// # Returns
///
/// The version string from Cargo.toml.
///
/// # Example
///
/// ```javascript
/// import { version } from 'pjs-wasm';
/// console.log(`PJS WASM version: ${version()}`);
/// ```
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}
