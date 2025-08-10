//! SJSP Transport layer
//! 
//! This crate provides transport layer functionality for SJSP protocol.

pub use sjsp_core::{Frame, Error, Result};

/// Transport layer (placeholder for future implementation)
pub struct Transport {
    // TODO: Implement transport functionality
}

impl Transport {
    /// Create new transport
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for Transport {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transport_creation() {
        let _transport = Transport::new();
    }
}