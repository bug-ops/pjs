//! Service responsible for priority calculation and adaptation
//!
//! This service focuses solely on determining the optimal priority thresholds
//! for streaming operations based on performance context and business rules.

use crate::{
    application::ApplicationResult,
    domain::value_objects::Priority,
};

/// Context information for priority calculations
#[derive(Debug, Clone)]
pub struct PerformanceContext {
    pub average_latency_ms: f64,
    pub available_bandwidth_mbps: f64,
    pub error_rate: f64,
    pub cpu_usage: f64,
    pub memory_usage_percent: f64,
    pub connection_count: usize,
}

impl Default for PerformanceContext {
    fn default() -> Self {
        Self {
            average_latency_ms: 100.0,
            available_bandwidth_mbps: 10.0,
            error_rate: 0.01,
            cpu_usage: 0.5,
            memory_usage_percent: 60.0,
            connection_count: 1,
        }
    }
}

/// Strategies for priority calculation
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum PrioritizationStrategy {
    /// Conservative - prioritize stability over performance
    Conservative,
    /// Balanced - balance between performance and stability
    Balanced,
    /// Aggressive - prioritize performance over stability
    Aggressive,
    /// Custom - use custom rules
    Custom(CustomPriorityRules),
}

/// Custom priority calculation rules
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CustomPriorityRules {
    pub latency_threshold_ms: f64,
    pub bandwidth_threshold_mbps: f64,
    pub error_rate_threshold: f64,
    pub priority_boost_on_error: u8,
    pub priority_reduction_on_good_performance: u8,
}

impl Default for CustomPriorityRules {
    fn default() -> Self {
        Self {
            latency_threshold_ms: 500.0,
            bandwidth_threshold_mbps: 5.0,
            error_rate_threshold: 0.03,
            priority_boost_on_error: 20,
            priority_reduction_on_good_performance: 10,
        }
    }
}

/// Result of priority calculation
#[derive(Debug, Clone)]
pub struct PriorityCalculationResult {
    pub calculated_priority: Priority,
    pub reasoning: Vec<String>,
    pub confidence_score: f64,
    pub strategy_used: PrioritizationStrategy,
}

/// Service for calculating and adapting priorities
#[derive(Debug)]
pub struct PrioritizationService {
    strategy: PrioritizationStrategy,
}

impl PrioritizationService {
    pub fn new(strategy: PrioritizationStrategy) -> Self {
        Self { strategy }
    }

    /// Calculate adaptive priority based on current performance context
    pub fn calculate_adaptive_priority(
        &self,
        context: &PerformanceContext,
    ) -> ApplicationResult<PriorityCalculationResult> {
        let mut reasoning = Vec::new();
        
        let priority = match &self.strategy {
            PrioritizationStrategy::Conservative => {
                self.calculate_conservative_priority(context, &mut reasoning)?
            }
            PrioritizationStrategy::Balanced => {
                self.calculate_balanced_priority(context, &mut reasoning)?
            }
            PrioritizationStrategy::Aggressive => {
                self.calculate_aggressive_priority(context, &mut reasoning)?
            }
            PrioritizationStrategy::Custom(rules) => {
                self.calculate_custom_priority(context, rules, &mut reasoning)?
            }
        };

        // Adjust confidence based on context stability
        let confidence = self.calculate_confidence_score(context);

        Ok(PriorityCalculationResult {
            calculated_priority: priority,
            reasoning,
            confidence_score: confidence,
            strategy_used: self.strategy.clone(),
        })
    }

    /// Calculate priority for global multi-stream optimization
    pub fn calculate_global_priority(
        &self,
        context: &PerformanceContext,
        stream_count: usize,
    ) -> ApplicationResult<PriorityCalculationResult> {
        let mut base_result = self.calculate_adaptive_priority(context)?;
        
        // Adjust for multi-stream scenarios
        let stream_factor = match stream_count {
            1..=3 => 0,
            4..=10 => 10,
            11..=50 => 20,
            _ => 30,
        };

        base_result.calculated_priority = base_result.calculated_priority.increase_by(stream_factor);
        base_result.reasoning.push(format!(
            "Increased priority by {stream_factor} for {stream_count} concurrent streams"
        ));

        Ok(base_result)
    }

