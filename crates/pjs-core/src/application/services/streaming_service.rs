//! High-level streaming orchestration service

use std::sync::Arc;
use crate::{
    application::{
        ApplicationResult,
        commands::*,
        queries::*,
        handlers::{CommandHandler, QueryHandler},
    },
    domain::{
        value_objects::{SessionId, StreamId, Priority},
        entities::Frame,
        services::PriorityService,
    },
};

/// High-level service for streaming workflows and optimizations
#[derive(Debug)]
pub struct StreamingService<CH> 
where
    CH: CommandHandler<GenerateFramesCommand, Vec<Frame>> +
         CommandHandler<BatchGenerateFramesCommand, Vec<Frame>> +
         CommandHandler<AdjustPriorityThresholdCommand, ()>,
{
    command_handler: Arc<CH>,
    priority_service: Arc<PriorityService>,
}

impl<CH> StreamingService<CH>
where
    CH: CommandHandler<GenerateFramesCommand, Vec<Frame>> +
         CommandHandler<BatchGenerateFramesCommand, Vec<Frame>> +
         CommandHandler<AdjustPriorityThresholdCommand, ()> +
         Send + Sync,
{
    pub fn new(command_handler: Arc<CH>, priority_service: Arc<PriorityService>) -> Self {
        Self {
            command_handler,
            priority_service,
        }
    }
    
    /// Generate optimized frames for a single stream based on adaptive priority
    pub async fn generate_adaptive_frames(
        &self,
        session_id: SessionId,
        stream_id: StreamId,
        performance_context: &PerformanceContext,
    ) -> ApplicationResult<AdaptiveFrameResult> {
        // Calculate adaptive priority threshold based on performance
        let priority_threshold = self.calculate_adaptive_priority(performance_context);
        
        // Calculate optimal batch size based on network conditions
        let max_frames = self.calculate_optimal_batch_size(performance_context);
        
        // Generate frames
        let command = GenerateFramesCommand {
            session_id,
            stream_id,
            priority_threshold,
            max_frames,
        };
        
        let frames = self.command_handler.handle(command).await?;
        
        Ok(AdaptiveFrameResult {
            frames,
            priority_threshold_used: priority_threshold,
            batch_size_used: max_frames,
            adaptation_reason: self.get_adaptation_reason(performance_context),
        })
    }
    
    /// Generate priority-optimized frames across multiple streams
    pub async fn generate_cross_stream_optimized_frames(
        &self,
        session_id: SessionId,
        performance_context: &PerformanceContext,
    ) -> ApplicationResult<CrossStreamFrameResult> {
        // Calculate global priority threshold
        let priority_threshold = self.calculate_global_priority_threshold(performance_context);
        
        // Calculate total frame budget
        let max_frames = self.calculate_total_frame_budget(performance_context);
        
        // Generate cross-stream optimized batch
        let command = BatchGenerateFramesCommand {
            session_id,
            priority_threshold,
            max_frames,
        };
        
        let frames = self.command_handler.handle(command).await?;
        
        // Analyze the results
        let frame_distribution = self.analyze_frame_distribution(&frames);
        
        let optimization_metrics = self.calculate_optimization_metrics(&frames, performance_context);
        
        Ok(CrossStreamFrameResult {
            frames,
            priority_threshold_used: priority_threshold,
            total_frames: max_frames,
            frame_distribution,
            optimization_metrics,
        })
    }
    
    /// Automatically adjust priority thresholds based on streaming performance
    pub async fn auto_adjust_priorities(
        &self,
        session_id: SessionId,
        streaming_metrics: &StreamingMetrics,
    ) -> ApplicationResult<PriorityAdjustmentResult> {
        let mut adjustments = Vec::new();
        
        // Analyze latency performance
        if let Some(adjustment) = self.analyze_latency_adjustment(streaming_metrics) {
            let command = AdjustPriorityThresholdCommand {
                session_id,
                new_threshold: adjustment.new_threshold,
                reason: adjustment.reason.clone(),
            };
            
            self.command_handler.handle(command).await?;
            adjustments.push(adjustment);
        }
        
        // Analyze throughput performance
        if let Some(adjustment) = self.analyze_throughput_adjustment(streaming_metrics) {
            let command = AdjustPriorityThresholdCommand {
                session_id,
                new_threshold: adjustment.new_threshold,
                reason: adjustment.reason.clone(),
            };
            
            self.command_handler.handle(command).await?;
            adjustments.push(adjustment);
        }
        
        // Analyze error rate performance
        if let Some(adjustment) = self.analyze_error_rate_adjustment(streaming_metrics) {
            let command = AdjustPriorityThresholdCommand {
                session_id,
                new_threshold: adjustment.new_threshold,
                reason: adjustment.reason.clone(),
            };
            
            self.command_handler.handle(command).await?;
            adjustments.push(adjustment);
        }
        
        Ok(PriorityAdjustmentResult {
            adjustments,
            metrics_analyzed: streaming_metrics.clone(),
        })
    }
    
    /// Optimize streaming for specific use cases
    pub async fn optimize_for_use_case(
        &self,
        session_id: SessionId,
        use_case: StreamingUseCase,
    ) -> ApplicationResult<UseCaseOptimizationResult> {
        let optimization_strategy = match use_case {
            StreamingUseCase::RealTimeDashboard => {
                OptimizationStrategy {
                    priority_threshold: Priority::HIGH,
                    max_frame_size: 16 * 1024, // 16KB for low latency
                    batch_size: 5,
                    description: "Optimized for real-time dashboard updates".to_string(),
                }
            },
            StreamingUseCase::BulkDataTransfer => {
                OptimizationStrategy {
                    priority_threshold: Priority::MEDIUM,
                    max_frame_size: 256 * 1024, // 256KB for throughput
                    batch_size: 20,
                    description: "Optimized for bulk data transfer efficiency".to_string(),
                }
            },
            StreamingUseCase::MobileApp => {
                OptimizationStrategy {
                    priority_threshold: Priority::HIGH,
                    max_frame_size: 8 * 1024, // 8KB for mobile networks
                    batch_size: 3,
                    description: "Optimized for mobile network constraints".to_string(),
                }
            },
            StreamingUseCase::ProgressiveWebApp => {
                OptimizationStrategy {
                    priority_threshold: Priority::CRITICAL,
                    max_frame_size: 32 * 1024, // 32KB balanced
                    batch_size: 8,
                    description: "Optimized for progressive web app UX".to_string(),
                }
            },
        };
        
        // Apply optimization
        let command = BatchGenerateFramesCommand {
            session_id,
            priority_threshold: optimization_strategy.priority_threshold,
            max_frames: optimization_strategy.batch_size,
        };
        
        let frames = self.command_handler.handle(command).await?;
        
        Ok(UseCaseOptimizationResult {
            use_case,
            strategy_applied: optimization_strategy,
            frames_generated: frames,
        })
    }
    
    /// Private: Calculate adaptive priority based on performance
    fn calculate_adaptive_priority(&self, context: &PerformanceContext) -> Priority {
        let mut priority = Priority::MEDIUM;
        
        // Adjust based on latency
        if context.average_latency_ms > 1000.0 {
            priority = Priority::HIGH; // Only send high priority in high latency
        } else if context.average_latency_ms < 100.0 {
            priority = Priority::LOW; // Can afford to send more data
        }
        
        // Adjust based on bandwidth
        if context.available_bandwidth_mbps < 1.0 {
            priority = priority.increase_by(20); // Prioritize more aggressively
        } else if context.available_bandwidth_mbps > 10.0 {
            priority = priority.decrease_by(10); // Can send more data
        }
        
        // Adjust based on error rate
        if context.error_rate > 0.05 {
            priority = priority.increase_by(30); // Much more selective
        }
        
        priority
    }
    
    /// Private: Calculate optimal batch size
    fn calculate_optimal_batch_size(&self, context: &PerformanceContext) -> usize {
        let base_size = 10;
        
        // Adjust based on latency (lower latency = smaller batches for responsiveness)
        let latency_factor = if context.average_latency_ms < 50.0 {
            0.5
        } else if context.average_latency_ms > 500.0 {
            2.0
        } else {
            1.0
        };
        
        // Adjust based on bandwidth
        let bandwidth_factor = (context.available_bandwidth_mbps / 5.0).min(3.0).max(0.2);
        
        // Adjust based on CPU usage
        let cpu_factor = if context.cpu_usage > 0.8 {
            0.7 // Reduce batch size when CPU is high
        } else {
            1.0
        };
        
        ((base_size as f64) * latency_factor * bandwidth_factor * cpu_factor) as usize
    }
    
    /// Private: Calculate global priority threshold for multi-stream optimization
    fn calculate_global_priority_threshold(&self, context: &PerformanceContext) -> Priority {
        // More aggressive prioritization for global optimization
        let individual_threshold = self.calculate_adaptive_priority(context);
        individual_threshold.increase_by(10)
    }
    
    /// Private: Calculate total frame budget
    fn calculate_total_frame_budget(&self, context: &PerformanceContext) -> usize {
        let individual_budget = self.calculate_optimal_batch_size(context);
        (individual_budget as f64 * 1.5) as usize // 50% more for multi-stream
    }
    
    /// Private: Get adaptation reason description
    fn get_adaptation_reason(&self, context: &PerformanceContext) -> String {
        let mut reasons = Vec::new();
        
        if context.average_latency_ms > 1000.0 {
            reasons.push("High latency detected".to_string());
        }
        
        if context.available_bandwidth_mbps < 1.0 {
            reasons.push("Limited bandwidth".to_string());
        }
        
        if context.error_rate > 0.05 {
            reasons.push("High error rate".to_string());
        }
        
        if context.cpu_usage > 0.8 {
            reasons.push("High CPU usage".to_string());
        }
        
        if reasons.is_empty() {
            "Optimal conditions".to_string()
        } else {
            reasons.join(", ")
        }
    }
    
    /// Private: Analyze frame distribution
    fn analyze_frame_distribution(&self, frames: &[Frame]) -> FrameDistribution {
        let mut critical = 0;
        let mut high = 0;
        let mut medium = 0;
        let mut low = 0;
        let mut background = 0;
        
        for frame in frames {
            match frame.priority() {
                p if p >= Priority::CRITICAL => critical += 1,
                p if p >= Priority::HIGH => high += 1,
                p if p >= Priority::MEDIUM => medium += 1,
                p if p >= Priority::LOW => low += 1,
                _ => background += 1,
            }
        }
        
        FrameDistribution {
            critical,
            high,
            medium,
            low,
            background,
        }
    }
    
    /// Private: Calculate optimization metrics
    fn calculate_optimization_metrics(
        &self,
        frames: &[Frame],
        context: &PerformanceContext,
    ) -> OptimizationMetrics {
        let total_size: usize = frames.iter().map(|f| f.estimated_size()).sum();
        let average_priority: f64 = frames.iter()
            .map(|f| f.priority().value() as f64)
            .sum::<f64>() / frames.len() as f64;
        
        let estimated_transfer_time = total_size as f64 / 
            (context.available_bandwidth_mbps * 125_000.0); // Convert to bytes/sec
        
        OptimizationMetrics {
            total_frames: frames.len(),
            total_bytes: total_size,
            average_priority,
            estimated_transfer_time_seconds: estimated_transfer_time,
            compression_ratio: 1.0, // Would calculate actual compression
        }
    }
    
    /// Private: Analyze latency-based adjustments
    fn analyze_latency_adjustment(&self, metrics: &StreamingMetrics) -> Option<PriorityAdjustment> {
        if metrics.average_latency_ms > 2000.0 {
            Some(PriorityAdjustment {
                new_threshold: Priority::CRITICAL,
                reason: format!("Latency too high: {}ms", metrics.average_latency_ms),
                impact: "Reducing data volume for latency".to_string(),
            })
        } else if metrics.average_latency_ms < 50.0 && metrics.throughput_mbps > 5.0 {
            Some(PriorityAdjustment {
                new_threshold: Priority::LOW,
                reason: format!("Excellent latency: {}ms", metrics.average_latency_ms),
                impact: "Increasing data volume for throughput".to_string(),
            })
        } else {
            None
        }
    }
    
    /// Private: Analyze throughput-based adjustments
    fn analyze_throughput_adjustment(&self, metrics: &StreamingMetrics) -> Option<PriorityAdjustment> {
        if metrics.throughput_mbps < 0.5 {
            Some(PriorityAdjustment {
                new_threshold: Priority::HIGH,
                reason: format!("Low throughput: {:.2} Mbps", metrics.throughput_mbps),
                impact: "Prioritizing critical data only".to_string(),
            })
        } else {
            None
        }
    }
    
    /// Private: Analyze error rate adjustments
    fn analyze_error_rate_adjustment(&self, metrics: &StreamingMetrics) -> Option<PriorityAdjustment> {
        if metrics.error_rate > 0.1 {
            Some(PriorityAdjustment {
                new_threshold: Priority::CRITICAL,
                reason: format!("High error rate: {:.1}%", metrics.error_rate * 100.0),
                impact: "Sending only most critical data".to_string(),
            })
        } else {
            None
        }
    }
}

