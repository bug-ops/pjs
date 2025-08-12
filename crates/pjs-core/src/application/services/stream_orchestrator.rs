//! Stream orchestrator that coordinates specialized services
//!
//! This orchestrator replaces the monolithic StreamingService by coordinating
//! multiple specialized services, each with a single responsibility.

use crate::{
    application::{
        ApplicationResult,
        commands::*,
        handlers::CommandHandler,
        services::{
            prioritization_service::{PrioritizationService, PerformanceContext},
            performance_analysis_service::PerformanceAnalysisService,
            optimization_service::{OptimizationService, StreamingUseCase},
        },
    },
    domain::{
        entities::Frame,
        value_objects::{Priority, SessionId, StreamId},
    },
};
use std::sync::Arc;

/// Main orchestrator that coordinates streaming operations using specialized services
/// 
/// This orchestrator follows the Single Responsibility Principle by delegating
/// specific concerns to dedicated services while maintaining coordination logic.
#[derive(Debug)]
pub struct StreamOrchestrator<CH>
where
    CH: CommandHandler<GenerateFramesCommand, Vec<Frame>>
        + CommandHandler<BatchGenerateFramesCommand, Vec<Frame>>
        + CommandHandler<AdjustPriorityThresholdCommand, ()>,
{
    // Core command handler
    command_handler: Arc<CH>,
    
    // Specialized services
    prioritization_service: Arc<PrioritizationService>,
    performance_service: Arc<std::sync::Mutex<PerformanceAnalysisService>>,
    optimization_service: Arc<OptimizationService>,
}