    /// Calculate priority adjustments based on streaming metrics
    pub fn analyze_priority_adjustments(
        &self,
        metrics: &StreamingMetrics,
    ) -> ApplicationResult<Vec<PriorityAdjustment>> {
        let mut adjustments = Vec::new();

        // Analyze latency-based adjustments
        if let Some(adjustment) = self.analyze_latency_adjustment(metrics)? {
            adjustments.push(adjustment);
        }

        // Analyze throughput-based adjustments
        if let Some(adjustment) = self.analyze_throughput_adjustment(metrics)? {
            adjustments.push(adjustment);
        }

        // Analyze error-rate-based adjustments
        if let Some(adjustment) = self.analyze_error_rate_adjustment(metrics)? {
            adjustments.push(adjustment);
        }

        Ok(adjustments)
    }

    /// Update prioritization strategy dynamically
    pub fn update_strategy(&mut self, new_strategy: PrioritizationStrategy) {
        self.strategy = new_strategy;
    }

    // Private implementation methods

    fn calculate_conservative_priority(
        &self,
        context: &PerformanceContext,
        reasoning: &mut Vec<String>,
    ) -> ApplicationResult<Priority> {
        let mut priority = Priority::HIGH; // Start conservatively high

        // Conservative approach: err on the side of higher priority
        if context.error_rate > 0.02 {
            priority = Priority::CRITICAL;
            reasoning.push("Conservative: High error rate detected".to_string());
        }

        if context.average_latency_ms > 500.0 {
            priority = Priority::CRITICAL;
            reasoning.push("Conservative: High latency detected".to_string());
        }

        if context.cpu_usage > 0.7 {
            priority = priority.increase_by(10);
            reasoning.push("Conservative: High CPU usage".to_string());
        }

        Ok(priority)
    }

    fn calculate_balanced_priority(
        &self,
        context: &PerformanceContext,
        reasoning: &mut Vec<String>,
    ) -> ApplicationResult<Priority> {
        let mut priority = Priority::MEDIUM; // Start balanced
        reasoning.push("Balanced: Starting with medium priority".to_string());

        // Latency adjustments
        if context.average_latency_ms > 1000.0 {
            priority = Priority::HIGH;
            reasoning.push("Balanced: High latency - prioritizing critical data".to_string());
        } else if context.average_latency_ms < 100.0 {
            priority = Priority::LOW;
            reasoning.push("Balanced: Low latency - can send more data".to_string());
        }

        // Bandwidth adjustments
        if context.available_bandwidth_mbps < 1.0 {
            priority = priority.increase_by(20);
            reasoning.push("Balanced: Limited bandwidth - being more selective".to_string());
        } else if context.available_bandwidth_mbps > 10.0 {
            priority = priority.decrease_by(10);
            reasoning.push("Balanced: Good bandwidth - can send more data".to_string());
        }

        // Error rate adjustments
        if context.error_rate > 0.05 {
            priority = priority.increase_by(30);
            reasoning.push("Balanced: High error rate - much more selective".to_string());
        }

        Ok(priority)
    }

    fn calculate_aggressive_priority(
        &self,
        context: &PerformanceContext,
        reasoning: &mut Vec<String>,
    ) -> ApplicationResult<Priority> {
        let mut priority = Priority::LOW; // Start aggressively low

        // Aggressive approach: only increase priority when absolutely necessary
        if context.error_rate > 0.1 {
            priority = Priority::HIGH;
            reasoning.push("Aggressive: Very high error rate - must prioritize".to_string());
        } else if context.error_rate > 0.05 {
            priority = Priority::MEDIUM;
            reasoning.push("Aggressive: High error rate - moderate prioritization".to_string());
        }

        if context.average_latency_ms > 2000.0 {
            priority = Priority::HIGH;
            reasoning.push("Aggressive: Extremely high latency".to_string());
        }

        if context.available_bandwidth_mbps < 0.5 {
            priority = priority.increase_by(40);
            reasoning.push("Aggressive: Very limited bandwidth".to_string());
        }

        Ok(priority)
    }

