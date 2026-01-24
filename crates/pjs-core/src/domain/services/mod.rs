//! Domain services implementing complex business logic
//!
//! These services orchestrate domain entities and value objects
//! to implement complex business workflows using Clean Architecture principles.

pub mod connection_manager;
pub mod gat_orchestrator;
pub mod priority_service;
pub mod validation_service;

pub use connection_manager::{ConnectionManager, ConnectionState, ConnectionStatistics};
pub use gat_orchestrator::{
    GatOrchestratorFactory, GatStreamingOrchestrator, HealthStatus, OrchestratorConfig,
};
pub use priority_service::PriorityService;
pub use validation_service::ValidationService;
