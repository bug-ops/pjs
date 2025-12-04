//! Comprehensive tests for OptimizationService
//!
//! This test suite aims to achieve 70%+ coverage by testing:
//! - All public APIs
//! - Error paths and edge cases
//! - Boundary conditions
//! - State transitions
//! - Strategy optimization logic

use pjson_rs::application::services::{
    optimization_service::{
        AdjustmentType, OptimizationService, OptimizationStrategy, StreamingUseCase,
    },
    performance_analysis_service::{
        ErrorAnalysis, LatencyAnalysis, PerformanceAnalysisReport, ResourceAnalysis,
        ThroughputAnalysis,
    },
    prioritization_service::PerformanceContext,
};
use pjson_rs::application::shared::AdjustmentUrgency;
use pjson_rs::domain::value_objects::{JsonData, Priority, StreamId};
use std::collections::HashMap;
use std::time::SystemTime;

// Test fixtures and helpers

fn create_test_performance_context(
    latency: f64,
    bandwidth: f64,
    error_rate: f64,
    cpu_usage: f64,
) -> PerformanceContext {
    PerformanceContext {
        average_latency_ms: latency,
        available_bandwidth_mbps: bandwidth,
        error_rate,
        cpu_usage,
        memory_usage_percent: 50.0,
        connection_count: 10,
    }
}

fn create_test_performance_report(
    avg_latency: f64,
    avg_throughput: f64,
    error_rate: f64,
    cpu_usage: f64,
) -> PerformanceAnalysisReport {
    PerformanceAnalysisReport {
        timestamp: SystemTime::now(),
        overall_score: 0.75,
        latency_analysis: LatencyAnalysis {
            average: avg_latency,
            p50: avg_latency * 0.8,
            p95: avg_latency * 1.5,
            p99: avg_latency * 2.0,
            min: avg_latency * 0.5,
            max: avg_latency * 2.5,
            sample_count: 100,
        },
        throughput_analysis: ThroughputAnalysis {
            average_mbps: avg_throughput,
            frames_per_second: 60.0,
            total_bytes: 1024000,
            total_frames: 1000,
            sample_count: 100,
        },
        error_analysis: ErrorAnalysis {
            error_rate,
            total_errors: (error_rate * 1000.0) as usize,
            error_type_distribution: HashMap::new(),
            severity_distribution: HashMap::new(),
        },
        resource_analysis: ResourceAnalysis {
            current_cpu_usage: cpu_usage,
            average_cpu_usage: cpu_usage * 0.9,
            current_memory_usage: 256 * 1024 * 1024, // 256MB in bytes
            average_memory_usage: 200 * 1024 * 1024,
            network_bandwidth_mbps: avg_throughput,
            active_connections: 10,
        },
        issues: vec![],
        recommendations: vec![],
    }
}

// === Basic Service Creation and Strategy Selection ===

#[test]
fn test_optimization_service_creation() {
    let service = OptimizationService::new();
    // Service should be created successfully with empty custom strategies
    let _ = service;
}

#[test]
fn test_default_trait() {
    let service = OptimizationService::default();
    let _ = service;
}

#[test]
fn test_realtime_dashboard_strategy() {
    let service = OptimizationService::new();
    let strategy = service
        .get_strategy_for_use_case(&StreamingUseCase::RealTimeDashboard)
        .unwrap();

    assert_eq!(strategy.priority_threshold, Priority::HIGH);
    assert_eq!(strategy.batch_size, 5);
    assert!(!strategy.compression_enabled);
    assert!(strategy.adaptive_quality);
    assert_eq!(strategy.target_latency_ms, 100.0);
}

#[test]
fn test_bulk_transfer_strategy() {
    let service = OptimizationService::new();
    let strategy = service
        .get_strategy_for_use_case(&StreamingUseCase::BulkDataTransfer)
        .unwrap();

    assert_eq!(strategy.priority_threshold, Priority::MEDIUM);
    assert_eq!(strategy.batch_size, 20);
    assert!(strategy.compression_enabled);
    assert!(!strategy.adaptive_quality);
    assert_eq!(strategy.target_throughput_mbps, 50.0);
}

#[test]
fn test_mobile_app_strategy() {
    let service = OptimizationService::new();
    let strategy = service
        .get_strategy_for_use_case(&StreamingUseCase::MobileApp)
        .unwrap();

    assert_eq!(strategy.priority_threshold, Priority::HIGH);
    assert_eq!(strategy.max_frame_size, 8 * 1024);
    assert_eq!(strategy.batch_size, 3);
    assert!(strategy.compression_enabled);
    assert!(strategy.adaptive_quality);
}

