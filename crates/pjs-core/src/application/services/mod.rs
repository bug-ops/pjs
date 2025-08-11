//! Application services orchestrating business workflows
//!
//! This module contains both the original monolithic services and the new
//! specialized services that follow Single Responsibility Principle.

// Original services (deprecated in favor of specialized services)
pub mod session_service;
pub mod streaming_service;

// New specialized services following SRP
pub mod prioritization_service;
pub mod performance_analysis_service;  
pub mod optimization_service;
pub mod stream_orchestrator;

// Re-exports for backward compatibility
pub use session_service::SessionService;
pub use streaming_service::StreamingService;

// Re-exports for new architecture
pub use prioritization_service::{PrioritizationService, PerformanceContext};
pub use performance_analysis_service::{PerformanceAnalysisService, PerformanceAnalysisReport};
pub use optimization_service::{OptimizationService, StreamingUseCase};
pub use stream_orchestrator::{StreamOrchestrator, StreamOrchestratorFactory};
