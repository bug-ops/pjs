//! Service responsible for analyzing streaming performance and metrics
//!
//! This service focuses on collecting, analyzing, and interpreting
//! performance metrics to support optimization decisions.

use crate::{
    application::ApplicationResult,
    domain::value_objects::{Priority, SessionId, StreamId},
};
use std::{
    collections::{HashMap, VecDeque},
    time::{Duration, SystemTime},
};

/// Service for performance analysis and metrics collection
#[derive(Debug)]
pub struct PerformanceAnalysisService {
    metrics_history: MetricsHistory,
    analysis_config: AnalysisConfig,
}

/// Configuration for performance analysis
#[derive(Debug, Clone)]
pub struct AnalysisConfig {
    pub history_retention_duration: Duration,
    pub sample_window_size: usize,
    pub alerting_thresholds: AlertingThresholds,
    pub analysis_interval: Duration,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            history_retention_duration: Duration::from_secs(3600), // 1 hour
            sample_window_size: 100,
            alerting_thresholds: AlertingThresholds::default(),
            analysis_interval: Duration::from_secs(30),
        }
    }
}

/// Thresholds for performance alerting
#[derive(Debug, Clone)]
pub struct AlertingThresholds {
    pub critical_latency_ms: f64,
    pub warning_latency_ms: f64,
    pub critical_error_rate: f64,
    pub warning_error_rate: f64,
    pub min_throughput_mbps: f64,
    pub max_cpu_usage: f64,
}

impl Default for AlertingThresholds {
    fn default() -> Self {
        Self {
            critical_latency_ms: 2000.0,
            warning_latency_ms: 1000.0,
            critical_error_rate: 0.1,
            warning_error_rate: 0.05,
            min_throughput_mbps: 1.0,
            max_cpu_usage: 0.9,
        }
    }
}

/// Historical metrics storage
#[derive(Debug)]
struct MetricsHistory {
    latency_samples: VecDeque<LatencySample>,
    throughput_samples: VecDeque<ThroughputSample>,
    error_samples: VecDeque<ErrorSample>,
    resource_samples: VecDeque<ResourceSample>,
    max_samples: usize,
}

impl MetricsHistory {
    fn new(max_samples: usize) -> Self {
        Self {
            latency_samples: VecDeque::with_capacity(max_samples),
            throughput_samples: VecDeque::with_capacity(max_samples),
            error_samples: VecDeque::with_capacity(max_samples),
            resource_samples: VecDeque::with_capacity(max_samples),
            max_samples,
        }
    }

    fn add_latency_sample(&mut self, sample: LatencySample) {
        if self.latency_samples.len() >= self.max_samples {
            self.latency_samples.pop_front();
        }
        self.latency_samples.push_back(sample);
    }

    fn add_throughput_sample(&mut self, sample: ThroughputSample) {
        if self.throughput_samples.len() >= self.max_samples {
            self.throughput_samples.pop_front();
        }
        self.throughput_samples.push_back(sample);
    }

    fn add_error_sample(&mut self, sample: ErrorSample) {
        if self.error_samples.len() >= self.max_samples {
            self.error_samples.pop_front();
        }
        self.error_samples.push_back(sample);
    }

    fn add_resource_sample(&mut self, sample: ResourceSample) {
        if self.resource_samples.len() >= self.max_samples {
            self.resource_samples.pop_front();
        }
        self.resource_samples.push_back(sample);
    }
}

/// Individual metric samples
#[derive(Debug, Clone)]
struct LatencySample {
    timestamp: SystemTime,
    session_id: SessionId,
    stream_id: Option<StreamId>,
    latency_ms: f64,
    operation_type: String,
}

#[derive(Debug, Clone)]
struct ThroughputSample {
    timestamp: SystemTime,
    session_id: SessionId,
    bytes_transferred: u64,
    duration: Duration,
    frame_count: usize,
}

#[derive(Debug, Clone)]
struct ErrorSample {
    timestamp: SystemTime,
    session_id: SessionId,
    stream_id: Option<StreamId>,
    error_type: String,
    error_severity: ErrorSeverity,
}

#[derive(Debug, Clone)]
struct ResourceSample {
    timestamp: SystemTime,
    cpu_usage: f64,
    memory_usage_bytes: u64,
    network_bandwidth_mbps: f64,
    active_connections: usize,
}

#[derive(Debug, Clone, PartialEq)]
enum ErrorSeverity {
    Low,
    Medium,
    High,
    Critical,
}

