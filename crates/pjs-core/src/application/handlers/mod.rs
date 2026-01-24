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

// Legacy traits for backward compatibility during migration
// These use async_trait and will be deprecated after full migration
use async_trait::async_trait;

#[deprecated(since = "0.5.0", note = "Use CommandHandlerGat for zero-cost async")]
#[async_trait]
pub trait CommandHandler<TCommand, TResponse>
where
    TCommand: Send + 'static,
    TResponse: Send + 'static,
{
    async fn handle(&self, command: TCommand) -> ApplicationResult<TResponse>;
}

#[deprecated(since = "0.5.0", note = "Use QueryHandlerGat for zero-cost async")]
#[async_trait]
pub trait QueryHandler<TQuery, TResponse>
where
    TQuery: Send + 'static,
    TResponse: Send + 'static,
{
    async fn handle(&self, query: TQuery) -> ApplicationResult<TResponse>;
}
