//! SJSP Client implementation
//! 
//! This crate provides high-performance client functionality for SJSP protocol.

pub use sjsp_core::{Frame, SemanticType, Error, Result};

/// SJSP client (placeholder for future implementation)
pub struct SjspClient {
    // TODO: Implement client functionality
}

impl SjspClient {
    /// Create new SJSP client
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