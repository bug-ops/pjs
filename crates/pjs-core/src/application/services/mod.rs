//! Application services orchestrating business workflows
//!
//! This module contains specialized services following Single Responsibility Principle.

pub mod event_service;
pub mod optimization_service;
pub mod performance_analysis_service;
pub mod prioritization_service;
pub mod stream_context;

pub use event_service::EventService;
pub use optimization_service::{OptimizationService, StreamingUseCase};
pub use performance_analysis_service::{PerformanceAnalysisReport, PerformanceAnalysisService};
pub use prioritization_service::{PerformanceContext, PrioritizationService};
pub use stream_context::{StreamConfig, StreamContext, StreamSession};