impl PerformanceAnalysisService {
    pub fn new(config: AnalysisConfig) -> Self {
        let history = MetricsHistory::new(config.sample_window_size);
        Self {
            metrics_history: history,
            analysis_config: config,
        }
    }

    /// Record a latency measurement
    pub fn record_latency(
        &mut self,
        session_id: SessionId,
        stream_id: Option<StreamId>,
        latency_ms: f64,
        operation_type: String,
    ) -> ApplicationResult<()> {
        let sample = LatencySample {
            timestamp: SystemTime::now(),
            session_id,
            stream_id,
            latency_ms,
            operation_type,
        };

        self.metrics_history.add_latency_sample(sample);
        Ok(())
    }

    /// Record throughput measurement
    pub fn record_throughput(
        &mut self,
        session_id: SessionId,
        bytes_transferred: u64,
        duration: Duration,
        frame_count: usize,
    ) -> ApplicationResult<()> {
        let sample = ThroughputSample {
            timestamp: SystemTime::now(),
            session_id,
            bytes_transferred,
            duration,
            frame_count,
        };

        self.metrics_history.add_throughput_sample(sample);
        Ok(())
    }

    /// Record error occurrence
    pub fn record_error(
        &mut self,
        session_id: SessionId,
        stream_id: Option<StreamId>,
        error_type: String,
        severity: ErrorSeverity,
    ) -> ApplicationResult<()> {
        let sample = ErrorSample {
            timestamp: SystemTime::now(),
            session_id,
            stream_id,
            error_type,
            error_severity: severity,
        };

        self.metrics_history.add_error_sample(sample);
        Ok(())
    }

    /// Record resource usage
    pub fn record_resource_usage(
        &mut self,
        cpu_usage: f64,
        memory_usage_bytes: u64,
        network_bandwidth_mbps: f64,
        active_connections: usize,
    ) -> ApplicationResult<()> {
        let sample = ResourceSample {
            timestamp: SystemTime::now(),
            cpu_usage,
            memory_usage_bytes,
            network_bandwidth_mbps,
            active_connections,
        };

        self.metrics_history.add_resource_sample(sample);
        Ok(())
    }

    /// Analyze current performance and generate report
    pub fn analyze_performance(&self) -> ApplicationResult<PerformanceAnalysisReport> {
        let latency_analysis = self.analyze_latency_metrics()?;
        let throughput_analysis = self.analyze_throughput_metrics()?;
        let error_analysis = self.analyze_error_metrics()?;
        let resource_analysis = self.analyze_resource_metrics()?;

        // Generate overall performance score
        let performance_score = self.calculate_performance_score(
            &latency_analysis,
            &throughput_analysis,
            &error_analysis,
            &resource_analysis,
        );

        // Identify performance issues
        let issues = self.identify_performance_issues(
            &latency_analysis,
            &throughput_analysis,
            &error_analysis,
            &resource_analysis,
        )?;

        // Generate recommendations
        let recommendations = self.generate_recommendations(&issues)?;

        Ok(PerformanceAnalysisReport {
            timestamp: SystemTime::now(),
            overall_score: performance_score,
            latency_analysis,
            throughput_analysis,
            error_analysis,
            resource_analysis,
            issues,
            recommendations,
        })
    }

    /// Get real-time performance context for priority calculations
    pub fn get_performance_context(&self) -> ApplicationResult<crate::application::services::prioritization_service::PerformanceContext> {
        let latency_stats = self.calculate_latency_statistics()?;
        let throughput_stats = self.calculate_throughput_statistics()?;
        let error_stats = self.calculate_error_statistics()?;
        let resource_stats = self.calculate_resource_statistics()?;

        Ok(crate::application::services::prioritization_service::PerformanceContext {
            average_latency_ms: latency_stats.average,
            available_bandwidth_mbps: throughput_stats.current_mbps,
            error_rate: error_stats.rate,
            cpu_usage: resource_stats.cpu_usage,
            memory_usage_percent: resource_stats.memory_usage_percent,
            connection_count: resource_stats.connection_count,
        })
    }

