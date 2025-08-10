//! Command and Query handlers implementing CQRS pattern

pub mod command_handlers;
pub mod query_handlers;

use async_trait::async_trait;
use crate::application::{ApplicationResult, ApplicationError};

/// Generic command handler trait
#[async_trait]
pub trait CommandHandler<TCommand, TResponse> {
    async fn handle(&self, command: TCommand) -> ApplicationResult<TResponse>;
}

/// Generic query handler trait  
#[async_trait]
pub trait QueryHandler<TQuery, TResponse> {
    async fn handle(&self, query: TQuery) -> ApplicationResult<TResponse>;
}

