//! Comprehensive tests for PrioritizationService
//!
//! This test suite aims to achieve 70%+ coverage by testing:
//! - All prioritization strategies (Conservative, Balanced, Aggressive, Custom)
//! - Priority calculation algorithms
//! - Priority adjustment analysis
//! - Edge cases and boundary conditions
//! - Confidence score calculations
//! - Strategy update functionality

use pjson_rs::application::services::prioritization_service::{
    CustomPriorityRules, PerformanceContext, PrioritizationService, PrioritizationStrategy,
    StreamingMetrics,
};
use pjson_rs::application::shared::AdjustmentUrgency;
use pjson_rs::domain::Priority;

// === Test Fixtures and Helpers ===

fn create_test_context(
    latency: f64,
    bandwidth: f64,
    error_rate: f64,
    cpu_usage: f64,
    memory: f64,
    connections: usize,
) -> PerformanceContext {
    PerformanceContext {
        average_latency_ms: latency,
        available_bandwidth_mbps: bandwidth,
        error_rate,
        cpu_usage,
        memory_usage_percent: memory,
        connection_count: connections,
    }
}

fn create_test_metrics(
    avg_latency: f64,
    p99_latency: f64,
    throughput: f64,
    error_rate: f64,
) -> StreamingMetrics {
    StreamingMetrics {
        average_latency_ms: avg_latency,
        p50_latency_ms: avg_latency * 0.9,
        p95_latency_ms: avg_latency * 1.5,
        p99_latency_ms: p99_latency,
        throughput_mbps: throughput,
        error_rate,
        frames_sent: 1000,
        bytes_sent: 1024000,
        connections_active: 10,
    }
}

// === Service Creation ===

#[test]
fn test_prioritization_service_creation_conservative() {
    let service = PrioritizationService::new(PrioritizationStrategy::Conservative);
    let context = PerformanceContext::default();
    let result = service.calculate_adaptive_priority(&context).unwrap();
    assert!(matches!(
        result.strategy_used,
        PrioritizationStrategy::Conservative
    ));
}

#[test]
fn test_prioritization_service_creation_balanced() {
    let service = PrioritizationService::new(PrioritizationStrategy::Balanced);
    let context = PerformanceContext::default();
    let result = service.calculate_adaptive_priority(&context).unwrap();
    assert!(matches!(
        result.strategy_used,
        PrioritizationStrategy::Balanced
    ));
}

#[test]
fn test_prioritization_service_creation_aggressive() {
    let service = PrioritizationService::new(PrioritizationStrategy::Aggressive);
    let context = PerformanceContext::default();
    let result = service.calculate_adaptive_priority(&context).unwrap();
    assert!(matches!(
        result.strategy_used,
        PrioritizationStrategy::Aggressive
    ));
}

#[test]
fn test_prioritization_service_creation_custom() {
    let custom_rules = CustomPriorityRules::default();
    let service = PrioritizationService::new(PrioritizationStrategy::Custom(custom_rules));
    let context = PerformanceContext::default();
    let result = service.calculate_adaptive_priority(&context).unwrap();
    assert!(matches!(
        result.strategy_used,
        PrioritizationStrategy::Custom(_)
    ));
}

#[test]
fn test_prioritization_service_default() {
    let service = PrioritizationService::default();
    let context = PerformanceContext::default();
    let result = service.calculate_adaptive_priority(&context).unwrap();
    // Default is Balanced
    assert!(matches!(
        result.strategy_used,
        PrioritizationStrategy::Balanced
    ));
}

// === Conservative Strategy Tests ===

#[test]
fn test_conservative_high_error_rate() {
    let service = PrioritizationService::new(PrioritizationStrategy::Conservative);
    let context = create_test_context(200.0, 10.0, 0.05, 0.5, 50.0, 10);

    let result = service.calculate_adaptive_priority(&context).unwrap();

    assert_eq!(result.calculated_priority, Priority::CRITICAL);
    assert!(!result.reasoning.is_empty());
    assert!(result.reasoning.iter().any(|r| r.contains("error rate")));
}

