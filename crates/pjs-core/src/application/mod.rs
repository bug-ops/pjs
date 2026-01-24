//! Application layer - Use cases and orchestration
//!
//! Implements CQRS pattern with separate command and query handlers.
//! Orchestrates domain logic and infrastructure concerns.

pub mod commands;
pub mod dto;
pub mod queries;
pub mod services;
pub mod shared;

// CQRS Handlers Module
// Status: Commented out pending GAT migration
// Location: ./handlers/ (exists but not exposed)
//
// Current Implementation: Uses async_trait for async handler traits
// Target Implementation: Zero-cost GAT-based traits matching domain ports pattern
//
// Migration Plan (Phase 2):
// 1. Define GAT-based CommandHandlerGat and QueryHandlerGat traits
// 2. Migrate SessionCommandHandler to use GAT traits
// 3. Remove async_trait dependency from handlers module
// 4. Re-export handlers from this module
//
// See: .local/implementation-plan-phase-all.md for detailed migration steps
// pub mod handlers;

pub use commands::*;
pub use queries::*;
pub use shared::AdjustmentUrgency;

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