    /// Calculate batch size recommendations
    pub fn calculate_optimal_batch_size(&self, base_size: usize) -> ApplicationResult<BatchSizeRecommendation> {
        let context = self.get_performance_context()?;
        
        // Analyze current performance to recommend batch size
        let latency_factor = if context.average_latency_ms < 50.0 {
            0.8 // Smaller batches for low latency responsiveness
        } else if context.average_latency_ms > 500.0 {
            1.5 // Larger batches when latency is already high
        } else {
            1.0
        };

        let bandwidth_factor = (context.available_bandwidth_mbps / 5.0).min(2.0).max(0.5);
        let cpu_factor = if context.cpu_usage > 0.8 { 0.7 } else { 1.0 };
        let error_factor = if context.error_rate > 0.05 { 0.8 } else { 1.0 };

        let recommended_size = ((base_size as f64) * latency_factor * bandwidth_factor * cpu_factor * error_factor) as usize;
        let recommended_size = recommended_size.max(1).min(1000); // Bounds checking

        Ok(BatchSizeRecommendation {
            recommended_size,
            confidence: self.calculate_recommendation_confidence(&context),
            reasoning: vec![
                format!("Latency factor: {:.2}", latency_factor),
                format!("Bandwidth factor: {:.2}", bandwidth_factor),
                format!("CPU factor: {:.2}", cpu_factor),
                format!("Error factor: {:.2}", error_factor),
            ],
        })
    }

    /// Analyze frame distribution efficiency
    pub fn analyze_frame_distribution(&self, frames: &[crate::domain::entities::Frame]) -> ApplicationResult<FrameDistributionAnalysis> {
        let mut priority_distribution = HashMap::new();
        let mut size_distribution = Vec::new();
        let mut total_bytes = 0u64;

        for frame in frames {
            // Analyze priority distribution
            if let Some(priority) = frame.priority() {
                *priority_distribution.entry(priority).or_insert(0) += 1;
            }

            // Analyze size distribution
            let frame_size = frame.data().len();
            size_distribution.push(frame_size);
            total_bytes += frame_size as u64;
        }

        // Calculate statistics
        size_distribution.sort_unstable();
        let median_size = if size_distribution.is_empty() {
            0
        } else {
            size_distribution[size_distribution.len() / 2]
        };

        let average_size = if frames.is_empty() {
            0.0
        } else {
            total_bytes as f64 / frames.len() as f64
        };

        Ok(FrameDistributionAnalysis {
            total_frames: frames.len(),
            total_bytes,
            average_frame_size: average_size,
            median_frame_size: median_size as f64,
            priority_distribution,
            efficiency_score: self.calculate_distribution_efficiency(&priority_distribution, frames.len()),
        })
    }

    // Private implementation methods

    fn analyze_latency_metrics(&self) -> ApplicationResult<LatencyAnalysis> {
        if self.metrics_history.latency_samples.is_empty() {
            return Ok(LatencyAnalysis::default());
        }

        let mut latencies: Vec<f64> = self.metrics_history.latency_samples
            .iter()
            .map(|s| s.latency_ms)
            .collect();

        latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let count = latencies.len();
        let average = latencies.iter().sum::<f64>() / count as f64;
        let p50 = latencies[count / 2];
        let p95 = latencies[(count as f64 * 0.95) as usize];
        let p99 = latencies[(count as f64 * 0.99) as usize];

        Ok(LatencyAnalysis {
            average,
            p50,
            p95,
            p99,
            min: latencies[0],
            max: latencies[count - 1],
            sample_count: count,
        })
    }

    fn analyze_throughput_metrics(&self) -> ApplicationResult<ThroughputAnalysis> {
        if self.metrics_history.throughput_samples.is_empty() {
            return Ok(ThroughputAnalysis::default());
        }

        let mut total_bytes = 0u64;
        let mut total_duration = Duration::ZERO;
        let mut total_frames = 0usize;

        for sample in &self.metrics_history.throughput_samples {
            total_bytes += sample.bytes_transferred;
            total_duration += sample.duration;
            total_frames += sample.frame_count;
        }

        let average_mbps = if total_duration.as_secs_f64() > 0.0 {
            (total_bytes as f64 * 8.0) / (total_duration.as_secs_f64() * 1_000_000.0)
        } else {
            0.0
        };

        let frames_per_second = if total_duration.as_secs_f64() > 0.0 {
            total_frames as f64 / total_duration.as_secs_f64()
        } else {
            0.0
        };

        Ok(ThroughputAnalysis {
            average_mbps,
            frames_per_second,
            total_bytes,
            total_frames,
            sample_count: self.metrics_history.throughput_samples.len(),
        })
    }