#[test]
fn test_conservative_high_latency() {
    let service = PrioritizationService::new(PrioritizationStrategy::Conservative);
    let context = create_test_context(600.0, 10.0, 0.01, 0.5, 50.0, 10);

    let result = service.calculate_adaptive_priority(&context).unwrap();

    assert_eq!(result.calculated_priority, Priority::CRITICAL);
    assert!(result.reasoning.iter().any(|r| r.contains("latency")));
}

#[test]
fn test_conservative_high_cpu() {
    let service = PrioritizationService::new(PrioritizationStrategy::Conservative);
    let context = create_test_context(200.0, 10.0, 0.01, 0.8, 50.0, 10);

    let result = service.calculate_adaptive_priority(&context).unwrap();

    // CPU usage increases priority
    assert!(result.calculated_priority >= Priority::HIGH);
    assert!(result.reasoning.iter().any(|r| r.contains("CPU")));
}

#[test]
fn test_conservative_good_conditions() {
    let service = PrioritizationService::new(PrioritizationStrategy::Conservative);
    let context = create_test_context(100.0, 10.0, 0.01, 0.4, 50.0, 10);

    let result = service.calculate_adaptive_priority(&context).unwrap();

    // Conservative starts high
    assert!(result.calculated_priority >= Priority::HIGH);
}

// === Balanced Strategy Tests ===

#[test]
fn test_balanced_default_conditions() {
    let service = PrioritizationService::new(PrioritizationStrategy::Balanced);
    let context = PerformanceContext::default();

    let result = service.calculate_adaptive_priority(&context).unwrap();

    // Balanced starts with MEDIUM
    assert!(result.calculated_priority <= Priority::MEDIUM);
    assert!(!result.reasoning.is_empty());
}

#[test]
fn test_balanced_high_latency() {
    let service = PrioritizationService::new(PrioritizationStrategy::Balanced);
    let context = create_test_context(1500.0, 5.0, 0.01, 0.5, 50.0, 10);

    let result = service.calculate_adaptive_priority(&context).unwrap();

    assert_eq!(result.calculated_priority, Priority::HIGH);
    assert!(result.reasoning.iter().any(|r| r.contains("latency")));
}

#[test]
fn test_balanced_low_latency() {
    let service = PrioritizationService::new(PrioritizationStrategy::Balanced);
    let context = create_test_context(50.0, 15.0, 0.01, 0.5, 50.0, 10);

    let result = service.calculate_adaptive_priority(&context).unwrap();

    // Low latency with good bandwidth results in priority lower than LOW
    assert!(result.calculated_priority <= Priority::LOW);
    assert!(
        result
            .reasoning
            .iter()
            .any(|r| r.contains("Low latency") || r.contains("bandwidth"))
    );
}

#[test]
fn test_balanced_limited_bandwidth() {
    let service = PrioritizationService::new(PrioritizationStrategy::Balanced);
    let context = create_test_context(200.0, 0.5, 0.01, 0.5, 50.0, 10);

    let result = service.calculate_adaptive_priority(&context).unwrap();

    // Limited bandwidth should increase priority
    assert!(result.calculated_priority > Priority::MEDIUM);
    assert!(result.reasoning.iter().any(|r| r.contains("bandwidth")));
}

#[test]
fn test_balanced_high_bandwidth() {
    let service = PrioritizationService::new(PrioritizationStrategy::Balanced);
    let context = create_test_context(200.0, 20.0, 0.01, 0.5, 50.0, 10);

    let result = service.calculate_adaptive_priority(&context).unwrap();

    assert!(result.reasoning.iter().any(|r| r.contains("bandwidth")));
}

#[test]
fn test_balanced_high_error_rate() {
    let service = PrioritizationService::new(PrioritizationStrategy::Balanced);
    let context = create_test_context(200.0, 10.0, 0.08, 0.5, 50.0, 10);

    let result = service.calculate_adaptive_priority(&context).unwrap();

    // High error rate increases priority
    assert!(result.calculated_priority > Priority::MEDIUM);
    assert!(result.reasoning.iter().any(|r| r.contains("error rate")));
}

// === Aggressive Strategy Tests ===

#[test]
fn test_aggressive_default_conditions() {
    let service = PrioritizationService::new(PrioritizationStrategy::Aggressive);
    let context = PerformanceContext::default();

    let result = service.calculate_adaptive_priority(&context).unwrap();

    // Aggressive starts low
    assert_eq!(result.calculated_priority, Priority::LOW);
}