/// Performance context for adaptive streaming
#[derive(Debug, Clone)]
pub struct PerformanceContext {
    pub average_latency_ms: f64,
    pub available_bandwidth_mbps: f64,
    pub error_rate: f64,
    pub cpu_usage: f64,
    pub memory_usage: f64,
}

/// Streaming metrics for analysis
#[derive(Debug, Clone)]
pub struct StreamingMetrics {
    pub average_latency_ms: f64,
    pub throughput_mbps: f64,
    pub error_rate: f64,
    pub frames_per_second: f64,
    pub active_streams: usize,
}

/// Streaming use cases for optimization
#[derive(Debug, Clone)]
pub enum StreamingUseCase {
    RealTimeDashboard,
    BulkDataTransfer,
    MobileApp,
    ProgressiveWebApp,
}

/// Optimization strategy configuration
#[derive(Debug, Clone)]
pub struct OptimizationStrategy {
    pub priority_threshold: Priority,
    pub max_frame_size: usize,
    pub batch_size: usize,
    pub description: String,
}

/// Result of adaptive frame generation
#[derive(Debug, Clone)]
pub struct AdaptiveFrameResult {
    pub frames: Vec<Frame>,
    pub priority_threshold_used: Priority,
    pub batch_size_used: usize,
    pub adaptation_reason: String,
}