#[test]
fn test_progressive_web_app_strategy() {
    let service = OptimizationService::new();
    let strategy = service
        .get_strategy_for_use_case(&StreamingUseCase::ProgressiveWebApp)
        .unwrap();

    assert_eq!(strategy.priority_threshold, Priority::CRITICAL);
    assert_eq!(strategy.max_frame_size, 32 * 1024);
    assert_eq!(strategy.batch_size, 8);
    assert!(strategy.compression_enabled);
    assert!(strategy.adaptive_quality);
}

#[test]
fn test_iot_device_strategy() {
    let service = OptimizationService::new();
    let strategy = service
        .get_strategy_for_use_case(&StreamingUseCase::IoTDevice)
        .unwrap();

    assert_eq!(strategy.priority_threshold, Priority::CRITICAL);
    assert_eq!(strategy.max_frame_size, 4 * 1024);
    assert_eq!(strategy.batch_size, 2);
    assert!(strategy.compression_enabled);
    assert!(!strategy.adaptive_quality);
}

#[test]
fn test_live_streaming_strategy() {
    let service = OptimizationService::new();
    let strategy = service
        .get_strategy_for_use_case(&StreamingUseCase::LiveStreaming)
        .unwrap();

    assert_eq!(strategy.priority_threshold, Priority::HIGH);
    assert_eq!(strategy.max_frame_size, 64 * 1024);
    assert_eq!(strategy.batch_size, 10);
    assert!(strategy.compression_enabled);
    assert!(strategy.adaptive_quality);
}

#[test]
fn test_all_use_case_strategies_return_valid_values() {
    let service = OptimizationService::new();

    let use_cases = vec![
        StreamingUseCase::RealTimeDashboard,
        StreamingUseCase::BulkDataTransfer,
        StreamingUseCase::MobileApp,
        StreamingUseCase::ProgressiveWebApp,
        StreamingUseCase::IoTDevice,
        StreamingUseCase::LiveStreaming,
    ];

    for use_case in use_cases {
        let strategy = service.get_strategy_for_use_case(&use_case).unwrap();

        // Validate all strategies have reasonable values
        assert!(strategy.max_frame_size > 0);
        assert!(strategy.batch_size > 0);
        assert!(strategy.target_latency_ms > 0.0);
        assert!(strategy.target_throughput_mbps > 0.0);
        assert!(!strategy.description.is_empty());
    }
}

// === Custom Strategy Management ===

#[test]
fn test_register_custom_strategy() {
    let mut service = OptimizationService::new();

    let custom_strategy = OptimizationStrategy {
        priority_threshold: Priority::CRITICAL,
        max_frame_size: 8 * 1024,
        batch_size: 3,
        compression_enabled: false,
        adaptive_quality: false,
        description: "Ultra low-latency gaming strategy".to_string(),
        target_latency_ms: 50.0,
        target_throughput_mbps: 30.0,
    };

    service
        .register_custom_strategy("ultra_gaming".to_string(), custom_strategy.clone())
        .unwrap();

    let retrieved = service
        .get_strategy_for_use_case(&StreamingUseCase::Custom("ultra_gaming".to_string()))
        .unwrap();

    assert_eq!(retrieved.priority_threshold, Priority::CRITICAL);
    assert_eq!(retrieved.max_frame_size, 8 * 1024);
    assert_eq!(retrieved.target_latency_ms, 50.0);
}

#[test]
fn test_custom_strategy_not_found() {
    let service = OptimizationService::new();

    let result =
        service.get_strategy_for_use_case(&StreamingUseCase::Custom("nonexistent".to_string()));

    assert!(result.is_err());
    match result {
        Err(e) => assert!(e.to_string().contains("not found")),
        Ok(_) => panic!("Expected error for nonexistent strategy"),
    }
}

#[test]
fn test_register_multiple_custom_strategies() {
    let mut service = OptimizationService::new();

    let strategy1 = OptimizationStrategy {
        priority_threshold: Priority::HIGH,
        max_frame_size: 16 * 1024,
        batch_size: 5,
        compression_enabled: true,
        adaptive_quality: true,
        description: "Strategy 1".to_string(),
        target_latency_ms: 100.0,
        target_throughput_mbps: 10.0,
    };

    let strategy2 = OptimizationStrategy {
        priority_threshold: Priority::MEDIUM,
        max_frame_size: 32 * 1024,
        batch_size: 10,
        compression_enabled: false,
        adaptive_quality: false,
        description: "Strategy 2".to_string(),
        target_latency_ms: 200.0,
        target_throughput_mbps: 20.0,
    };

    service
        .register_custom_strategy("custom1".to_string(), strategy1)
        .unwrap();
    service
        .register_custom_strategy("custom2".to_string(), strategy2)
        .unwrap();

    let retrieved1 = service
        .get_strategy_for_use_case(&StreamingUseCase::Custom("custom1".to_string()))
        .unwrap();
    let retrieved2 = service
        .get_strategy_for_use_case(&StreamingUseCase::Custom("custom2".to_string()))
        .unwrap();

    assert_eq!(retrieved1.batch_size, 5);
    assert_eq!(retrieved2.batch_size, 10);
}