    fn calculate_custom_priority(
        &self,
        context: &PerformanceContext,
        rules: &CustomPriorityRules,
        reasoning: &mut Vec<String>,
    ) -> ApplicationResult<Priority> {
        let mut priority = Priority::MEDIUM;

        if context.average_latency_ms > rules.latency_threshold_ms {
            priority = priority.increase_by(rules.priority_boost_on_error);
            reasoning.push(format!(
                "Custom: Latency {:.1}ms exceeds threshold {:.1}ms",
                context.average_latency_ms, rules.latency_threshold_ms
            ));
        }

        if context.available_bandwidth_mbps < rules.bandwidth_threshold_mbps {
            priority = priority.increase_by(rules.priority_boost_on_error);
            reasoning.push(format!(
                "Custom: Bandwidth {:.1}Mbps below threshold {:.1}Mbps",
                context.available_bandwidth_mbps, rules.bandwidth_threshold_mbps
            ));
        }

        if context.error_rate > rules.error_rate_threshold {
            priority = priority.increase_by(rules.priority_boost_on_error);
            reasoning.push(format!(
                "Custom: Error rate {:.3} exceeds threshold {:.3}",
                context.error_rate, rules.error_rate_threshold
            ));
        }

        // Apply reduction for good performance
        if context.average_latency_ms < rules.latency_threshold_ms / 2.0 &&
           context.available_bandwidth_mbps > rules.bandwidth_threshold_mbps * 2.0 &&
           context.error_rate < rules.error_rate_threshold / 2.0 {
            priority = priority.decrease_by(rules.priority_reduction_on_good_performance);
            reasoning.push("Custom: Excellent performance - reducing priority".to_string());
        }

        Ok(priority)
    }

    fn calculate_confidence_score(&self, context: &PerformanceContext) -> f64 {
        let mut confidence: f64 = 1.0;

        // Reduce confidence based on volatility indicators
        if context.error_rate > 0.1 {
            confidence *= 0.7; // High error rate reduces confidence
        }

        if context.cpu_usage > 0.9 {
            confidence *= 0.8; // High CPU usage may affect measurements
        }

        if context.connection_count > 100 {
            confidence *= 0.9; // High load may affect accuracy
        }

        confidence.max(0.1) // Minimum confidence of 10%
    }

    fn analyze_latency_adjustment(&self, metrics: &StreamingMetrics) -> ApplicationResult<Option<PriorityAdjustment>> {
        if metrics.average_latency_ms > 1500.0 && metrics.p99_latency_ms > 3000.0 {
            Ok(Some(PriorityAdjustment {
                new_threshold: Priority::CRITICAL,
                reason: format!(
                    "Latency degradation: avg {:.1}ms, p99 {:.1}ms",
                    metrics.average_latency_ms, metrics.p99_latency_ms
                ),
                confidence: 0.9,
                urgency: AdjustmentUrgency::High,
            }))
        } else if metrics.average_latency_ms < 100.0 && metrics.p99_latency_ms < 200.0 {
            Ok(Some(PriorityAdjustment {
                new_threshold: Priority::LOW,
                reason: format!(
                    "Excellent latency: avg {:.1}ms, p99 {:.1}ms",
                    metrics.average_latency_ms, metrics.p99_latency_ms
                ),
                confidence: 0.8,
                urgency: AdjustmentUrgency::Low,
            }))
        } else {
            Ok(None)
        }
    }

