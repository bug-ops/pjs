//! Command and Query handlers implementing CQRS pattern with GAT-based traits
//!
//! Uses Generic Associated Types for zero-cost async abstractions,
//! matching the pattern used in domain ports.

pub mod command_handlers;
pub mod query_handlers;

use crate::application::ApplicationResult;
use std::future::Future;

/// GAT-based command handler trait for zero-cost async
///
/// This trait uses Generic Associated Types instead of async_trait
/// to avoid the boxing overhead of dynamic dispatch.
pub trait CommandHandlerGat<TCommand> {
    /// The response type for this command
    type Response;

    /// Future type returned by handle method
    type HandleFuture<'a>: Future<Output = ApplicationResult<Self::Response>> + Send + 'a
    where
        Self: 'a;

    /// Handle the command and return a future
    fn handle(&self, command: TCommand) -> Self::HandleFuture<'_>;
}

/// GAT-based query handler trait for zero-cost async
///
/// This trait uses Generic Associated Types instead of async_trait
/// to avoid the boxing overhead of dynamic dispatch.
pub trait QueryHandlerGat<TQuery> {
    /// The response type for this query
    type Response;

    /// Future type returned by handle method
    type HandleFuture<'a>: Future<Output = ApplicationResult<Self::Response>> + Send + 'a
    where
        Self: 'a;

    /// Handle the query and return a future
    fn handle(&self, query: TQuery) -> Self::HandleFuture<'_>;
}

// Legacy async_trait handlers removed - use GAT versions instead