    fn analyze_error_metrics(&self) -> ApplicationResult<ErrorAnalysis> {
        if self.metrics_history.error_samples.is_empty() {
            return Ok(ErrorAnalysis::default());
        }

        let total_samples = self.metrics_history.error_samples.len() +
                           self.metrics_history.latency_samples.len() +
                           self.metrics_history.throughput_samples.len();

        let error_count = self.metrics_history.error_samples.len();
        let error_rate = if total_samples > 0 {
            error_count as f64 / total_samples as f64
        } else {
            0.0
        };

        // Analyze error types
        let mut error_type_distribution = HashMap::new();
        let mut severity_distribution = HashMap::new();

        for sample in &self.metrics_history.error_samples {
            *error_type_distribution.entry(sample.error_type.clone()).or_insert(0) += 1;
            *severity_distribution.entry(format!("{:?}", sample.error_severity)).or_insert(0) += 1;
        }

        Ok(ErrorAnalysis {
            error_rate,
            total_errors: error_count,
            error_type_distribution,
            severity_distribution,
        })
    }

    fn analyze_resource_metrics(&self) -> ApplicationResult<ResourceAnalysis> {
        if self.metrics_history.resource_samples.is_empty() {
            return Ok(ResourceAnalysis::default());
        }

        let latest_sample = self.metrics_history.resource_samples.back().unwrap();
        
        let cpu_values: Vec<f64> = self.metrics_history.resource_samples
            .iter()
            .map(|s| s.cpu_usage)
            .collect();

        let memory_values: Vec<u64> = self.metrics_history.resource_samples
            .iter()
            .map(|s| s.memory_usage_bytes)
            .collect();

        let average_cpu = cpu_values.iter().sum::<f64>() / cpu_values.len() as f64;
        let average_memory = memory_values.iter().sum::<u64>() / memory_values.len() as u64;

        Ok(ResourceAnalysis {
            current_cpu_usage: latest_sample.cpu_usage,
            average_cpu_usage: average_cpu,
            current_memory_usage: latest_sample.memory_usage_bytes,
            average_memory_usage: average_memory,
            network_bandwidth_mbps: latest_sample.network_bandwidth_mbps,
            active_connections: latest_sample.active_connections,
        })
    }

    fn calculate_performance_score(
        &self,
        latency: &LatencyAnalysis,
        throughput: &ThroughputAnalysis,
        errors: &ErrorAnalysis,
        resources: &ResourceAnalysis,
    ) -> f64 {
        let mut score = 100.0;

        // Penalize high latency
        if latency.average > 1000.0 {
            score -= 30.0;
        } else if latency.average > 500.0 {
            score -= 15.0;
        }

        // Penalize low throughput
        if throughput.average_mbps < 1.0 {
            score -= 20.0;
        } else if throughput.average_mbps < 5.0 {
            score -= 10.0;
        }

        // Penalize high error rates
        if errors.error_rate > 0.1 {
            score -= 40.0;
        } else if errors.error_rate > 0.05 {
            score -= 20.0;
        }

        // Penalize high resource usage
        if resources.current_cpu_usage > 0.9 {
            score -= 15.0;
        } else if resources.current_cpu_usage > 0.8 {
            score -= 5.0;
        }

        score.max(0.0).min(100.0)
    }

    fn identify_performance_issues(
        &self,
        latency: &LatencyAnalysis,
        throughput: &ThroughputAnalysis,
        errors: &ErrorAnalysis,
        resources: &ResourceAnalysis,
    ) -> ApplicationResult<Vec<PerformanceIssue>> {
        let mut issues = Vec::new();

        // Latency issues
        if latency.average > self.analysis_config.alerting_thresholds.critical_latency_ms {
            issues.push(PerformanceIssue {
                issue_type: "High Latency".to_string(),
                severity: IssueSeverity::Critical,
                description: format!("Average latency {:.1}ms exceeds critical threshold", latency.average),
                impact: "User experience severely degraded".to_string(),
                suggested_action: "Reduce data size, increase priority threshold".to_string(),
            });
        }

        // Throughput issues
        if throughput.average_mbps < self.analysis_config.alerting_thresholds.min_throughput_mbps {
            issues.push(PerformanceIssue {
                issue_type: "Low Throughput".to_string(),
                severity: IssueSeverity::High,
                description: format!("Throughput {:.1}Mbps below minimum threshold", throughput.average_mbps),
                impact: "Data delivery is slower than expected".to_string(),
                suggested_action: "Optimize batch sizes, check network conditions".to_string(),
            });
        }

        // Error rate issues
        if errors.error_rate > self.analysis_config.alerting_thresholds.critical_error_rate {
            issues.push(PerformanceIssue {
                issue_type: "High Error Rate".to_string(),
                severity: IssueSeverity::Critical,
                description: format!("Error rate {:.1}% exceeds critical threshold", errors.error_rate * 100.0),
                impact: "System reliability is compromised".to_string(),
                suggested_action: "Investigate error causes, increase priority selectivity".to_string(),
            });
        }

        // Resource issues
        if resources.current_cpu_usage > self.analysis_config.alerting_thresholds.max_cpu_usage {
            issues.push(PerformanceIssue {
                issue_type: "High CPU Usage".to_string(),
                severity: IssueSeverity::High,
                description: format!("CPU usage {:.1}% exceeds threshold", resources.current_cpu_usage * 100.0),
                impact: "System performance may degrade".to_string(),
                suggested_action: "Reduce processing load, optimize algorithms".to_string(),
            });
        }

        Ok(issues)
    }

