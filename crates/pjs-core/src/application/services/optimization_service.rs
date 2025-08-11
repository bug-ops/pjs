//! Service responsible for use case optimization strategies
//!
//! This service focuses on applying different optimization strategies
//! based on specific streaming use cases and requirements.

use crate::{
    application::ApplicationResult,
    domain::value_objects::Priority,
};

/// Service for optimization strategies and use case handling
#[derive(Debug)]
pub struct OptimizationService {
    default_strategy: OptimizationStrategy,
    custom_strategies: std::collections::HashMap<String, OptimizationStrategy>,
}

/// Supported streaming use cases
#[derive(Debug, Clone, PartialEq)]
pub enum StreamingUseCase {
    /// Real-time dashboard updates requiring low latency
    RealTimeDashboard,
    /// Bulk data transfer prioritizing throughput
    BulkDataTransfer,
    /// Mobile application with network constraints
    MobileApp,
    /// Progressive web application balancing UX and performance
    ProgressiveWebApp,
    /// IoT device streaming with power constraints
    IoTDevice,
    /// Live streaming with audience interaction
    LiveStreaming,
    /// Custom use case with specific requirements
    Custom(String),
}

/// Optimization strategy configuration
#[derive(Debug, Clone)]
pub struct OptimizationStrategy {
    pub priority_threshold: Priority,
    pub max_frame_size: usize,
    pub batch_size: usize,
    pub compression_enabled: bool,
    pub adaptive_quality: bool,
    pub description: String,
    pub target_latency_ms: f64,
    pub target_throughput_mbps: f64,
}

/// Result of use case optimization
#[derive(Debug, Clone)]
pub struct UseCaseOptimizationResult {
    pub use_case: StreamingUseCase,
    pub strategy_applied: OptimizationStrategy,
    pub frames_generated: Vec<crate::domain::entities::Frame>,
    pub optimization_metrics: OptimizationMetrics,
}

/// Metrics about the optimization performance
#[derive(Debug, Clone)]
pub struct OptimizationMetrics {
    pub efficiency_score: f64,
    pub latency_improvement: f64,
    pub throughput_improvement: f64,
    pub resource_utilization: f64,
    pub quality_score: f64,
}

impl OptimizationService {
    pub fn new() -> Self {
        Self {
            default_strategy: Self::create_balanced_strategy(),
            custom_strategies: std::collections::HashMap::new(),
        }
    }

    /// Get optimization strategy for a specific use case
    pub fn get_strategy_for_use_case(&self, use_case: &StreamingUseCase) -> ApplicationResult<OptimizationStrategy> {
        match use_case {
            StreamingUseCase::RealTimeDashboard => Ok(Self::create_realtime_dashboard_strategy()),
            StreamingUseCase::BulkDataTransfer => Ok(Self::create_bulk_transfer_strategy()),
            StreamingUseCase::MobileApp => Ok(Self::create_mobile_app_strategy()),
            StreamingUseCase::ProgressiveWebApp => Ok(Self::create_pwa_strategy()),
            StreamingUseCase::IoTDevice => Ok(Self::create_iot_device_strategy()),
            StreamingUseCase::LiveStreaming => Ok(Self::create_live_streaming_strategy()),
            StreamingUseCase::Custom(name) => {
                self.custom_strategies
                    .get(name)
                    .cloned()
                    .ok_or_else(|| crate::application::ApplicationError::Logic(
                        format!("Custom strategy '{}' not found", name)
                    ))
            }
        }
    }

    /// Register a custom optimization strategy
    pub fn register_custom_strategy(
        &mut self,
        name: String,
        strategy: OptimizationStrategy,
    ) -> ApplicationResult<()> {
        self.custom_strategies.insert(name, strategy);
        Ok(())
    }

    /// Optimize strategy based on performance context
    pub fn optimize_strategy_for_context(
        &self,
        base_strategy: OptimizationStrategy,
        context: &crate::application::services::prioritization_service::PerformanceContext,
    ) -> ApplicationResult<OptimizationStrategy> {
        let mut optimized = base_strategy;

        // Adjust priority threshold based on performance
        if context.error_rate > 0.05 {
            optimized.priority_threshold = optimized.priority_threshold.increase_by(20);
        } else if context.error_rate < 0.01 && context.average_latency_ms < 100.0 {
            optimized.priority_threshold = optimized.priority_threshold.decrease_by(10);
        }

        // Adjust batch size based on latency and bandwidth
        if context.average_latency_ms > 1000.0 {
            optimized.batch_size = (optimized.batch_size as f64 * 0.7) as usize;
        } else if context.average_latency_ms < 100.0 && context.available_bandwidth_mbps > 10.0 {
            optimized.batch_size = (optimized.batch_size as f64 * 1.3) as usize;
        }

        // Adjust frame size based on bandwidth
        if context.available_bandwidth_mbps < 2.0 {
            optimized.max_frame_size = (optimized.max_frame_size as f64 * 0.8) as usize;
        } else if context.available_bandwidth_mbps > 20.0 {
            optimized.max_frame_size = (optimized.max_frame_size as f64 * 1.2) as usize;
        }

        // Enable compression for low bandwidth
        if context.available_bandwidth_mbps < 5.0 {
            optimized.compression_enabled = true;
        }

        // Adjust adaptive quality based on CPU usage
        if context.cpu_usage > 0.8 {
            optimized.adaptive_quality = false; // Disable to reduce CPU load
        }

        Ok(optimized)
    }

