//! Domain layer - Pure business logic
//!
//! Contains entities, value objects, aggregates, domain services
//! and domain events. No dependencies on infrastructure concerns.

pub mod aggregates;
pub mod entities;
pub mod events;
pub mod ports;
pub mod services;
pub mod value_objects;

// Re-export core domain types
pub use aggregates::StreamSession;
pub use entities::{Frame, Stream};
pub use events::DomainEvent;
pub use ports::{FrameSink, FrameSource, StreamRepository};
pub use services::PriorityService;
pub use value_objects::{JsonPath, Priority, SessionId, StreamId};

/// Domain Result type
pub type DomainResult<T> = Result<T, DomainError>;

/// Domain-specific errors
#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    #[error("Invalid state transition: {0}")]
    InvalidStateTransition(String),

    #[error("Invalid stream state: {0}")]
    InvalidStreamState(String),

    #[error("Invalid session state: {0}")]
    InvalidSessionState(String),

    #[error("Invalid frame: {0}")]
    InvalidFrame(String),

    #[error("Stream invariant violation: {0}")]
    InvariantViolation(String),

    #[error("Invalid priority value: {0}")]
    InvalidPriority(String),

    #[error("Invalid JSON path: {0}")]
    InvalidPath(String),

    #[error("Session not found: {0}")]
    SessionNotFound(String),

    #[error("Stream not found: {0}")]
    StreamNotFound(String),

    #[error("Too many streams: {0}")]
    TooManyStreams(String),

    #[error("Domain logic error: {0}")]
    Logic(String),

    #[error("Compression error: {0}")]
    CompressionError(String),

    #[error("Validation error: {0}")]
    ValidationError(String),
}

impl DomainError {
    pub fn invariant_violation(message: impl Into<String>) -> Self {
        Self::InvariantViolation(message.into())
    }

    pub fn invalid_transition(from: &str, to: &str) -> Self {
        Self::InvalidStateTransition(format!("{} -> {}", from, to))
    }
}

impl From<String> for DomainError {
    fn from(error: String) -> Self {
        Self::Logic(error)
    }
}
