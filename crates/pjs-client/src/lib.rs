//! PJS Client implementation
//!
//! This crate provides high-performance client functionality for PJS protocol.

pub use pjson_rs::{Error, Frame, Result, SemanticType};

/// PJS client (placeholder for future implementation)
pub struct SjspClient {
    // TODO: Implement client functionality
}

impl SjspClient {
    /// Create new PJS client
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for SjspClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let _client = SjspClient::new();
    }
}
