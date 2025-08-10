//! SJSP Server implementation
//! 
//! This crate provides high-performance server functionality for SJSP protocol.

pub use sjsp_core::{Frame, SemanticType, Error, Result};

/// SJSP server (placeholder for future implementation)
pub struct SjspServer {
    // TODO: Implement server functionality
}

impl SjspServer {
    /// Create new SJSP server
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for SjspServer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_creation() {
        let _server = SjspServer::new();
    }
}