    fn generate_recommendations(&self, issues: &[PerformanceIssue]) -> ApplicationResult<Vec<OptimizationRecommendation>> {
        let mut recommendations = Vec::new();

        for issue in issues {
            match issue.issue_type.as_str() {
                "High Latency" => {
                    recommendations.push(OptimizationRecommendation {
                        priority: RecommendationPriority::High,
                        category: "Priority Optimization".to_string(),
                        description: "Increase priority threshold to reduce data volume".to_string(),
                        expected_impact: "Reduce latency by 20-40%".to_string(),
                        implementation_effort: ImplementationEffort::Low,
                    });
                }
                "Low Throughput" => {
                    recommendations.push(OptimizationRecommendation {
                        priority: RecommendationPriority::Medium,
                        category: "Batch Optimization".to_string(),
                        description: "Increase batch size to improve throughput".to_string(),
                        expected_impact: "Improve throughput by 15-30%".to_string(),
                        implementation_effort: ImplementationEffort::Low,
                    });
                }
                "High Error Rate" => {
                    recommendations.push(OptimizationRecommendation {
                        priority: RecommendationPriority::High,
                        category: "Reliability Improvement".to_string(),
                        description: "Implement retry logic and error handling".to_string(),
                        expected_impact: "Reduce error rate by 50-80%".to_string(),
                        implementation_effort: ImplementationEffort::Medium,
                    });
                }
                _ => {}
            }
        }

        Ok(recommendations)
    }

    fn calculate_latency_statistics(&self) -> ApplicationResult<LatencyStatistics> {
        if self.metrics_history.latency_samples.is_empty() {
            return Ok(LatencyStatistics::default());
        }

        let latencies: Vec<f64> = self.metrics_history.latency_samples
            .iter()
            .map(|s| s.latency_ms)
            .collect();

        let average = latencies.iter().sum::<f64>() / latencies.len() as f64;

        Ok(LatencyStatistics { average })
    }

    fn calculate_throughput_statistics(&self) -> ApplicationResult<ThroughputStatistics> {
        if self.metrics_history.throughput_samples.is_empty() {
            return Ok(ThroughputStatistics::default());
        }

        // Use the most recent sample for current throughput
        let latest_sample = self.metrics_history.throughput_samples.back().unwrap();
        let current_mbps = if latest_sample.duration.as_secs_f64() > 0.0 {
            (latest_sample.bytes_transferred as f64 * 8.0) / (latest_sample.duration.as_secs_f64() * 1_000_000.0)
        } else {
            0.0
        };

        Ok(ThroughputStatistics { current_mbps })
    }

    fn calculate_error_statistics(&self) -> ApplicationResult<ErrorStatistics> {
        let total_operations = self.metrics_history.latency_samples.len() +
                             self.metrics_history.throughput_samples.len();
        
        let error_count = self.metrics_history.error_samples.len();
        
        let rate = if total_operations > 0 {
            error_count as f64 / total_operations as f64
        } else {
            0.0
        };

        Ok(ErrorStatistics { rate })
    }

    fn calculate_resource_statistics(&self) -> ApplicationResult<ResourceStatistics> {
        if self.metrics_history.resource_samples.is_empty() {
            return Ok(ResourceStatistics::default());
        }

        let latest = self.metrics_history.resource_samples.back().unwrap();
        
        Ok(ResourceStatistics {
            cpu_usage: latest.cpu_usage,
            memory_usage_percent: (latest.memory_usage_bytes as f64 / (8_000_000_000.0)) * 100.0, // Assume 8GB total
            connection_count: latest.active_connections,
        })
    }