impl<CH> StreamOrchestrator<CH>
where
    CH: CommandHandler<GenerateFramesCommand, Vec<Frame>>
        + CommandHandler<BatchGenerateFramesCommand, Vec<Frame>>
        + CommandHandler<AdjustPriorityThresholdCommand, ()>
        + Send
        + Sync,
{
    pub fn new(
        command_handler: Arc<CH>,
        prioritization_service: Arc<PrioritizationService>,
        performance_service: Arc<std::sync::Mutex<PerformanceAnalysisService>>,
        optimization_service: Arc<OptimizationService>,
    ) -> Self {
        Self {
            command_handler,
            prioritization_service,
            performance_service,
            optimization_service,
        }
    }

    /// Generate frames with adaptive priority calculation
    /// 
    /// Delegates priority calculation to PrioritizationService and uses
    /// PerformanceAnalysisService for context analysis.
    pub async fn generate_adaptive_frames(
        &self,
        session_id: SessionId,
        stream_id: StreamId,
        performance_context: &PerformanceContext,
    ) -> ApplicationResult<AdaptiveFrameResult> {
        // Delegate priority calculation to specialized service
        let priority_result = self.prioritization_service
            .calculate_adaptive_priority(performance_context)?;

        // Delegate batch size calculation to performance service
        let batch_recommendation = {
            let performance_guard = self.performance_service.lock()
                .map_err(|_| crate::application::ApplicationError::Logic("Failed to acquire performance service lock".to_string()))?;
            
            performance_guard.calculate_optimal_batch_size(10)? // Base size of 10
        };

        // Generate frames using calculated parameters
        let command = GenerateFramesCommand {
            session_id: session_id.into(),
            stream_id: stream_id.into(),
            priority_threshold: priority_result.calculated_priority.into(),
            max_frames: batch_recommendation.recommended_size,
        };

        let frames = self.command_handler.handle(command).await?;

        Ok(AdaptiveFrameResult {
            frames,
            priority_threshold_used: priority_result.calculated_priority,
            batch_size_used: batch_recommendation.recommended_size,
            adaptation_reason: priority_result.reasoning.join("; "),
            confidence_score: priority_result.confidence_score,
        })
    }

    /// Generate frames optimized for cross-stream scenarios
    pub async fn generate_cross_stream_optimized_frames(
        &self,
        session_id: SessionId,
        performance_context: &PerformanceContext,
        stream_count: usize,
    ) -> ApplicationResult<CrossStreamFrameResult> {
        // Use prioritization service for global optimization
        let priority_result = self.prioritization_service
            .calculate_global_priority(performance_context, stream_count)?;

        // Calculate total frame budget
        let total_frames = {
            let performance_guard = self.performance_service.lock()
                .map_err(|_| crate::application::ApplicationError::Logic("Failed to acquire performance service lock".to_string()))?;
            
            let base_batch = performance_guard.calculate_optimal_batch_size(10)?;
            (base_batch.recommended_size as f64 * 1.5) as usize // 50% more for multi-stream
        };

        // Generate optimized batch
        let command = BatchGenerateFramesCommand {
            session_id: session_id.into(),
            priority_threshold: priority_result.calculated_priority.into(),
            max_frames: total_frames,
        };

        let frames = self.command_handler.handle(command).await?;

        // Analyze results using performance service
        let performance_guard = self.performance_service.lock()
            .map_err(|_| crate::application::ApplicationError::Logic("Failed to acquire performance service lock".to_string()))?;
        
        let frame_distribution = performance_guard.analyze_frame_distribution(&frames)?;
        
        drop(performance_guard);

        // Calculate optimization metrics using optimization service
        let optimization_metrics = self.optimization_service
            .calculate_optimization_metrics(
                &crate::application::services::optimization_service::OptimizationStrategy {
                    priority_threshold: priority_result.calculated_priority,
                    max_frame_size: 32 * 1024,
                    batch_size: total_frames,
                    compression_enabled: true,
                    adaptive_quality: true,
                    description: "Cross-stream optimization".to_string(),
                    target_latency_ms: 500.0,
                    target_throughput_mbps: 10.0,
                },
                &frames,
                performance_context,
            )?;

        Ok(CrossStreamFrameResult {
            frames,
            priority_threshold_used: priority_result.calculated_priority,
            total_frames,
            frame_distribution,
            optimization_metrics,
        })
    }

    /// Automatically adjust priorities based on streaming metrics
    pub async fn auto_adjust_priorities(
        &self,
        session_id: SessionId,
        streaming_metrics: &crate::application::services::prioritization_service::StreamingMetrics,
    ) -> ApplicationResult<PriorityAdjustmentResult> {
        // Delegate analysis to prioritization service
        let adjustments = self.prioritization_service
            .analyze_priority_adjustments(streaming_metrics)?;

        let mut applied_adjustments = Vec::new();

        // Apply each adjustment
        for adjustment in adjustments {
            let command = AdjustPriorityThresholdCommand {
                session_id: session_id.into(),
                new_threshold: adjustment.new_threshold.into(),
                reason: adjustment.reason.clone(),
            };

            self.command_handler.handle(command).await?;

            applied_adjustments.push(PriorityAdjustment {
                new_threshold: adjustment.new_threshold,
                reason: adjustment.reason,
                confidence: adjustment.confidence,
                urgency: adjustment.urgency, // Same type now - no conversion needed
            });
        }

        Ok(PriorityAdjustmentResult {
            adjustments: applied_adjustments,
            metrics_analyzed: streaming_metrics.clone(),
        })
    }

    /// Optimize streaming for specific use cases
    pub async fn optimize_for_use_case(
        &self,
        session_id: SessionId,
        use_case: StreamingUseCase,
        performance_context: &PerformanceContext,
    ) -> ApplicationResult<UseCaseOptimizationResult> {
        // Get base strategy from optimization service
        let base_strategy = self.optimization_service
            .get_strategy_for_use_case(&use_case)?;

        // Optimize strategy for current context
        let optimized_strategy = self.optimization_service
            .optimize_strategy_for_context(base_strategy, performance_context)?;

        // Apply optimization
        let command = BatchGenerateFramesCommand {
            session_id: session_id.into(),
            priority_threshold: optimized_strategy.priority_threshold.into(),
            max_frames: optimized_strategy.batch_size,
        };

        let frames = self.command_handler.handle(command).await?;

        // Calculate optimization metrics
        let optimization_metrics = self.optimization_service
            .calculate_optimization_metrics(&optimized_strategy, &frames, performance_context)?;

        Ok(UseCaseOptimizationResult {
            use_case,
            strategy_applied: optimized_strategy,
            frames_generated: frames,
            optimization_metrics,
        })
    }

    /// Record performance metrics for continuous analysis
    pub async fn record_performance_metrics(
        &self,
        session_id: SessionId,
        stream_id: Option<StreamId>,
        latency_ms: f64,
        bytes_transferred: u64,
        duration: std::time::Duration,
        frame_count: usize,
    ) -> ApplicationResult<()> {
        let mut performance_guard = self.performance_service.lock()
            .map_err(|_| crate::application::ApplicationError::Logic("Failed to acquire performance service lock".to_string()))?;

        // Record latency
        performance_guard.record_latency(
            session_id,
            stream_id,
            latency_ms,
            "streaming_operation".to_string(),
        )?;

        // Record throughput
        performance_guard.record_throughput(
            session_id,
            bytes_transferred,
            duration,
            frame_count,
        )?;

        Ok(())
    }

    /// Get comprehensive performance analysis
    pub async fn get_performance_analysis(&self) -> ApplicationResult<crate::application::services::performance_analysis_service::PerformanceAnalysisReport> {
        let performance_guard = self.performance_service.lock()
            .map_err(|_| crate::application::ApplicationError::Logic("Failed to acquire performance service lock".to_string()))?;

        performance_guard.analyze_performance()
    }

    /// Get strategy recommendations based on performance analysis
    pub async fn get_strategy_recommendations(
        &self,
        current_strategy: &crate::application::services::optimization_service::OptimizationStrategy,
    ) -> ApplicationResult<Vec<crate::application::services::optimization_service::StrategyAdjustmentRecommendation>> {
        // Get performance analysis
        let performance_report = self.get_performance_analysis().await?;

        // Get recommendations from optimization service
        self.optimization_service
            .recommend_strategy_adjustments(current_strategy, &performance_report)
    }
}