/// Result of cross-stream frame generation
#[derive(Debug, Clone)]
pub struct CrossStreamFrameResult {
    pub frames: Vec<Frame>,
    pub priority_threshold_used: Priority,
    pub total_frames: usize,
    pub frame_distribution: FrameDistribution,
    pub optimization_metrics: OptimizationMetrics,
}

/// Frame distribution by priority
#[derive(Debug, Clone)]
pub struct FrameDistribution {
    pub critical: usize,
    pub high: usize,
    pub medium: usize,
    pub low: usize,
    pub background: usize,
}

/// Optimization effectiveness metrics
#[derive(Debug, Clone)]
pub struct OptimizationMetrics {
    pub total_frames: usize,
    pub total_bytes: usize,
    pub average_priority: f64,
    pub estimated_transfer_time_seconds: f64,
    pub compression_ratio: f64,
}

/// Priority adjustment recommendation
#[derive(Debug, Clone)]
pub struct PriorityAdjustment {
    pub new_threshold: Priority,
    pub reason: String,
    pub impact: String,
}

/// Result of priority adjustment analysis
#[derive(Debug, Clone)]
pub struct PriorityAdjustmentResult {
    pub adjustments: Vec<PriorityAdjustment>,
    pub metrics_analyzed: StreamingMetrics,
}

/// Result of use case optimization
#[derive(Debug, Clone)]
pub struct UseCaseOptimizationResult {
    pub use_case: StreamingUseCase,
    pub strategy_applied: OptimizationStrategy,
    pub frames_generated: Vec<Frame>,
}