    /// Calculate optimization metrics for a given strategy and results
    pub fn calculate_optimization_metrics(
        &self,
        strategy: &OptimizationStrategy,
        frames: &[crate::domain::entities::Frame],
        context: &crate::application::services::prioritization_service::PerformanceContext,
    ) -> ApplicationResult<OptimizationMetrics> {
        // Calculate efficiency score based on frame priority distribution
        let efficiency_score = self.calculate_efficiency_score(frames, strategy);

        // Estimate latency improvement
        let latency_improvement = self.estimate_latency_improvement(strategy, context);

        // Estimate throughput improvement
        let throughput_improvement = self.estimate_throughput_improvement(strategy, context);

        // Calculate resource utilization score
        let resource_utilization = self.calculate_resource_utilization(strategy, context);

        // Calculate quality score based on frame completeness
        let quality_score = self.calculate_quality_score(frames, strategy);

        Ok(OptimizationMetrics {
            efficiency_score,
            latency_improvement,
            throughput_improvement,
            resource_utilization,
            quality_score,
        })
    }

    /// Recommend strategy adjustments based on analysis
    pub fn recommend_strategy_adjustments(
        &self,
        current_strategy: &OptimizationStrategy,
        performance_report: &crate::application::services::performance_analysis_service::PerformanceAnalysisReport,
    ) -> ApplicationResult<Vec<StrategyAdjustmentRecommendation>> {
        let mut recommendations = Vec::new();

        // Analyze latency performance
        if performance_report.latency_analysis.average > current_strategy.target_latency_ms * 1.5 {
            recommendations.push(StrategyAdjustmentRecommendation {
                adjustment_type: AdjustmentType::PriorityIncrease,
                description: "Increase priority threshold to reduce latency".to_string(),
                expected_impact: "Reduce latency by 20-40%".to_string(),
                confidence: 0.8,
                urgency: AdjustmentUrgency::High,
            });
        }

        // Analyze throughput performance
        if performance_report.throughput_analysis.average_mbps < current_strategy.target_throughput_mbps * 0.7 {
            recommendations.push(StrategyAdjustmentRecommendation {
                adjustment_type: AdjustmentType::BatchSizeIncrease,
                description: "Increase batch size to improve throughput".to_string(),
                expected_impact: "Improve throughput by 15-30%".to_string(),
                confidence: 0.7,
                urgency: AdjustmentUrgency::Medium,
            });
        }

        // Analyze error rate
        if performance_report.error_analysis.error_rate > 0.05 {
            recommendations.push(StrategyAdjustmentRecommendation {
                adjustment_type: AdjustmentType::QualityReduction,
                description: "Enable adaptive quality to reduce errors".to_string(),
                expected_impact: "Reduce error rate by 30-50%".to_string(),
                confidence: 0.9,
                urgency: AdjustmentUrgency::High,
            });
        }

        // Analyze resource usage
        if performance_report.resource_analysis.current_cpu_usage > 0.8 {
            recommendations.push(StrategyAdjustmentRecommendation {
                adjustment_type: AdjustmentType::CompressionDisable,
                description: "Disable compression to reduce CPU load".to_string(),
                expected_impact: "Reduce CPU usage by 10-20%".to_string(),
                confidence: 0.8,
                urgency: AdjustmentUrgency::Medium,
            });
        }

        Ok(recommendations)
    }

    // Private implementation methods for creating specific strategies

    fn create_realtime_dashboard_strategy() -> OptimizationStrategy {
        OptimizationStrategy {
            priority_threshold: Priority::HIGH,
            max_frame_size: 16 * 1024, // 16KB for low latency
            batch_size: 5,
            compression_enabled: false, // Avoid compression latency
            adaptive_quality: true,
            description: "Optimized for real-time dashboard updates with minimal latency".to_string(),
            target_latency_ms: 100.0,
            target_throughput_mbps: 5.0,
        }
    }