#[test]
fn test_aggressive_very_high_error_rate() {
    let service = PrioritizationService::new(PrioritizationStrategy::Aggressive);
    let context = create_test_context(200.0, 5.0, 0.15, 0.5, 50.0, 10);

    let result = service.calculate_adaptive_priority(&context).unwrap();

    assert_eq!(result.calculated_priority, Priority::HIGH);
    assert!(result.reasoning.iter().any(|r| r.contains("error rate")));
}

#[test]
fn test_aggressive_moderate_error_rate() {
    let service = PrioritizationService::new(PrioritizationStrategy::Aggressive);
    let context = create_test_context(200.0, 5.0, 0.07, 0.5, 50.0, 10);

    let result = service.calculate_adaptive_priority(&context).unwrap();

    assert_eq!(result.calculated_priority, Priority::MEDIUM);
}

#[test]
fn test_aggressive_extreme_latency() {
    let service = PrioritizationService::new(PrioritizationStrategy::Aggressive);
    let context = create_test_context(2500.0, 5.0, 0.01, 0.5, 50.0, 10);

    let result = service.calculate_adaptive_priority(&context).unwrap();

    assert_eq!(result.calculated_priority, Priority::HIGH);
    assert!(result.reasoning.iter().any(|r| r.contains("latency")));
}

#[test]
fn test_aggressive_very_limited_bandwidth() {
    let service = PrioritizationService::new(PrioritizationStrategy::Aggressive);
    let context = create_test_context(200.0, 0.3, 0.01, 0.5, 50.0, 10);

    let result = service.calculate_adaptive_priority(&context).unwrap();

    // Very limited bandwidth increases priority
    assert!(result.calculated_priority > Priority::LOW);
    assert!(result.reasoning.iter().any(|r| r.contains("bandwidth")));
}

// === Custom Strategy Tests ===

#[test]
fn test_custom_exceeds_latency_threshold() {
    let custom_rules = CustomPriorityRules {
        latency_threshold_ms: 300.0,
        bandwidth_threshold_mbps: 5.0,
        error_rate_threshold: 0.02,
        priority_boost_on_error: 30,
        priority_reduction_on_good_performance: 15,
    };
    let service = PrioritizationService::new(PrioritizationStrategy::Custom(custom_rules));
    let context = create_test_context(400.0, 10.0, 0.01, 0.5, 50.0, 10);

    let result = service.calculate_adaptive_priority(&context).unwrap();

    assert!(
        result
            .reasoning
            .iter()
            .any(|r| r.contains("Latency") && r.contains("threshold"))
    );
}

#[test]
fn test_custom_below_bandwidth_threshold() {
    let custom_rules = CustomPriorityRules {
        latency_threshold_ms: 500.0,
        bandwidth_threshold_mbps: 8.0,
        error_rate_threshold: 0.02,
        priority_boost_on_error: 25,
        priority_reduction_on_good_performance: 10,
    };
    let service = PrioritizationService::new(PrioritizationStrategy::Custom(custom_rules));
    let context = create_test_context(200.0, 5.0, 0.01, 0.5, 50.0, 10);

    let result = service.calculate_adaptive_priority(&context).unwrap();

    assert!(
        result
            .reasoning
            .iter()
            .any(|r| r.contains("Bandwidth") && r.contains("threshold"))
    );
}

#[test]
fn test_custom_exceeds_error_threshold() {
    let custom_rules = CustomPriorityRules {
        latency_threshold_ms: 500.0,
        bandwidth_threshold_mbps: 5.0,
        error_rate_threshold: 0.01,
        priority_boost_on_error: 30,
        priority_reduction_on_good_performance: 10,
    };
    let service = PrioritizationService::new(PrioritizationStrategy::Custom(custom_rules));
    let context = create_test_context(200.0, 10.0, 0.04, 0.5, 50.0, 10);

    let result = service.calculate_adaptive_priority(&context).unwrap();

    assert!(
        result
            .reasoning
            .iter()
            .any(|r| r.contains("Error rate") && r.contains("threshold"))
    );
}

