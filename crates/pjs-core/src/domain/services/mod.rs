//! Domain services implementing complex business logic
//!
//! These services orchestrate domain entities and value objects
//! to implement complex business workflows using Clean Architecture principles.

pub mod connection_manager;
pub mod priority_service;
// pub mod streaming_orchestrator; // TODO: migrate to GAT
pub mod gat_orchestrator;

pub use connection_manager::{ConnectionManager, ConnectionState, ConnectionStatistics};
pub use priority_service::PriorityService;
// pub use streaming_orchestrator::{
//     StreamingOrchestrator, StreamingOrchestratorFactory,
//     StreamingStats, StreamingPerformanceMetrics
// }; // TODO: migrate to GAT
pub use gat_orchestrator::{
    GatStreamingOrchestrator, GatOrchestratorFactory,
    OrchestratorConfig, HealthStatus
};