#[test]
fn test_overwrite_custom_strategy() {
    let mut service = OptimizationService::new();

    let strategy1 = OptimizationStrategy {
        priority_threshold: Priority::HIGH,
        max_frame_size: 16 * 1024,
        batch_size: 5,
        compression_enabled: true,
        adaptive_quality: true,
        description: "Original".to_string(),
        target_latency_ms: 100.0,
        target_throughput_mbps: 10.0,
    };

    let strategy2 = OptimizationStrategy {
        priority_threshold: Priority::CRITICAL,
        max_frame_size: 8 * 1024,
        batch_size: 2,
        compression_enabled: false,
        adaptive_quality: false,
        description: "Replacement".to_string(),
        target_latency_ms: 50.0,
        target_throughput_mbps: 5.0,
    };

    service
        .register_custom_strategy("test".to_string(), strategy1)
        .unwrap();
    service
        .register_custom_strategy("test".to_string(), strategy2)
        .unwrap();

    let retrieved = service
        .get_strategy_for_use_case(&StreamingUseCase::Custom("test".to_string()))
        .unwrap();

    assert_eq!(retrieved.batch_size, 2);
    assert_eq!(retrieved.description, "Replacement");
}

// === Strategy Optimization Based on Performance Context ===

#[test]
fn test_optimize_strategy_high_error_rate() {
    let service = OptimizationService::new();
    let base_strategy = service
        .get_strategy_for_use_case(&StreamingUseCase::RealTimeDashboard)
        .unwrap();

    let context = create_test_performance_context(200.0, 10.0, 0.06, 0.5);

    let optimized = service
        .optimize_strategy_for_context(base_strategy.clone(), &context)
        .unwrap();

    // High error rate should increase priority
    assert!(optimized.priority_threshold.value() > base_strategy.priority_threshold.value());
}

#[test]
fn test_optimize_strategy_low_error_rate_low_latency() {
    let service = OptimizationService::new();
    let base_strategy = service
        .get_strategy_for_use_case(&StreamingUseCase::RealTimeDashboard)
        .unwrap();

    let context = create_test_performance_context(50.0, 15.0, 0.005, 0.3);

    let optimized = service
        .optimize_strategy_for_context(base_strategy.clone(), &context)
        .unwrap();

    // Low error rate and low latency should decrease priority
    assert!(optimized.priority_threshold.value() <= base_strategy.priority_threshold.value());
}

#[test]
fn test_optimize_strategy_high_latency() {
    let service = OptimizationService::new();
    let base_strategy = service
        .get_strategy_for_use_case(&StreamingUseCase::RealTimeDashboard)
        .unwrap();

    let context = create_test_performance_context(1500.0, 10.0, 0.02, 0.5);

    let optimized = service
        .optimize_strategy_for_context(base_strategy.clone(), &context)
        .unwrap();

    // High latency should reduce batch size
    assert!(optimized.batch_size < base_strategy.batch_size);
}

#[test]
fn test_optimize_strategy_low_latency_high_bandwidth() {
    let service = OptimizationService::new();
    let base_strategy = service
        .get_strategy_for_use_case(&StreamingUseCase::RealTimeDashboard)
        .unwrap();

    let context = create_test_performance_context(50.0, 25.0, 0.01, 0.3);

    let optimized = service
        .optimize_strategy_for_context(base_strategy.clone(), &context)
        .unwrap();

    // Low latency and high bandwidth should increase batch size
    assert!(optimized.batch_size > base_strategy.batch_size);
}

#[test]
fn test_optimize_strategy_low_bandwidth() {
    let service = OptimizationService::new();
    let base_strategy = service
        .get_strategy_for_use_case(&StreamingUseCase::RealTimeDashboard)
        .unwrap();

    let context = create_test_performance_context(200.0, 1.5, 0.02, 0.5);

    let optimized = service
        .optimize_strategy_for_context(base_strategy.clone(), &context)
        .unwrap();

    // Low bandwidth should reduce frame size and enable compression
    assert!(optimized.max_frame_size < base_strategy.max_frame_size);
    assert!(optimized.compression_enabled);
}