// Factory for creating StreamOrchestrator with default services
pub struct StreamOrchestratorFactory;

impl StreamOrchestratorFactory {
    pub fn create_with_default_services<CH>(command_handler: Arc<CH>) -> StreamOrchestrator<CH>
    where
        CH: CommandHandler<GenerateFramesCommand, Vec<Frame>>
            + CommandHandler<BatchGenerateFramesCommand, Vec<Frame>>
            + CommandHandler<AdjustPriorityThresholdCommand, ()>
            + Send
            + Sync,
    {
        let prioritization_service = Arc::new(PrioritizationService::default());
        let performance_service = Arc::new(std::sync::Mutex::new(PerformanceAnalysisService::default()));
        let optimization_service = Arc::new(OptimizationService::default());

        StreamOrchestrator::new(
            command_handler,
            prioritization_service,
            performance_service,
            optimization_service,
        )
    }

    pub fn create_with_custom_services<CH>(
        command_handler: Arc<CH>,
        prioritization_service: Arc<PrioritizationService>,
        performance_service: Arc<std::sync::Mutex<PerformanceAnalysisService>>,
        optimization_service: Arc<OptimizationService>,
    ) -> StreamOrchestrator<CH>
    where
        CH: CommandHandler<GenerateFramesCommand, Vec<Frame>>
            + CommandHandler<BatchGenerateFramesCommand, Vec<Frame>>
            + CommandHandler<AdjustPriorityThresholdCommand, ()>
            + Send
            + Sync,
    {
        StreamOrchestrator::new(
            command_handler,
            prioritization_service,
            performance_service,
            optimization_service,
        )
    }
}

// Result types (these would normally be defined in the original streaming_service.rs)

#[derive(Debug, Clone)]
pub struct AdaptiveFrameResult {
    pub frames: Vec<Frame>,
    pub priority_threshold_used: Priority,
    pub batch_size_used: usize,
    pub adaptation_reason: String,
    pub confidence_score: f64,
}

#[derive(Debug, Clone)]
pub struct CrossStreamFrameResult {
    pub frames: Vec<Frame>,
    pub priority_threshold_used: Priority,
    pub total_frames: usize,
    pub frame_distribution: crate::application::services::performance_analysis_service::FrameDistributionAnalysis,
    pub optimization_metrics: crate::application::services::optimization_service::OptimizationMetrics,
}

#[derive(Debug, Clone)]
pub struct PriorityAdjustmentResult {
    pub adjustments: Vec<PriorityAdjustment>,
    pub metrics_analyzed: crate::application::services::prioritization_service::StreamingMetrics,
}

#[derive(Debug, Clone)]
pub struct PriorityAdjustment {
    pub new_threshold: Priority,
    pub reason: String,
    pub confidence: f64,
    pub urgency: AdjustmentUrgency,
}

// Use shared AdjustmentUrgency type
use crate::application::shared::AdjustmentUrgency;

pub type UseCaseOptimizationResult = crate::application::services::optimization_service::UseCaseOptimizationResult;

#[cfg(test)]
mod tests {
    use super::*;
    // use crate::application::services::prioritization_service::PrioritizationStrategy; // TODO: Use when implementing custom strategies

    // Mock command handler for testing
    struct MockCommandHandler {
        frames: Vec<Frame>,
    }

    #[async_trait::async_trait]
    impl CommandHandler<GenerateFramesCommand, Vec<Frame>> for MockCommandHandler {
        async fn handle(&self, _command: GenerateFramesCommand) -> ApplicationResult<Vec<Frame>> {
            Ok(self.frames.clone())
        }
    }

    #[async_trait::async_trait]
    impl CommandHandler<BatchGenerateFramesCommand, Vec<Frame>> for MockCommandHandler {
        async fn handle(&self, _command: BatchGenerateFramesCommand) -> ApplicationResult<Vec<Frame>> {
            Ok(self.frames.clone())
        }
    }

    #[async_trait::async_trait]
    impl CommandHandler<AdjustPriorityThresholdCommand, ()> for MockCommandHandler {
        async fn handle(&self, _command: AdjustPriorityThresholdCommand) -> ApplicationResult<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_orchestrator_creation() {
        let command_handler = Arc::new(MockCommandHandler { frames: Vec::new() });
        let orchestrator = StreamOrchestratorFactory::create_with_default_services(command_handler);
        
        // Test that orchestrator can be created and basic functionality works
        let context = PerformanceContext::default();
        let session_id = SessionId::new();
        let stream_id = StreamId::new();

        let result = orchestrator.generate_adaptive_frames(session_id, stream_id, &context).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_cross_stream_optimization() {
        let command_handler = Arc::new(MockCommandHandler { frames: Vec::new() });
        let orchestrator = StreamOrchestratorFactory::create_with_default_services(command_handler);
        
        let context = PerformanceContext::default();
        let session_id = SessionId::new();

        let result = orchestrator.generate_cross_stream_optimized_frames(session_id, &context, 5).await;
        assert!(result.is_ok());
    }
}