//! Utility functions for WASM environment.
//!
//! This module provides helper functions for working with WebAssembly,
//! including panic hooks and browser console integration.

use wasm_bindgen::prelude::*;

/// Set panic hook for better error messages in browser.
///
/// When a panic occurs in WASM code, the default behavior is to show
/// a cryptic error message. This function installs a custom panic hook
/// that provides much more detailed error messages in the browser console.
///
/// This function is called automatically during module initialization.
pub fn set_panic_hook() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}

/// Log a message to the browser console.
///
/// This is a direct binding to JavaScript's `console.log` function.
///
/// # Arguments
///
/// * `s` - The string to log
///
/// # Example
///
/// ```rust,ignore
/// use crate::utils::log;
/// log("Hello from WASM!");
/// ```
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    pub fn log(s: &str);
}

/// Macro for logging to browser console.
///
/// Similar to `println!` but outputs to the browser console instead of stdout.
///
/// # Example
///
/// ```rust,ignore
/// use crate::utils::console_log;
/// console_log!("Processing {} items", count);
/// ```
#[allow(unused_macros)]
macro_rules! console_log {
    ($($t:tt)*) => (crate::utils::log(&format_args!($($t)*).to_string()))
}

#[allow(unused_imports)]
pub(crate) use console_log;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_panic_hook() {
        // Should not panic
        set_panic_hook();
    }
}