#[test]
fn test_optimize_strategy_high_bandwidth() {
    let service = OptimizationService::new();
    let base_strategy = service
        .get_strategy_for_use_case(&StreamingUseCase::RealTimeDashboard)
        .unwrap();

    let context = create_test_performance_context(100.0, 25.0, 0.01, 0.4);

    let optimized = service
        .optimize_strategy_for_context(base_strategy.clone(), &context)
        .unwrap();

    // High bandwidth should increase frame size
    assert!(optimized.max_frame_size > base_strategy.max_frame_size);
}

#[test]
fn test_optimize_strategy_high_cpu_usage() {
    let service = OptimizationService::new();
    let base_strategy = OptimizationStrategy {
        priority_threshold: Priority::MEDIUM,
        max_frame_size: 32 * 1024,
        batch_size: 10,
        compression_enabled: true,
        adaptive_quality: true,
        description: "Test".to_string(),
        target_latency_ms: 200.0,
        target_throughput_mbps: 10.0,
    };

    let context = create_test_performance_context(200.0, 10.0, 0.02, 0.85);

    let optimized = service
        .optimize_strategy_for_context(base_strategy, &context)
        .unwrap();

    // High CPU usage should disable adaptive quality
    assert!(!optimized.adaptive_quality);
}

#[test]
fn test_optimize_strategy_combined_factors() {
    let service = OptimizationService::new();
    let base_strategy = service
        .get_strategy_for_use_case(&StreamingUseCase::MobileApp)
        .unwrap();

    // Poor conditions: high latency, low bandwidth, high error rate, high CPU
    let poor_context = create_test_performance_context(2000.0, 0.5, 0.08, 0.9);

    let optimized = service
        .optimize_strategy_for_context(base_strategy, &poor_context)
        .unwrap();

    // Multiple optimizations should be applied
    assert!(optimized.compression_enabled);
    assert!(!optimized.adaptive_quality); // Disabled due to high CPU
}

// === Strategy Adjustment Recommendations ===

#[test]
fn test_recommend_adjustments_high_latency() {
    let service = OptimizationService::new();
    let strategy = service
        .get_strategy_for_use_case(&StreamingUseCase::RealTimeDashboard)
        .unwrap();

    let report = create_test_performance_report(200.0, 10.0, 0.02, 0.5);

    let recommendations = service
        .recommend_strategy_adjustments(&strategy, &report)
        .unwrap();

    assert!(!recommendations.is_empty());
    let has_priority_increase = recommendations
        .iter()
        .any(|r| r.adjustment_type == AdjustmentType::PriorityIncrease);
    assert!(has_priority_increase);
}

#[test]
fn test_recommend_adjustments_low_throughput() {
    let service = OptimizationService::new();
    let strategy = service
        .get_strategy_for_use_case(&StreamingUseCase::BulkDataTransfer)
        .unwrap();

    let report = create_test_performance_report(500.0, 30.0, 0.02, 0.5);

    let recommendations = service
        .recommend_strategy_adjustments(&strategy, &report)
        .unwrap();

    let has_batch_increase = recommendations
        .iter()
        .any(|r| r.adjustment_type == AdjustmentType::BatchSizeIncrease);
    assert!(has_batch_increase);
}

#[test]
fn test_recommend_adjustments_high_error_rate() {
    let service = OptimizationService::new();
    let strategy = service
        .get_strategy_for_use_case(&StreamingUseCase::RealTimeDashboard)
        .unwrap();

    let report = create_test_performance_report(200.0, 10.0, 0.08, 0.5);

    let recommendations = service
        .recommend_strategy_adjustments(&strategy, &report)
        .unwrap();

    let has_quality_reduction = recommendations
        .iter()
        .any(|r| r.adjustment_type == AdjustmentType::QualityReduction);
    assert!(has_quality_reduction);

    // Should have high urgency
    let quality_rec = recommendations
        .iter()
        .find(|r| r.adjustment_type == AdjustmentType::QualityReduction)
        .unwrap();
    assert_eq!(quality_rec.urgency, AdjustmentUrgency::High);
}

#[test]
fn test_recommend_adjustments_high_cpu() {
    let service = OptimizationService::new();
    let strategy = service
        .get_strategy_for_use_case(&StreamingUseCase::RealTimeDashboard)
        .unwrap();

    let report = create_test_performance_report(200.0, 10.0, 0.02, 0.85);

    let recommendations = service
        .recommend_strategy_adjustments(&strategy, &report)
        .unwrap();

    let has_compression_disable = recommendations
        .iter()
        .any(|r| r.adjustment_type == AdjustmentType::CompressionDisable);
    assert!(has_compression_disable);
}

