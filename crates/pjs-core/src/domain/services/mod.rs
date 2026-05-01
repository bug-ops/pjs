//! Domain services implementing complex business logic
//!
//! These services orchestrate domain entities and value objects
//! to implement complex business workflows using Clean Architecture principles.

pub mod gat_orchestrator;
pub mod validation_service;

pub use gat_orchestrator::{
    GatOrchestratorFactory, GatStreamingOrchestrator, HealthStatus, OrchestratorConfig,
};
pub use validation_service::ValidationService;