    fn create_bulk_transfer_strategy() -> OptimizationStrategy {
        OptimizationStrategy {
            priority_threshold: Priority::MEDIUM,
            max_frame_size: 256 * 1024, // 256KB for throughput
            batch_size: 20,
            compression_enabled: true,
            adaptive_quality: false, // Consistent quality for bulk
            description: "Optimized for bulk data transfer with maximum throughput".to_string(),
            target_latency_ms: 1000.0,
            target_throughput_mbps: 50.0,
        }
    }

    fn create_mobile_app_strategy() -> OptimizationStrategy {
        OptimizationStrategy {
            priority_threshold: Priority::HIGH,
            max_frame_size: 8 * 1024, // 8KB for mobile networks
            batch_size: 3,
            compression_enabled: true, // Important for mobile data usage
            adaptive_quality: true,
            description: "Optimized for mobile network constraints and battery life".to_string(),
            target_latency_ms: 300.0,
            target_throughput_mbps: 2.0,
        }
    }

    fn create_pwa_strategy() -> OptimizationStrategy {
        OptimizationStrategy {
            priority_threshold: Priority::CRITICAL,
            max_frame_size: 32 * 1024, // 32KB balanced
            batch_size: 8,
            compression_enabled: true,
            adaptive_quality: true,
            description: "Optimized for progressive web app user experience".to_string(),
            target_latency_ms: 200.0,
            target_throughput_mbps: 10.0,
        }
    }

    fn create_iot_device_strategy() -> OptimizationStrategy {
        OptimizationStrategy {
            priority_threshold: Priority::CRITICAL,
            max_frame_size: 4 * 1024, // 4KB for constrained devices
            batch_size: 2,
            compression_enabled: true, // Critical for bandwidth savings
            adaptive_quality: false, // Consistent behavior
            description: "Optimized for IoT devices with power and bandwidth constraints".to_string(),
            target_latency_ms: 500.0,
            target_throughput_mbps: 1.0,
        }
    }

    fn create_live_streaming_strategy() -> OptimizationStrategy {
        OptimizationStrategy {
            priority_threshold: Priority::HIGH,
            max_frame_size: 64 * 1024, // 64KB for video frames
            batch_size: 10,
            compression_enabled: true,
            adaptive_quality: true, // Important for varying network conditions
            description: "Optimized for live streaming with audience interaction".to_string(),
            target_latency_ms: 500.0,
            target_throughput_mbps: 20.0,
        }
    }

    fn create_balanced_strategy() -> OptimizationStrategy {
        OptimizationStrategy {
            priority_threshold: Priority::MEDIUM,
            max_frame_size: 32 * 1024,
            batch_size: 10,
            compression_enabled: true,
            adaptive_quality: true,
            description: "Balanced strategy for general use cases".to_string(),
            target_latency_ms: 500.0,
            target_throughput_mbps: 10.0,
        }
    }

    // Private calculation methods

    fn calculate_efficiency_score(&self, frames: &[crate::domain::entities::Frame], strategy: &OptimizationStrategy) -> f64 {
        if frames.is_empty() {
            return 0.0;
        }

        // Calculate how well the frames align with the strategy
        let target_priority = strategy.priority_threshold.value();
        let mut efficiency_sum = 0.0;

        for frame in frames {
            if let Some(priority) = frame.priority() {
                // Score based on how close the frame priority is to target
                let priority_diff = (priority as i32 - target_priority as i32).abs() as f64;
                let priority_score = 1.0 - (priority_diff / 255.0).min(1.0);
                
                // Score based on frame size efficiency
                let size_score = if frame.data().len() <= strategy.max_frame_size {
                    1.0
                } else {
                    strategy.max_frame_size as f64 / frame.data().len() as f64
                };

                efficiency_sum += (priority_score + size_score) / 2.0;
            }
        }

        (efficiency_sum / frames.len() as f64).min(1.0)
    }

    fn estimate_latency_improvement(&self, strategy: &OptimizationStrategy, context: &crate::application::services::prioritization_service::PerformanceContext) -> f64 {
        // Estimate improvement based on strategy parameters
        let mut improvement = 0.0;

        // Higher priority threshold should reduce latency
        let priority_factor = strategy.priority_threshold.value() as f64 / 255.0;
        improvement += priority_factor * 0.3;

        // Smaller batch sizes reduce latency
        let batch_factor = 1.0 - (strategy.batch_size as f64 / 50.0).min(1.0);
        improvement += batch_factor * 0.2;

        // Smaller frame sizes reduce processing time
        let frame_size_factor = 1.0 - (strategy.max_frame_size as f64 / (1024.0 * 1024.0)).min(1.0);
        improvement += frame_size_factor * 0.2;

        // Consider current performance
        if context.average_latency_ms > strategy.target_latency_ms {
            improvement *= 1.5; // Higher improvement potential
        }

        improvement.min(1.0)
    }