#[test]
fn test_recommend_adjustments_good_performance() {
    let service = OptimizationService::new();
    let strategy = service
        .get_strategy_for_use_case(&StreamingUseCase::RealTimeDashboard)
        .unwrap();

    let report = create_test_performance_report(50.0, 15.0, 0.005, 0.3);

    let recommendations = service
        .recommend_strategy_adjustments(&strategy, &report)
        .unwrap();

    // Good performance should result in few or no recommendations
    assert!(recommendations.is_empty() || recommendations.len() <= 1);
}

#[test]
fn test_recommendation_confidence_scores() {
    let service = OptimizationService::new();
    let strategy = service
        .get_strategy_for_use_case(&StreamingUseCase::RealTimeDashboard)
        .unwrap();

    let report = create_test_performance_report(200.0, 10.0, 0.08, 0.5);

    let recommendations = service
        .recommend_strategy_adjustments(&strategy, &report)
        .unwrap();

    // All recommendations should have confidence between 0 and 1
    for rec in recommendations {
        assert!(rec.confidence >= 0.0 && rec.confidence <= 1.0);
    }
}

// === Edge Cases and Boundary Conditions ===

#[test]
fn test_streaming_use_case_equality() {
    assert_eq!(
        StreamingUseCase::RealTimeDashboard,
        StreamingUseCase::RealTimeDashboard
    );
    assert_ne!(
        StreamingUseCase::RealTimeDashboard,
        StreamingUseCase::MobileApp
    );
}

#[test]
fn test_custom_use_case_equality() {
    assert_eq!(
        StreamingUseCase::Custom("test".to_string()),
        StreamingUseCase::Custom("test".to_string())
    );
    assert_ne!(
        StreamingUseCase::Custom("test1".to_string()),
        StreamingUseCase::Custom("test2".to_string())
    );
}

#[test]
fn test_adjustment_type_equality() {
    assert_eq!(
        AdjustmentType::PriorityIncrease,
        AdjustmentType::PriorityIncrease
    );
    assert_ne!(
        AdjustmentType::PriorityIncrease,
        AdjustmentType::PriorityDecrease
    );
}

#[test]
fn test_strategy_with_extreme_values() {
    let mut service = OptimizationService::new();

    let extreme_strategy = OptimizationStrategy {
        priority_threshold: Priority::CRITICAL,
        max_frame_size: 1, // Very small
        batch_size: 1,     // Very small
        compression_enabled: true,
        adaptive_quality: true,
        description: "Extreme strategy".to_string(),
        target_latency_ms: 1.0,
        target_throughput_mbps: 0.1,
    };

    service
        .register_custom_strategy("extreme".to_string(), extreme_strategy)
        .unwrap();

    let retrieved = service
        .get_strategy_for_use_case(&StreamingUseCase::Custom("extreme".to_string()))
        .unwrap();

    assert_eq!(retrieved.max_frame_size, 1);
    assert_eq!(retrieved.batch_size, 1);
}

#[test]
fn test_optimization_with_zero_latency() {
    let service = OptimizationService::new();
    let strategy = service
        .get_strategy_for_use_case(&StreamingUseCase::RealTimeDashboard)
        .unwrap();

    let context = create_test_performance_context(0.0, 10.0, 0.01, 0.5);

    let optimized = service
        .optimize_strategy_for_context(strategy, &context)
        .unwrap();

    // Should handle zero latency gracefully
    assert!(optimized.batch_size > 0);
}

#[test]
fn test_optimization_with_zero_bandwidth() {
    let service = OptimizationService::new();
    let strategy = service
        .get_strategy_for_use_case(&StreamingUseCase::RealTimeDashboard)
        .unwrap();

    let context = create_test_performance_context(100.0, 0.0, 0.01, 0.5);

    let optimized = service
        .optimize_strategy_for_context(strategy, &context)
        .unwrap();

    // Should handle zero bandwidth gracefully
    assert!(optimized.compression_enabled);
}

#[test]
fn test_optimization_with_max_error_rate() {
    let service = OptimizationService::new();
    let strategy = service
        .get_strategy_for_use_case(&StreamingUseCase::RealTimeDashboard)
        .unwrap();

    let context = create_test_performance_context(100.0, 10.0, 1.0, 0.5);

    let optimized = service
        .optimize_strategy_for_context(strategy, &context)
        .unwrap();

    // Should handle 100% error rate
    assert!(optimized.priority_threshold.value() > Priority::MEDIUM.value());
}