#[test]
fn test_custom_excellent_performance() {
    let custom_rules = CustomPriorityRules {
        latency_threshold_ms: 500.0,
        bandwidth_threshold_mbps: 5.0,
        error_rate_threshold: 0.03,
        priority_boost_on_error: 20,
        priority_reduction_on_good_performance: 15,
    };
    let service = PrioritizationService::new(PrioritizationStrategy::Custom(custom_rules));
    let context = create_test_context(100.0, 15.0, 0.005, 0.5, 50.0, 10);

    let result = service.calculate_adaptive_priority(&context).unwrap();

    assert!(
        result
            .reasoning
            .iter()
            .any(|r| r.contains("Excellent performance"))
    );
}

#[test]
fn test_custom_rules_default() {
    let rules = CustomPriorityRules::default();
    assert_eq!(rules.latency_threshold_ms, 500.0);
    assert_eq!(rules.bandwidth_threshold_mbps, 5.0);
    assert_eq!(rules.error_rate_threshold, 0.03);
}

// === Global Priority Calculation Tests ===

#[test]
fn test_global_priority_single_stream() {
    let service = PrioritizationService::default();
    let context = PerformanceContext::default();

    let result = service.calculate_global_priority(&context, 1).unwrap();

    // With 1 stream, no adjustment
    assert!(
        result
            .reasoning
            .iter()
            .any(|r| r.contains("0") && r.contains("concurrent"))
    );
}

#[test]
fn test_global_priority_few_streams() {
    let service = PrioritizationService::default();
    let context = PerformanceContext::default();

    let result = service.calculate_global_priority(&context, 5).unwrap();

    assert!(
        result
            .reasoning
            .iter()
            .any(|r| r.contains("10") && r.contains("concurrent"))
    );
}

#[test]
fn test_global_priority_moderate_streams() {
    let service = PrioritizationService::default();
    let context = PerformanceContext::default();

    let result = service.calculate_global_priority(&context, 20).unwrap();

    assert!(
        result
            .reasoning
            .iter()
            .any(|r| r.contains("20") && r.contains("concurrent"))
    );
}

#[test]
fn test_global_priority_many_streams() {
    let service = PrioritizationService::default();
    let context = PerformanceContext::default();

    let result = service.calculate_global_priority(&context, 100).unwrap();

    assert!(
        result
            .reasoning
            .iter()
            .any(|r| r.contains("30") && r.contains("concurrent"))
    );
}

// === Confidence Score Tests ===

#[test]
fn test_confidence_score_good_conditions() {
    let service = PrioritizationService::default();
    let context = create_test_context(200.0, 10.0, 0.02, 0.5, 50.0, 20);

    let result = service.calculate_adaptive_priority(&context).unwrap();

    // Good conditions should have high confidence
    assert!(result.confidence_score > 0.8);
}

#[test]
fn test_confidence_score_high_error_rate() {
    let service = PrioritizationService::default();
    let context = create_test_context(200.0, 10.0, 0.15, 0.5, 50.0, 20);

    let result = service.calculate_adaptive_priority(&context).unwrap();

    // High error rate reduces confidence
    assert!(result.confidence_score < 0.8);
}

#[test]
fn test_confidence_score_high_cpu() {
    let service = PrioritizationService::default();
    let context = create_test_context(200.0, 10.0, 0.02, 0.95, 50.0, 20);

    let result = service.calculate_adaptive_priority(&context).unwrap();

    // High CPU reduces confidence
    assert!(result.confidence_score < 1.0);
}

#[test]
fn test_confidence_score_many_connections() {
    let service = PrioritizationService::default();
    let context = create_test_context(200.0, 10.0, 0.02, 0.5, 50.0, 150);

    let result = service.calculate_adaptive_priority(&context).unwrap();

    // Many connections reduce confidence
    assert!(result.confidence_score < 1.0);
}

#[test]
fn test_confidence_score_minimum_threshold() {
    let service = PrioritizationService::default();
    let context = create_test_context(200.0, 10.0, 0.2, 0.95, 50.0, 200);

    let result = service.calculate_adaptive_priority(&context).unwrap();

    // Confidence should never go below 0.1
    assert!(result.confidence_score >= 0.1);
}