    fn calculate_recommendation_confidence(&self, context: &crate::application::services::prioritization_service::PerformanceContext) -> f64 {
        let mut confidence = 1.0;

        if context.error_rate > 0.1 {
            confidence *= 0.6; // High error rate reduces confidence
        }

        if self.metrics_history.latency_samples.len() < 10 {
            confidence *= 0.7; // Low sample count reduces confidence
        }

        confidence.max(0.1)
    }

    fn calculate_distribution_efficiency(&self, priority_distribution: &HashMap<u8, usize>, total_frames: usize) -> f64 {
        if total_frames == 0 {
            return 1.0;
        }

        // Calculate how well distributed priorities are
        let unique_priorities = priority_distribution.len() as f64;
        let max_possible_priorities = 5.0; // Assuming 5 priority levels

        // Higher score for more diverse priority usage
        (unique_priorities / max_possible_priorities).min(1.0)
    }
}

impl Default for PerformanceAnalysisService {
    fn default() -> Self {
        Self::new(AnalysisConfig::default())
    }
}

// Supporting types for analysis results

#[derive(Debug, Clone)]
pub struct PerformanceAnalysisReport {
    pub timestamp: SystemTime,
    pub overall_score: f64,
    pub latency_analysis: LatencyAnalysis,
    pub throughput_analysis: ThroughputAnalysis,
    pub error_analysis: ErrorAnalysis,
    pub resource_analysis: ResourceAnalysis,
    pub issues: Vec<PerformanceIssue>,
    pub recommendations: Vec<OptimizationRecommendation>,
}

#[derive(Debug, Clone, Default)]
pub struct LatencyAnalysis {
    pub average: f64,
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub min: f64,
    pub max: f64,
    pub sample_count: usize,
}

#[derive(Debug, Clone, Default)]
pub struct ThroughputAnalysis {
    pub average_mbps: f64,
    pub frames_per_second: f64,
    pub total_bytes: u64,
    pub total_frames: usize,
    pub sample_count: usize,
}

#[derive(Debug, Clone, Default)]
pub struct ErrorAnalysis {
    pub error_rate: f64,
    pub total_errors: usize,
    pub error_type_distribution: HashMap<String, usize>,
    pub severity_distribution: HashMap<String, usize>,
}

#[derive(Debug, Clone, Default)]
pub struct ResourceAnalysis {
    pub current_cpu_usage: f64,
    pub average_cpu_usage: f64,
    pub current_memory_usage: u64,
    pub average_memory_usage: u64,
    pub network_bandwidth_mbps: f64,
    pub active_connections: usize,
}

#[derive(Debug, Clone)]
pub struct BatchSizeRecommendation {
    pub recommended_size: usize,
    pub confidence: f64,
    pub reasoning: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct FrameDistributionAnalysis {
    pub total_frames: usize,
    pub total_bytes: u64,
    pub average_frame_size: f64,
    pub median_frame_size: f64,
    pub priority_distribution: HashMap<u8, usize>,
    pub efficiency_score: f64,
}

#[derive(Debug, Clone)]
pub struct PerformanceIssue {
    pub issue_type: String,
    pub severity: IssueSeverity,
    pub description: String,
    pub impact: String,
    pub suggested_action: String,
}

#[derive(Debug, Clone)]
pub struct OptimizationRecommendation {
    pub priority: RecommendationPriority,
    pub category: String,
    pub description: String,
    pub expected_impact: String,
    pub implementation_effort: ImplementationEffort,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IssueSeverity {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RecommendationPriority {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImplementationEffort {
    Low,
    Medium,
    High,
}

// Internal statistics types
#[derive(Debug, Clone, Default)]
struct LatencyStatistics {
    average: f64,
}

#[derive(Debug, Clone, Default)]
struct ThroughputStatistics {
    current_mbps: f64,
}

#[derive(Debug, Clone, Default)]
struct ErrorStatistics {
    rate: f64,
}

#[derive(Debug, Clone, Default)]
struct ResourceStatistics {
    cpu_usage: f64,
    memory_usage_percent: f64,
    connection_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_performance_analysis_service_creation() {
        let service = PerformanceAnalysisService::default();
        assert_eq!(service.metrics_history.latency_samples.len(), 0);
    }

    #[test]
    fn test_latency_recording() {
        let mut service = PerformanceAnalysisService::default();
        let session_id = crate::domain::value_objects::SessionId::new();

        service.record_latency(
            session_id,
            None,
            100.0,
            "test_operation".to_string(),
        ).unwrap();

        assert_eq!(service.metrics_history.latency_samples.len(), 1);
    }
}