#[test]
fn test_optimization_with_max_cpu() {
    let service = OptimizationService::new();
    let strategy = OptimizationStrategy {
        priority_threshold: Priority::MEDIUM,
        max_frame_size: 32 * 1024,
        batch_size: 10,
        compression_enabled: true,
        adaptive_quality: true,
        description: "Test".to_string(),
        target_latency_ms: 200.0,
        target_throughput_mbps: 10.0,
    };

    let context = create_test_performance_context(100.0, 10.0, 0.01, 1.0);

    let optimized = service
        .optimize_strategy_for_context(strategy, &context)
        .unwrap();

    // Should disable adaptive quality at 100% CPU
    assert!(!optimized.adaptive_quality);
}

#[test]
fn test_empty_custom_strategy_name() {
    let mut service = OptimizationService::new();

    let strategy = OptimizationStrategy {
        priority_threshold: Priority::HIGH,
        max_frame_size: 16 * 1024,
        batch_size: 5,
        compression_enabled: true,
        adaptive_quality: true,
        description: "Empty name test".to_string(),
        target_latency_ms: 100.0,
        target_throughput_mbps: 10.0,
    };

    service
        .register_custom_strategy("".to_string(), strategy)
        .unwrap();

    let retrieved = service
        .get_strategy_for_use_case(&StreamingUseCase::Custom("".to_string()))
        .unwrap();

    assert_eq!(retrieved.description, "Empty name test");
}

#[test]
fn test_strategy_cloning() {
    let service = OptimizationService::new();
    let strategy1 = service
        .get_strategy_for_use_case(&StreamingUseCase::RealTimeDashboard)
        .unwrap();

    let strategy2 = strategy1.clone();

    assert_eq!(strategy1.batch_size, strategy2.batch_size);
    assert_eq!(strategy1.max_frame_size, strategy2.max_frame_size);
    assert_eq!(strategy1.priority_threshold, strategy2.priority_threshold);
}

// === Calculate Optimization Metrics Tests ===

#[test]
fn test_calculate_metrics_empty_frames() {
    let service = OptimizationService::new();
    let strategy = service
        .get_strategy_for_use_case(&StreamingUseCase::RealTimeDashboard)
        .unwrap();
    let context = create_test_performance_context(100.0, 10.0, 0.01, 0.5);

    let frames = vec![]; // Empty frames

    let metrics = service
        .calculate_optimization_metrics(&strategy, &frames, &context)
        .unwrap();

    assert_eq!(metrics.efficiency_score, 0.0);
    assert_eq!(metrics.quality_score, 0.0);
    assert!(metrics.latency_improvement >= 0.0);
    assert!(metrics.throughput_improvement >= 0.0);
}

#[test]
fn test_calculate_metrics_with_frames() {
    use pjson_rs::domain::entities::Frame;

    let service = OptimizationService::new();
    let strategy = service
        .get_strategy_for_use_case(&StreamingUseCase::RealTimeDashboard)
        .unwrap();
    let context = create_test_performance_context(100.0, 10.0, 0.01, 0.5);

    let stream_id = StreamId::new();
    let frames = vec![
        Frame::skeleton(stream_id, 0, JsonData::Null),
        Frame::complete(stream_id, 1, None),
    ];

    let metrics = service
        .calculate_optimization_metrics(&strategy, &frames, &context)
        .unwrap();

    assert!(metrics.efficiency_score >= 0.0 && metrics.efficiency_score <= 1.0);
    assert!(metrics.quality_score >= 0.0 && metrics.quality_score <= 1.0);
    assert!(metrics.latency_improvement >= 0.0 && metrics.latency_improvement <= 1.0);
    assert!(metrics.throughput_improvement >= 0.0 && metrics.throughput_improvement <= 1.0);
    assert!(metrics.resource_utilization >= 0.0 && metrics.resource_utilization <= 1.0);
}

#[test]
fn test_calculate_metrics_high_priority_frames() {
    use pjson_rs::domain::entities::frame::{Frame, FramePatch};
    use pjson_rs::domain::value_objects::JsonPath;

    let service = OptimizationService::new();
    let strategy = service
        .get_strategy_for_use_case(&StreamingUseCase::RealTimeDashboard)
        .unwrap();
    let context = create_test_performance_context(100.0, 10.0, 0.01, 0.5);

    let stream_id = StreamId::new();
    let patch1 = FramePatch::set(JsonPath::root(), JsonData::String("value1".into()));
    let patch2 = FramePatch::set(JsonPath::root(), JsonData::String("value2".into()));
    let frames = vec![
        Frame::skeleton(stream_id, 0, JsonData::Null),
        Frame::patch(stream_id, 1, Priority::CRITICAL, vec![patch1]).unwrap(),
        Frame::patch(stream_id, 2, Priority::HIGH, vec![patch2]).unwrap(),
        Frame::complete(stream_id, 3, None),
    ];

    let metrics = service
        .calculate_optimization_metrics(&strategy, &frames, &context)
        .unwrap();

    // High priority frames should result in good quality score
    assert!(metrics.quality_score > 0.5);
}

