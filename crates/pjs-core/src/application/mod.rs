//! Application layer - Use cases and orchestration
//!
//! Implements CQRS pattern with separate command and query handlers.
//! Orchestrates domain logic and infrastructure concerns.

pub mod commands;
pub mod dto;
pub mod handlers;
pub mod queries;
pub mod services;

pub use commands::*;
pub use handlers::{CommandHandler, QueryHandler};
pub use queries::*;

/// Application Result type
pub type ApplicationResult<T> = Result<T, ApplicationError>;

/// Application-specific errors
#[derive(Debug, thiserror::Error)]
pub enum ApplicationError {
    #[error("Domain error: {0}")]
    Domain(#[from] crate::domain::DomainError),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Authorization error: {0}")]
    Authorization(String),

    #[error("Concurrency error: {0}")]
    Concurrency(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Application logic error: {0}")]
    Logic(String),
}
