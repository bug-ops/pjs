//! Priority constants exported to JavaScript
//!
//! This module exports predefined priority constants to JavaScript,
//! making it easier to use common priority levels without memorizing
//! numeric values.

use pjs_domain::value_objects::Priority;
use wasm_bindgen::prelude::*;

/// Priority constants for JavaScript.
///
/// These constants provide easy access to commonly used priority levels
/// without needing to know the numeric values.
///
/// # Example
///
/// ```javascript
/// import { PjsParser, PriorityConstants } from 'pjs-wasm';
///
/// const parser = new PjsParser();
/// const frames = parser.generateFrames(
///   '{"id": 1, "name": "Alice"}',
///   PriorityConstants.MEDIUM
/// );
/// ```
#[wasm_bindgen]
pub struct PriorityConstants;

#[wasm_bindgen]
impl PriorityConstants {
    /// Critical priority (100) - for essential data (IDs, status, core metadata)
    ///
    /// # Example
    ///
    /// ```javascript
    /// const frames = parser.generateFrames(json, PriorityConstants.CRITICAL);
    /// ```
    #[wasm_bindgen(getter)]
    #[allow(non_snake_case)]
    pub fn CRITICAL() -> u8 {
        Priority::CRITICAL.value()
    }

    /// High priority (80) - for important visible data (names, titles)
    ///
    /// # Example
    ///
    /// ```javascript
    /// const frames = parser.generateFrames(json, PriorityConstants.HIGH);
    /// ```
    #[wasm_bindgen(getter)]
    #[allow(non_snake_case)]
    pub fn HIGH() -> u8 {
        Priority::HIGH.value()
    }

    /// Medium priority (50) - for regular content
    ///
    /// # Example
    ///
    /// ```javascript
    /// const frames = parser.generateFrames(json, PriorityConstants.MEDIUM);
    /// ```
    #[wasm_bindgen(getter)]
    #[allow(non_snake_case)]
    pub fn MEDIUM() -> u8 {
        Priority::MEDIUM.value()
    }

    /// Low priority (25) - for supplementary data
    ///
    /// # Example
    ///
    /// ```javascript
    /// const frames = parser.generateFrames(json, PriorityConstants.LOW);
    /// ```
    #[wasm_bindgen(getter)]
    #[allow(non_snake_case)]
    pub fn LOW() -> u8 {
        Priority::LOW.value()
    }

    /// Background priority (10) - for analytics, logs, etc.
    ///
    /// # Example
    ///
    /// ```javascript
    /// const frames = parser.generateFrames(json, PriorityConstants.BACKGROUND);
    /// ```
    #[wasm_bindgen(getter)]
    #[allow(non_snake_case)]
    pub fn BACKGROUND() -> u8 {
        Priority::BACKGROUND.value()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_constants_values() {
        assert_eq!(PriorityConstants::CRITICAL(), 100);
        assert_eq!(PriorityConstants::HIGH(), 80);
        assert_eq!(PriorityConstants::MEDIUM(), 50);
        assert_eq!(PriorityConstants::LOW(), 25);
        assert_eq!(PriorityConstants::BACKGROUND(), 10);
    }

    #[test]
    fn test_priority_ordering() {
        assert!(PriorityConstants::CRITICAL() > PriorityConstants::HIGH());
        assert!(PriorityConstants::HIGH() > PriorityConstants::MEDIUM());
        assert!(PriorityConstants::MEDIUM() > PriorityConstants::LOW());
        assert!(PriorityConstants::LOW() > PriorityConstants::BACKGROUND());
    }
}
