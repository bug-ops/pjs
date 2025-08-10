//! Error types for SJSP operations

use std::fmt;

/// Result type alias for SJSP operations
pub type Result<T> = std::result::Result<T, Error>;

/// Main error type for SJSP operations
#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    /// Invalid JSON syntax
    #[error("Invalid JSON syntax at position {position}: {message}")]
    InvalidJson {
        /// Position in the input where error occurred
        position: usize,
        /// Error description
        message: String,
    },

    /// Frame format error
    #[error("Invalid frame format: {0}")]
    InvalidFrame(String),

    /// Schema validation error
    #[error("Schema validation failed: {0}")]
    SchemaValidation(String),

    /// Semantic type mismatch
    #[error("Semantic type mismatch: expected {expected}, got {actual}")]
    SemanticTypeMismatch {
        /// Expected semantic type
        expected: String,
        /// Actual semantic type
        actual: String,
    },

    /// Buffer overflow or underflow
    #[error("Buffer error: {0}")]
    Buffer(String),

    /// Memory allocation error
    #[error("Memory allocation failed: {0}")]
    Memory(String),

    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// UTF-8 conversion error
    #[error("UTF-8 conversion failed: {0}")]
    Utf8(#[from] std::str::Utf8Error),

    /// Generic error for other cases
    #[error("{0}")]
    Other(String),
}

impl Error {
    /// Create an invalid JSON error
    pub fn invalid_json(position: usize, message: impl Into<String>) -> Self {
        Self::InvalidJson {
            position,
            message: message.into(),
        }
    }

    /// Create an invalid frame error
    pub fn invalid_frame(message: impl Into<String>) -> Self {
        Self::InvalidFrame(message.into())
    }

    /// Create a buffer error
    pub fn buffer(message: impl Into<String>) -> Self {
        Self::Buffer(message.into())
    }

    /// Create a memory error
    pub fn memory(message: impl Into<String>) -> Self {
        Self::Memory(message.into())
    }

    /// Create a generic error
    pub fn other(message: impl Into<String>) -> Self {
        Self::Other(message.into())
    }
}