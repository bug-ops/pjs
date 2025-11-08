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
//! - Zero-copy JSON parsing where possible
//! - Priority-based streaming support
//! - Schema validation
//! - Optimized for minimal WASM bundle size
//!
//! # Example
//!
//! ```javascript
//! import { PjsParser } from 'pjs-wasm';
//!
//! const parser = new PjsParser();
//! const result = parser.parse('{"name": "test", "value": 42}');
//! console.log(result);
//! ```

use wasm_bindgen::prelude::*;

mod parser;
mod utils;

pub use parser::PjsParser;

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