    fn estimate_throughput_improvement(&self, strategy: &OptimizationStrategy, context: &crate::application::services::prioritization_service::PerformanceContext) -> f64 {
        let mut improvement = 0.0;

        // Larger batch sizes improve throughput
        let batch_factor = (strategy.batch_size as f64 / 50.0).min(1.0);
        improvement += batch_factor * 0.4;

        // Compression can improve effective throughput
        if strategy.compression_enabled {
            improvement += 0.2;
        }

        // Larger frame sizes can improve efficiency
        let frame_size_factor = (strategy.max_frame_size as f64 / (1024.0 * 1024.0)).min(1.0);
        improvement += frame_size_factor * 0.3;

        // Consider current performance
        if context.available_bandwidth_mbps < strategy.target_throughput_mbps {
            improvement *= 1.3; // Higher improvement potential
        }

        improvement.min(1.0)
    }

    fn calculate_resource_utilization(&self, strategy: &OptimizationStrategy, context: &crate::application::services::prioritization_service::PerformanceContext) -> f64 {
        let mut utilization = context.cpu_usage;

        // Compression increases CPU usage
        if strategy.compression_enabled {
            utilization += 0.1;
        }

        // Adaptive quality increases CPU usage
        if strategy.adaptive_quality {
            utilization += 0.05;
        }

        // Larger batch sizes reduce relative overhead
        let batch_efficiency = (strategy.batch_size as f64 / 20.0).min(1.0);
        utilization -= batch_efficiency * 0.1;

        utilization.max(0.0).min(1.0)
    }

    fn calculate_quality_score(&self, frames: &[crate::domain::entities::Frame], strategy: &OptimizationStrategy) -> f64 {
        if frames.is_empty() {
            return 0.0;
        }

        let mut quality_sum = 0.0;

        for frame in frames {
            let mut frame_quality = 1.0;

            // Penalize frames that are too large
            if frame.data().len() > strategy.max_frame_size {
                frame_quality *= 0.8;
            }

            // Reward frames with appropriate priority
            if let Some(priority) = frame.priority() {
                if priority >= strategy.priority_threshold.value() {
                    frame_quality *= 1.1;
                }
            }

            quality_sum += frame_quality;
        }

        (quality_sum / frames.len() as f64).min(1.0)
    }
}

impl Default for OptimizationService {
    fn default() -> Self {
        Self::new()
    }
}

// Supporting types

#[derive(Debug, Clone)]
pub struct StrategyAdjustmentRecommendation {
    pub adjustment_type: AdjustmentType,
    pub description: String,
    pub expected_impact: String,
    pub confidence: f64,
    pub urgency: AdjustmentUrgency,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AdjustmentType {
    PriorityIncrease,
    PriorityDecrease,
    BatchSizeIncrease,
    BatchSizeDecrease,
    FrameSizeIncrease,
    FrameSizeDecrease,
    CompressionEnable,
    CompressionDisable,
    QualityIncrease,
    QualityReduction,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AdjustmentUrgency {
    Low,
    Medium,
    High,
    Critical,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_optimization_service_creation() {
        let service = OptimizationService::new();
        assert!(service.custom_strategies.is_empty());
    }

    #[test]
    fn test_realtime_dashboard_strategy() {
        let service = OptimizationService::new();
        let strategy = service.get_strategy_for_use_case(&StreamingUseCase::RealTimeDashboard).unwrap();
        
        assert_eq!(strategy.priority_threshold, Priority::HIGH);
        assert_eq!(strategy.max_frame_size, 16 * 1024);
        assert!(!strategy.compression_enabled); // Low latency priority
    }

    #[test]
    fn test_mobile_app_strategy() {
        let service = OptimizationService::new();
        let strategy = service.get_strategy_for_use_case(&StreamingUseCase::MobileApp).unwrap();
        
        assert_eq!(strategy.max_frame_size, 8 * 1024);
        assert!(strategy.compression_enabled); // Important for mobile
        assert_eq!(strategy.batch_size, 3);
    }

    #[test]
    fn test_custom_strategy_registration() {
        let mut service = OptimizationService::new();
        let custom_strategy = OptimizationStrategy {
            priority_threshold: Priority::CRITICAL,
            max_frame_size: 1024,
            batch_size: 1,
            compression_enabled: false,
            adaptive_quality: false,
            description: "Ultra low latency".to_string(),
            target_latency_ms: 10.0,
            target_throughput_mbps: 1.0,
        };

        service.register_custom_strategy("ultra_low_latency".to_string(), custom_strategy).unwrap();
        
        let retrieved = service.get_strategy_for_use_case(&StreamingUseCase::Custom("ultra_low_latency".to_string())).unwrap();
        assert_eq!(retrieved.target_latency_ms, 10.0);
    }
}