    fn analyze_throughput_adjustment(&self, metrics: &StreamingMetrics) -> ApplicationResult<Option<PriorityAdjustment>> {
        if metrics.throughput_mbps < 1.0 && metrics.error_rate < 0.02 {
            // Low throughput but no errors suggests we can be more aggressive
            Ok(Some(PriorityAdjustment {
                new_threshold: Priority::LOW,
                reason: format!("Low throughput {:.1}Mbps with good stability", metrics.throughput_mbps),
                confidence: 0.7,
                urgency: AdjustmentUrgency::Medium,
            }))
        } else if metrics.throughput_mbps > 50.0 {
            // Very high throughput suggests we can be more selective
            Ok(Some(PriorityAdjustment {
                new_threshold: Priority::MEDIUM,
                reason: format!("High throughput {:.1}Mbps allows selectivity", metrics.throughput_mbps),
                confidence: 0.8,
                urgency: AdjustmentUrgency::Low,
            }))
        } else {
            Ok(None)
        }
    }

    fn analyze_error_rate_adjustment(&self, metrics: &StreamingMetrics) -> ApplicationResult<Option<PriorityAdjustment>> {
        if metrics.error_rate > 0.1 {
            Ok(Some(PriorityAdjustment {
                new_threshold: Priority::CRITICAL,
                reason: format!("High error rate {:.1}%", metrics.error_rate * 100.0),
                confidence: 0.95,
                urgency: AdjustmentUrgency::Critical,
            }))
        } else if metrics.error_rate < 0.001 {
            Ok(Some(PriorityAdjustment {
                new_threshold: Priority::LOW,
                reason: format!("Excellent stability {:.3}% errors", metrics.error_rate * 100.0),
                confidence: 0.8,
                urgency: AdjustmentUrgency::Low,
            }))
        } else {
            Ok(None)
        }
    }
}

impl Default for PrioritizationService {
    fn default() -> Self {
        Self::new(PrioritizationStrategy::Balanced)
    }
}

// Supporting types

/// Metrics for streaming performance analysis
#[derive(Debug, Clone)]
pub struct StreamingMetrics {
    pub average_latency_ms: f64,
    pub p50_latency_ms: f64,
    pub p95_latency_ms: f64,
    pub p99_latency_ms: f64,
    pub throughput_mbps: f64,
    pub error_rate: f64,
    pub frames_sent: u64,
    pub bytes_sent: u64,
    pub connections_active: usize,
}

/// Recommended priority adjustment
#[derive(Debug, Clone)]
pub struct PriorityAdjustment {
    pub new_threshold: Priority,
    pub reason: String,
    pub confidence: f64,
    pub urgency: AdjustmentUrgency,
}

// Use shared AdjustmentUrgency type
use crate::application::shared::AdjustmentUrgency;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conservative_strategy() {
        let service = PrioritizationService::new(PrioritizationStrategy::Conservative);
        let context = PerformanceContext {
            average_latency_ms: 600.0,
            error_rate: 0.03,
            ..Default::default()
        };

        let result = service.calculate_adaptive_priority(&context).unwrap();
        assert_eq!(result.calculated_priority, Priority::CRITICAL);
        assert!(!result.reasoning.is_empty());
    }

    #[test]
    fn test_balanced_strategy() {
        let service = PrioritizationService::new(PrioritizationStrategy::Balanced);
        let context = PerformanceContext::default();

        let result = service.calculate_adaptive_priority(&context).unwrap();
        assert!(result.confidence_score > 0.0);
        assert!(!result.reasoning.is_empty());
    }

    #[test]
    fn test_custom_strategy() {
        let custom_rules = CustomPriorityRules {
            latency_threshold_ms: 200.0,
            bandwidth_threshold_mbps: 5.0,
            error_rate_threshold: 0.02,
            priority_boost_on_error: 25,
            priority_reduction_on_good_performance: 15,
        };

        let service = PrioritizationService::new(PrioritizationStrategy::Custom(custom_rules));
        let context = PerformanceContext {
            average_latency_ms: 50.0,  // Good
            available_bandwidth_mbps: 15.0,  // Good
            error_rate: 0.005,  // Good
            ..Default::default()
        };

        let result = service.calculate_adaptive_priority(&context).unwrap();
        // Should reduce priority due to excellent performance
        assert!(result.calculated_priority <= Priority::MEDIUM);
    }
}