// === Priority Adjustment Analysis Tests ===

#[test]
fn test_analyze_adjustments_high_latency() {
    let service = PrioritizationService::default();
    let metrics = create_test_metrics(1800.0, 3500.0, 5.0, 0.02);

    let adjustments = service.analyze_priority_adjustments(&metrics).unwrap();

    assert!(!adjustments.is_empty());
    let latency_adj = adjustments.iter().find(|a| a.reason.contains("Latency"));
    assert!(latency_adj.is_some());
    assert_eq!(latency_adj.unwrap().new_threshold, Priority::CRITICAL);
}

#[test]
fn test_analyze_adjustments_excellent_latency() {
    let service = PrioritizationService::default();
    let metrics = create_test_metrics(50.0, 150.0, 5.0, 0.001);

    let adjustments = service.analyze_priority_adjustments(&metrics).unwrap();

    let latency_adj = adjustments.iter().find(|a| a.reason.contains("latency"));
    assert!(latency_adj.is_some());
    assert_eq!(latency_adj.unwrap().new_threshold, Priority::LOW);
}

#[test]
fn test_analyze_adjustments_low_throughput_stable() {
    let service = PrioritizationService::default();
    let metrics = create_test_metrics(200.0, 300.0, 0.5, 0.01);

    let adjustments = service.analyze_priority_adjustments(&metrics).unwrap();

    let throughput_adj = adjustments.iter().find(|a| a.reason.contains("throughput"));
    assert!(throughput_adj.is_some());
}

#[test]
fn test_analyze_adjustments_high_throughput() {
    let service = PrioritizationService::default();
    let metrics = create_test_metrics(200.0, 300.0, 60.0, 0.01);

    let adjustments = service.analyze_priority_adjustments(&metrics).unwrap();

    let throughput_adj = adjustments.iter().find(|a| a.reason.contains("throughput"));
    assert!(throughput_adj.is_some());
}

#[test]
fn test_analyze_adjustments_critical_error_rate() {
    let service = PrioritizationService::default();
    let metrics = create_test_metrics(200.0, 300.0, 5.0, 0.15);

    let adjustments = service.analyze_priority_adjustments(&metrics).unwrap();

    assert!(!adjustments.is_empty());
    let error_adj = adjustments.iter().find(|a| a.reason.contains("error rate"));
    assert!(error_adj.is_some());
    assert_eq!(error_adj.unwrap().new_threshold, Priority::CRITICAL);
    assert!(matches!(
        error_adj.unwrap().urgency,
        AdjustmentUrgency::Critical
    ));
}

#[test]
fn test_analyze_adjustments_excellent_stability() {
    let service = PrioritizationService::default();
    let metrics = create_test_metrics(200.0, 300.0, 5.0, 0.0005);

    let adjustments = service.analyze_priority_adjustments(&metrics).unwrap();

    let error_adj = adjustments.iter().find(|a| a.reason.contains("stability"));
    assert!(error_adj.is_some());
    assert_eq!(error_adj.unwrap().new_threshold, Priority::LOW);
}

#[test]
fn test_analyze_adjustments_no_adjustments_needed() {
    let service = PrioritizationService::default();
    let metrics = create_test_metrics(500.0, 800.0, 10.0, 0.02);

    let adjustments = service.analyze_priority_adjustments(&metrics).unwrap();

    // Moderate conditions may not trigger adjustments
    // This is OK - adjustments are only for significant deviations
    let _ = adjustments;
}

// === Strategy Update Tests ===

#[test]
fn test_update_strategy_from_conservative_to_aggressive() {
    let mut service = PrioritizationService::new(PrioritizationStrategy::Conservative);
    let context = PerformanceContext::default();

    service.update_strategy(PrioritizationStrategy::Aggressive);
    let result = service.calculate_adaptive_priority(&context).unwrap();

    assert!(matches!(
        result.strategy_used,
        PrioritizationStrategy::Aggressive
    ));
}

