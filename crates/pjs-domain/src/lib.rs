//! PJS Domain Layer - Pure Business Logic
//!
//! This crate contains the pure domain logic for PJS (Priority JSON Streaming Protocol)
//! with ZERO external dependencies (except thiserror for error handling).
//!
//! The domain layer is WASM-compatible and can be used in both native and
//! WebAssembly environments.
//!
//! ## Architecture
//!
//! Following Clean Architecture principles:
//! - **Value Objects**: Immutable, validated domain concepts (Priority, JsonPath, etc.)
//! - **Entities**: Domain objects with identity (Frame, Stream)
//! - **Domain Events**: State change notifications
//!
//! ## Features
//!
//! - `std` (default): Standard library support
//! - `serde`: Serialization support for WASM interop
//! - `wasm`: Enables WASM-specific optimizations

#![cfg_attr(not(feature = "std"), no_std)]
#![warn(missing_docs)]

#[cfg(not(feature = "std"))]
extern crate alloc;

#[cfg(not(feature = "std"))]
use alloc::{format, string::String, vec::Vec};

pub mod entities;
pub mod events;
pub mod value_objects;

// Re-export core types
pub use entities::{Frame, Stream};
pub use events::{DomainEvent, SessionState};
pub use value_objects::{JsonData, JsonPath, Priority, Schema, SessionId, StreamId};

/// Domain Result type
pub type DomainResult<T> = Result<T, DomainError>;

/// Domain-specific errors
///
/// All domain errors are value types with no external dependencies.
/// Uses thiserror for ergonomic error handling.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DomainError {
    /// Invalid state transition attempted
    #[error("Invalid state transition: {0}")]
    InvalidStateTransition(String),

    /// Invalid stream state
    #[error("Invalid stream state: {0}")]
    InvalidStreamState(String),

    /// Invalid session state
    #[error("Invalid session state: {0}")]
    InvalidSessionState(String),

    /// Invalid frame structure or content
    #[error("Invalid frame: {0}")]
    InvalidFrame(String),

    /// Stream invariant violation
    #[error("Stream invariant violation: {0}")]
    InvariantViolation(String),

    /// Invalid priority value (must be 1-255)
    #[error("Invalid priority value: {0}")]
    InvalidPriority(String),

    /// Invalid JSON path format
    #[error("Invalid JSON path: {0}")]
    InvalidPath(String),

    /// Session not found
    #[error("Session not found: {0}")]
    SessionNotFound(String),

    /// Stream not found
    #[error("Stream not found: {0}")]
    StreamNotFound(String),

    /// Too many concurrent streams
    #[error("Too many streams: {0}")]
    TooManyStreams(String),

    /// General domain logic error
    #[error("Domain logic error: {0}")]
    Logic(String),

    /// I/O operation failed
    #[error("I/O error: {0}")]
    Io(String),

    /// Resource not found
    #[error("Resource not found: {0}")]
    NotFound(String),

    /// Concurrency conflict detected
    #[error("Concurrency conflict: {0}")]
    ConcurrencyConflict(String),

    /// Compression operation failed
    #[error("Compression error: {0}")]
    CompressionError(String),

    /// Validation failed
    #[error("Validation error: {0}")]
    ValidationError(String),

    /// Invalid input provided
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// Internal error (should not happen)
    #[error("Internal error: {0}")]
    InternalError(String),

    /// Security policy violation
    #[error("Security violation: {0}")]
    SecurityViolation(String),

    /// Resource exhausted (memory, connections, etc.)
    #[error("Resource exhausted: {0}")]
    ResourceExhausted(String),
}

impl DomainError {
    /// Create an invariant violation error
    pub fn invariant_violation(message: impl Into<String>) -> Self {
        Self::InvariantViolation(message.into())
    }

    /// Create an invalid state transition error
    pub fn invalid_transition(from: &str, to: &str) -> Self {
        Self::InvalidStateTransition(format!("{from} -> {to}"))
    }
}

impl From<String> for DomainError {
    fn from(error: String) -> Self {
        Self::Logic(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_domain_error_creation() {
        let err = DomainError::invariant_violation("test");
        assert!(matches!(err, DomainError::InvariantViolation(_)));

        let err = DomainError::invalid_transition("StateA", "StateB");
        assert!(matches!(err, DomainError::InvalidStateTransition(_)));
    }

    #[test]
    fn test_domain_result() {
        let result: DomainResult<u32> = Ok(42);
        assert!(result.is_ok());

        let result: DomainResult<u32> = Err(DomainError::Logic("test".to_string()));
        assert!(result.is_err());
    }
}
