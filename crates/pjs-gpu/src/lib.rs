//! PJS GPU acceleration
//!
//! This crate provides GPU acceleration for PJS protocol (future implementation).

pub use pjson_rs::{Error, Frame, Result};

/// GPU accelerator (placeholder for future implementation)
pub struct GpuAccelerator {
    // TODO: Implement GPU acceleration
}

impl GpuAccelerator {
    /// Create new GPU accelerator
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for GpuAccelerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gpu_creation() {
        let _gpu = GpuAccelerator::new();
    }
}
