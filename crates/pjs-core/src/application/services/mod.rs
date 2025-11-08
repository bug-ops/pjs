//! Application services orchestrating business workflows
//!
//! This module contains both the original monolithic services and the new
//! specialized services that follow Single Responsibility Principle.

// Original services (temporarily disabled for GAT migration)
// pub mod session_service; // TODO: migrate to GAT
// pub mod streaming_service; // TODO: migrate to GAT

// New specialized services following SRP
pub mod event_service;
pub mod optimization_service;
pub mod performance_analysis_service;
pub mod prioritization_service;
pub mod stream_context;
// pub mod stream_orchestrator; // TODO: migrate to GAT

// Re-exports for backward compatibility (temporarily disabled)
// pub use session_service::SessionService; // TODO: migrate to GAT
// pub use streaming_service::StreamingService; // TODO: migrate to GAT

// Re-exports for new architecture
pub use event_service::EventService;
pub use optimization_service::{OptimizationService, StreamingUseCase};
pub use performance_analysis_service::{PerformanceAnalysisReport, PerformanceAnalysisService};
pub use prioritization_service::{PerformanceContext, PrioritizationService};
pub use stream_context::{StreamConfig, StreamContext, StreamSession};
// pub use stream_orchestrator::{StreamOrchestrator, StreamOrchestratorFactory}; // TODO: migrate to GAT