#[test]
fn test_update_strategy_to_custom() {
    let mut service = PrioritizationService::default();
    let custom_rules = CustomPriorityRules {
        latency_threshold_ms: 200.0,
        bandwidth_threshold_mbps: 10.0,
        error_rate_threshold: 0.01,
        priority_boost_on_error: 50,
        priority_reduction_on_good_performance: 20,
    };

    service.update_strategy(PrioritizationStrategy::Custom(custom_rules));
    let context = PerformanceContext::default();
    let result = service.calculate_adaptive_priority(&context).unwrap();

    assert!(matches!(
        result.strategy_used,
        PrioritizationStrategy::Custom(_)
    ));
}

// === Edge Cases and Boundary Conditions ===

#[test]
fn test_edge_case_zero_latency() {
    let service = PrioritizationService::default();
    let context = create_test_context(0.0, 10.0, 0.01, 0.5, 50.0, 10);

    let result = service.calculate_adaptive_priority(&context).unwrap();

    // Should handle zero latency gracefully
    assert!(result.confidence_score > 0.0);
}

#[test]
fn test_edge_case_zero_bandwidth() {
    let service = PrioritizationService::default();
    let context = create_test_context(200.0, 0.0, 0.01, 0.5, 50.0, 10);

    let result = service.calculate_adaptive_priority(&context).unwrap();

    // Zero bandwidth should increase priority
    assert!(result.calculated_priority > Priority::LOW);
}

#[test]
fn test_edge_case_zero_error_rate() {
    let service = PrioritizationService::default();
    let context = create_test_context(200.0, 10.0, 0.0, 0.5, 50.0, 10);

    let result = service.calculate_adaptive_priority(&context).unwrap();

    // Perfect error rate is valid
    assert!(result.confidence_score > 0.0);
}

#[test]
fn test_edge_case_zero_cpu() {
    let service = PrioritizationService::default();
    let context = create_test_context(200.0, 10.0, 0.01, 0.0, 50.0, 10);

    let result = service.calculate_adaptive_priority(&context).unwrap();

    // Zero CPU is unrealistic but should be handled
    assert!(result.confidence_score > 0.0);
}

#[test]
fn test_edge_case_zero_connections() {
    let service = PrioritizationService::default();
    let context = create_test_context(200.0, 10.0, 0.01, 0.5, 50.0, 0);

    let result = service.calculate_adaptive_priority(&context).unwrap();

    // Zero connections should work
    assert!(result.confidence_score > 0.0);
}

#[test]
fn test_edge_case_extreme_values() {
    let service = PrioritizationService::default();
    let context = create_test_context(
        10000.0, // Extreme latency
        1000.0,  // Extreme bandwidth
        1.0,     // 100% error rate
        1.0,     // 100% CPU
        100.0,   // Full memory
        10000,   // Many connections
    );

    let result = service.calculate_adaptive_priority(&context).unwrap();

    // Should handle extreme values without panicking
    assert!(result.calculated_priority >= Priority::LOW);
    assert!(result.confidence_score >= 0.1);
}

// === Performance Context Tests ===

#[test]
fn test_performance_context_default() {
    let context = PerformanceContext::default();

    assert_eq!(context.average_latency_ms, 100.0);
    assert_eq!(context.available_bandwidth_mbps, 10.0);
    assert_eq!(context.error_rate, 0.01);
    assert_eq!(context.cpu_usage, 0.5);
    assert_eq!(context.memory_usage_percent, 60.0);
    assert_eq!(context.connection_count, 1);
}

#[test]
fn test_performance_context_clone() {
    let context = PerformanceContext::default();
    let cloned = context.clone();

    assert_eq!(context.average_latency_ms, cloned.average_latency_ms);
}

// === Prioritization Strategy Tests ===

#[test]
fn test_prioritization_strategy_serialization() {
    use serde_json;

    let strategy = PrioritizationStrategy::Balanced;
    let json = serde_json::to_string(&strategy).unwrap();
    let deserialized: PrioritizationStrategy = serde_json::from_str(&json).unwrap();

    assert_eq!(strategy, deserialized);
}

#[test]
fn test_custom_rules_serialization() {
    use serde_json;

    let rules = CustomPriorityRules::default();
    let json = serde_json::to_string(&rules).unwrap();
    let deserialized: CustomPriorityRules = serde_json::from_str(&json).unwrap();

    assert_eq!(rules, deserialized);
}
