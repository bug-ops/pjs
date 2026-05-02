//! Application layer - Use cases and orchestration
//!
//! Implements CQRS pattern with separate command and query handlers.
//! Orchestrates domain logic and infrastructure concerns.

pub mod commands;
pub mod dto;
pub mod handlers;
pub mod queries;
pub mod shared;

pub use commands::*;
pub use queries::*;
pub use shared::AdjustmentUrgency;

/// Application Result type
pub type ApplicationResult<T> = Result<T, ApplicationError>;

/// Application-specific errors
#[derive(Debug, thiserror::Error)]
pub enum ApplicationError {
    /// Wraps a domain-layer error that bubbled up to the application boundary.
    #[error("Domain error: {0}")]
    Domain(#[from] crate::domain::DomainError),

    /// Input failed validation before any domain logic ran.
    #[error("Validation error: {0}")]
    Validation(String),

    /// Caller is not authorized to perform the requested operation.
    #[error("Authorization error: {0}")]
    Authorization(String),

    /// A concurrent operation conflicted with the current request.
    #[error("Concurrency error: {0}")]
    Concurrency(String),

    /// Requested entity does not exist.
    #[error("Not found: {0}")]
    NotFound(String),

    /// Operation conflicted with the current state of the resource.
    #[error("Conflict: {0}")]
    Conflict(String),

    /// Generic application-layer logic error not covered by other variants.
    #[error("Application logic error: {0}")]
    Logic(String),
}