#[test]
fn test_calculate_metrics_latency_improvement_when_exceeding_target() {
    let service = OptimizationService::new();
    let strategy = service
        .get_strategy_for_use_case(&StreamingUseCase::RealTimeDashboard)
        .unwrap();

    // High latency exceeding target
    let context = create_test_performance_context(500.0, 10.0, 0.01, 0.5);

    let stream_id = StreamId::new();
    let frames = vec![pjson_rs::domain::entities::Frame::skeleton(
        stream_id,
        0,
        JsonData::Null,
    )];

    let metrics = service
        .calculate_optimization_metrics(&strategy, &frames, &context)
        .unwrap();

    // Should show improvement potential when latency exceeds target
    assert!(metrics.latency_improvement > 0.0);
}

#[test]
fn test_calculate_metrics_throughput_improvement_low_bandwidth() {
    let service = OptimizationService::new();
    let strategy = service
        .get_strategy_for_use_case(&StreamingUseCase::BulkDataTransfer)
        .unwrap();

    // Low bandwidth compared to target
    let context = create_test_performance_context(100.0, 2.0, 0.01, 0.5);

    let stream_id = StreamId::new();
    let frames = vec![pjson_rs::domain::entities::Frame::skeleton(
        stream_id,
        0,
        JsonData::Null,
    )];

    let metrics = service
        .calculate_optimization_metrics(&strategy, &frames, &context)
        .unwrap();

    assert!(metrics.throughput_improvement > 0.0);
}

#[test]
fn test_calculate_metrics_resource_utilization_with_compression() {
    let service = OptimizationService::new();
    let mut strategy = service
        .get_strategy_for_use_case(&StreamingUseCase::RealTimeDashboard)
        .unwrap();
    strategy.compression_enabled = true;
    strategy.adaptive_quality = true;

    let context = create_test_performance_context(100.0, 10.0, 0.01, 0.5);

    let stream_id = StreamId::new();
    let frames = vec![pjson_rs::domain::entities::Frame::skeleton(
        stream_id,
        0,
        JsonData::Null,
    )];

    let metrics = service
        .calculate_optimization_metrics(&strategy, &frames, &context)
        .unwrap();

    // Compression and adaptive quality increase resource utilization
    assert!(metrics.resource_utilization > context.cpu_usage);
}

// === Additional Recommendation Tests ===

#[test]
fn test_recommend_multiple_issues() {
    let service = OptimizationService::new();
    let strategy = service
        .get_strategy_for_use_case(&StreamingUseCase::RealTimeDashboard)
        .unwrap();

    // Multiple issues: high latency, low throughput, high error rate, high CPU
    let report = create_test_performance_report(200.0, 3.0, 0.08, 0.85);

    let recommendations = service
        .recommend_strategy_adjustments(&strategy, &report)
        .unwrap();

    // Should have multiple recommendations
    assert!(recommendations.len() >= 2);
}

#[test]
fn test_recommend_latency_urgency() {
    let service = OptimizationService::new();
    let strategy = service
        .get_strategy_for_use_case(&StreamingUseCase::RealTimeDashboard)
        .unwrap();

    let report = create_test_performance_report(300.0, 10.0, 0.02, 0.5);

    let recommendations = service
        .recommend_strategy_adjustments(&strategy, &report)
        .unwrap();

    // High latency for real-time dashboard should have high urgency
    if let Some(rec) = recommendations
        .iter()
        .find(|r| r.adjustment_type == AdjustmentType::PriorityIncrease)
    {
        assert_eq!(rec.urgency, AdjustmentUrgency::High);
    }
}

#[test]
fn test_recommend_throughput_urgency() {
    let service = OptimizationService::new();
    let strategy = service
        .get_strategy_for_use_case(&StreamingUseCase::BulkDataTransfer)
        .unwrap();

    let report = create_test_performance_report(500.0, 30.0, 0.02, 0.5);

    let recommendations = service
        .recommend_strategy_adjustments(&strategy, &report)
        .unwrap();

    if let Some(rec) = recommendations
        .iter()
        .find(|r| r.adjustment_type == AdjustmentType::BatchSizeIncrease)
    {
        assert_eq!(rec.urgency, AdjustmentUrgency::Medium);
    }
}

