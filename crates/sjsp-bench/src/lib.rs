//! SJSP Benchmarking suite
//! 
//! This crate provides comprehensive benchmarking for SJSP protocol.

pub use sjsp_core::{Parser, Frame, Error, Result};

/// Benchmarking utilities (placeholder for future implementation)
pub struct BenchSuite {
    // TODO: Implement benchmarking functionality
}

impl BenchSuite {
    /// Create new benchmark suite
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for BenchSuite {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bench_creation() {
        let _bench = BenchSuite::new();
    }
}