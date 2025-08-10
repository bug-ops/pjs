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

/// Command execution result
#[derive(Debug, Clone)]
pub enum CommandResult<T> {
    Success(T),
    ValidationError(Vec<String>),
    BusinessRuleViolation(String),
    ConcurrencyConflict,
}

impl<T> CommandResult<T> {
    pub fn success(value: T) -> Self {
        Self::Success(value)
    }
    
    pub fn validation_error(errors: Vec<String>) -> Self {
        Self::ValidationError(errors)
    }
    
    pub fn business_rule_violation(message: String) -> Self {
        Self::BusinessRuleViolation(message)
    }
    
    pub fn concurrency_conflict() -> Self {
        Self::ConcurrencyConflict
    }
    
    pub fn into_application_result(self) -> ApplicationResult<T> {
        match self {
            Self::Success(value) => Ok(value),
            Self::ValidationError(errors) => {
                Err(ApplicationError::Validation(errors.join("; ")))
            },
            Self::BusinessRuleViolation(msg) => {
                Err(ApplicationError::Logic(msg))
            },
            Self::ConcurrencyConflict => {
                Err(ApplicationError::Concurrency("Resource was modified concurrently".to_string()))
            },
        }
    }
}

/// Query execution result
#[derive(Debug, Clone)]
pub enum QueryResult<T> {
    Success(T),
    NotFound,
    Unauthorized,
}

impl<T> QueryResult<T> {
    pub fn success(value: T) -> Self {
        Self::Success(value)
    }
    
    pub fn not_found() -> Self {
        Self::NotFound
    }
    
    pub fn unauthorized() -> Self {
        Self::Unauthorized
    }
    
    pub fn into_application_result(self) -> ApplicationResult<T> {
        match self {
            Self::Success(value) => Ok(value),
            Self::NotFound => Err(ApplicationError::NotFound("Resource not found".to_string())),
            Self::Unauthorized => Err(ApplicationError::Authorization("Unauthorized access".to_string())),
        }
    }
}