#[test]
fn test_recommend_cpu_urgency() {
    let service = OptimizationService::new();
    let strategy = service
        .get_strategy_for_use_case(&StreamingUseCase::RealTimeDashboard)
        .unwrap();

    let report = create_test_performance_report(200.0, 10.0, 0.02, 0.90);

    let recommendations = service
        .recommend_strategy_adjustments(&strategy, &report)
        .unwrap();

    if let Some(rec) = recommendations
        .iter()
        .find(|r| r.adjustment_type == AdjustmentType::CompressionDisable)
    {
        assert_eq!(rec.urgency, AdjustmentUrgency::Medium);
    }
}

#[test]
fn test_recommendation_descriptions() {
    let service = OptimizationService::new();
    let strategy = service
        .get_strategy_for_use_case(&StreamingUseCase::RealTimeDashboard)
        .unwrap();

    let report = create_test_performance_report(200.0, 10.0, 0.08, 0.5);

    let recommendations = service
        .recommend_strategy_adjustments(&strategy, &report)
        .unwrap();

    for rec in recommendations {
        assert!(!rec.description.is_empty());
        assert!(!rec.expected_impact.is_empty());
    }
}

// === Optimization Context Boundary Tests ===

#[test]
fn test_optimize_with_minimum_latency() {
    let service = OptimizationService::new();
    let strategy = service
        .get_strategy_for_use_case(&StreamingUseCase::RealTimeDashboard)
        .unwrap();

    let context = create_test_performance_context(1.0, 10.0, 0.01, 0.5);

    let optimized = service
        .optimize_strategy_for_context(strategy, &context)
        .unwrap();

    assert!(optimized.batch_size > 0);
}

#[test]
fn test_optimize_with_maximum_latency() {
    let service = OptimizationService::new();
    let strategy = service
        .get_strategy_for_use_case(&StreamingUseCase::RealTimeDashboard)
        .unwrap();

    let context = create_test_performance_context(10000.0, 10.0, 0.01, 0.5);

    let optimized = service
        .optimize_strategy_for_context(strategy.clone(), &context)
        .unwrap();

    // Very high latency should reduce batch size
    assert!(optimized.batch_size < strategy.batch_size);
}

#[test]
fn test_optimize_with_maximum_bandwidth() {
    let service = OptimizationService::new();
    let strategy = service
        .get_strategy_for_use_case(&StreamingUseCase::RealTimeDashboard)
        .unwrap();

    let context = create_test_performance_context(100.0, 100.0, 0.01, 0.5);

    let optimized = service
        .optimize_strategy_for_context(strategy.clone(), &context)
        .unwrap();

    // Very high bandwidth should increase frame size
    assert!(optimized.max_frame_size > strategy.max_frame_size);
}

#[test]
fn test_optimize_multiple_times() {
    let service = OptimizationService::new();
    let base_strategy = service
        .get_strategy_for_use_case(&StreamingUseCase::RealTimeDashboard)
        .unwrap();

    let context1 = create_test_performance_context(100.0, 10.0, 0.01, 0.5);
    let strategy1 = service
        .optimize_strategy_for_context(base_strategy.clone(), &context1)
        .unwrap();

    let context2 = create_test_performance_context(200.0, 5.0, 0.02, 0.6);
    let strategy2 = service
        .optimize_strategy_for_context(strategy1.clone(), &context2)
        .unwrap();

    // Multiple optimizations should produce valid strategies
    // The batch_size may or may not change depending on context
    assert!(strategy2.batch_size > 0);
    assert!(strategy1.batch_size > 0);
    assert!(base_strategy.batch_size > 0);
}

// === Use Case Variants Tests ===

#[test]
fn test_all_adjustment_types() {
    let types = vec![
        AdjustmentType::PriorityIncrease,
        AdjustmentType::PriorityDecrease,
        AdjustmentType::BatchSizeIncrease,
        AdjustmentType::BatchSizeDecrease,
        AdjustmentType::FrameSizeIncrease,
        AdjustmentType::FrameSizeDecrease,
        AdjustmentType::CompressionEnable,
        AdjustmentType::CompressionDisable,
        AdjustmentType::QualityIncrease,
        AdjustmentType::QualityReduction,
    ];

    // All types should be usable
    for adjustment_type in types {
        let _ = format!("{:?}", adjustment_type);
    }
}

#[test]
fn test_streaming_use_case_debug() {
    let use_cases = vec![
        StreamingUseCase::RealTimeDashboard,
        StreamingUseCase::BulkDataTransfer,
        StreamingUseCase::MobileApp,
        StreamingUseCase::ProgressiveWebApp,
        StreamingUseCase::IoTDevice,
        StreamingUseCase::LiveStreaming,
        StreamingUseCase::Custom("test".to_string()),
    ];

    for use_case in use_cases {
        let debug_str = format!("{:?}", use_case);
        assert!(!debug_str.is_empty());
    }
}
