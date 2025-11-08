//! Stream context separation - Config vs Session state
//!
//! Separates configuration (static) from session state (dynamic runtime data)
//! following the principle of separating concerns and improving performance.

use serde::{Deserialize, Serialize};

/// Static configuration for streaming operations
/// This should be immutable after creation and can be shared across sessions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamConfig {
    /// Maximum bandwidth allocation in Mbps
    pub max_bandwidth_mbps: f64,

    /// Latency threshold for priority adjustment in milliseconds
    pub latency_threshold_ms: f64,

    /// Maximum acceptable error rate (0.0-1.0)
    pub max_error_rate: f64,

    /// CPU usage threshold for throttling (0.0-1.0)
    pub cpu_usage_threshold: f64,

    /// Memory usage threshold for throttling (0.0-100.0)
    pub memory_usage_threshold_percent: f64,

    /// Maximum concurrent connections per session
    pub max_connection_count: usize,

    /// Default priority strategy
    pub default_prioritization_strategy: PrioritizationStrategy,

    /// Enable adaptive priority adjustment
    pub enable_adaptive_priority: bool,

    /// Buffer size configuration
    pub buffer_size_kb: usize,

    /// Compression settings
    pub enable_compression: bool,
    pub compression_level: u8,

    /// Timeout configurations
    pub session_timeout_seconds: u64,
    pub frame_timeout_ms: u64,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            max_bandwidth_mbps: 100.0,
            latency_threshold_ms: 500.0,
            max_error_rate: 0.05,
            cpu_usage_threshold: 0.8,
            memory_usage_threshold_percent: 80.0,
            max_connection_count: 100,
            default_prioritization_strategy: PrioritizationStrategy::Balanced,
            enable_adaptive_priority: true,
            buffer_size_kb: 64,
            enable_compression: true,
            compression_level: 6,
            session_timeout_seconds: 3600, // 1 hour
            frame_timeout_ms: 30000,       // 30 seconds
        }
    }
}

/// Dynamic session state tracking runtime metrics and performance
/// This data changes frequently during a session's lifetime
#[derive(Debug, Clone)]
pub struct StreamSession {
    /// Current average latency in milliseconds
    pub current_latency_ms: f64,

    /// Current bandwidth utilization in Mbps
    pub current_bandwidth_mbps: f64,

    /// Current error rate (0.0-1.0)
    pub current_error_rate: f64,

    /// Current CPU usage (0.0-1.0)
    pub current_cpu_usage: f64,

    /// Current memory usage percentage (0.0-100.0)
    pub current_memory_usage_percent: f64,

    /// Current active connection count
    pub active_connection_count: usize,

    /// Number of frames processed in this session
    pub frames_processed: u64,

    /// Total bytes transferred in this session
    pub bytes_transferred: u64,

    /// Session start time
    pub session_start_time: std::time::Instant,

    /// Last update time for metrics
    pub last_metrics_update: std::time::Instant,

    /// Current priority override (if any)
    pub priority_override: Option<crate::domain::value_objects::Priority>,

    /// Performance trend (improving/degrading/stable)
    pub performance_trend: PerformanceTrend,

    /// Adaptive adjustments made during this session
    pub adaptive_adjustments: Vec<AdaptiveAdjustment>,
}

impl Default for StreamSession {
    fn default() -> Self {
        let now = std::time::Instant::now();
        Self {
            current_latency_ms: 100.0,
            current_bandwidth_mbps: 10.0,
            current_error_rate: 0.01,
            current_cpu_usage: 0.5,
            current_memory_usage_percent: 60.0,
            active_connection_count: 1,
            frames_processed: 0,
            bytes_transferred: 0,
            session_start_time: now,
            last_metrics_update: now,
            priority_override: None,
            performance_trend: PerformanceTrend::Stable,
            adaptive_adjustments: Vec::new(),
        }
    }
}

/// Performance trend analysis
#[derive(Debug, Clone, PartialEq)]
pub enum PerformanceTrend {
    Improving,
    Stable,
    Degrading,
}

/// Record of adaptive adjustments made during session
#[derive(Debug, Clone)]
pub struct AdaptiveAdjustment {
    pub timestamp: std::time::Instant,
    pub adjustment_type: AdjustmentType,
    pub reason: String,
    pub impact_estimate: f64, // estimated improvement (0.0-1.0)
}

#[derive(Debug, Clone)]
pub enum AdjustmentType {
    PriorityIncrease(u8),
    PriorityDecrease(u8),
    BufferSizeChange(usize),
    CompressionToggle(bool),
    ThrottleEnable,
    ThrottleDisable,
}

/// Re-export prioritization strategy from existing module
pub use super::prioritization_service::PrioritizationStrategy;

/// Combined context that references both config and session
/// This provides a unified interface while keeping concerns separated
#[derive(Debug)]
pub struct StreamContext<'a> {
    pub config: &'a StreamConfig,
    pub session: &'a StreamSession,
}

impl<'a> StreamContext<'a> {
    pub fn new(config: &'a StreamConfig, session: &'a StreamSession) -> Self {
        Self { config, session }
    }

    /// Check if current metrics exceed configured thresholds
    pub fn is_threshold_exceeded(&self) -> bool {
        self.session.current_latency_ms > self.config.latency_threshold_ms
            || self.session.current_error_rate > self.config.max_error_rate
            || self.session.current_cpu_usage > self.config.cpu_usage_threshold
            || self.session.current_memory_usage_percent
                > self.config.memory_usage_threshold_percent
    }

    /// Calculate performance score (0.0 = poor, 1.0 = excellent)
    pub fn performance_score(&self) -> f64 {
        let latency_score = (self.config.latency_threshold_ms
            - self
                .session
                .current_latency_ms
                .min(self.config.latency_threshold_ms))
            / self.config.latency_threshold_ms;
        let error_score =
            1.0 - (self.session.current_error_rate / self.config.max_error_rate).min(1.0);
        let cpu_score =
            1.0 - (self.session.current_cpu_usage / self.config.cpu_usage_threshold).min(1.0);
        let memory_score = 1.0
            - (self.session.current_memory_usage_percent
                / self.config.memory_usage_threshold_percent)
                .min(1.0);

        (latency_score + error_score + cpu_score + memory_score) / 4.0
    }

    /// Get session duration
    pub fn session_duration(&self) -> std::time::Duration {
        self.session
            .last_metrics_update
            .duration_since(self.session.session_start_time)
    }

    /// Check if session should timeout
    pub fn should_timeout(&self) -> bool {
        self.session_duration().as_secs() > self.config.session_timeout_seconds
    }
}

/// Utility functions for migration from old PerformanceContext
impl StreamSession {
    /// Create from legacy PerformanceContext
    pub fn from_performance_context(
        ctx: &super::prioritization_service::PerformanceContext,
    ) -> Self {
        Self {
            current_latency_ms: ctx.average_latency_ms,
            current_bandwidth_mbps: ctx.available_bandwidth_mbps,
            current_error_rate: ctx.error_rate,
            current_cpu_usage: ctx.cpu_usage,
            current_memory_usage_percent: ctx.memory_usage_percent,
            active_connection_count: ctx.connection_count,
            ..Default::default()
        }
    }

    /// Convert to legacy PerformanceContext for backward compatibility
    pub fn to_performance_context(&self) -> super::prioritization_service::PerformanceContext {
        super::prioritization_service::PerformanceContext {
            average_latency_ms: self.current_latency_ms,
            available_bandwidth_mbps: self.current_bandwidth_mbps,
            error_rate: self.current_error_rate,
            cpu_usage: self.current_cpu_usage,
            memory_usage_percent: self.current_memory_usage_percent,
            connection_count: self.active_connection_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_config_default() {
        let config = StreamConfig::default();
        assert_eq!(config.max_bandwidth_mbps, 100.0);
        assert_eq!(config.latency_threshold_ms, 500.0);
        assert!(config.enable_adaptive_priority);
    }

    #[test]
    fn test_stream_session_default() {
        let session = StreamSession::default();
        assert_eq!(session.frames_processed, 0);
        assert_eq!(session.bytes_transferred, 0);
        assert_eq!(session.performance_trend, PerformanceTrend::Stable);
    }

    #[test]
    fn test_stream_context_threshold_check() {
        let config = StreamConfig::default();
        let mut session = StreamSession::default();

        // Normal operation - no thresholds exceeded
        let context = StreamContext::new(&config, &session);
        assert!(!context.is_threshold_exceeded());

        // Exceed latency threshold
        session.current_latency_ms = 600.0; // > 500.0 threshold
        let context = StreamContext::new(&config, &session);
        assert!(context.is_threshold_exceeded());
    }

    #[test]
    fn test_performance_score_calculation() {
        let config = StreamConfig::default();
        let session = StreamSession::default(); // Good defaults

        let context = StreamContext::new(&config, &session);
        let score = context.performance_score();

        // Should be a good score with default values
        assert!(score > 0.5);
        assert!(score <= 1.0);
    }

    #[test]
    fn test_migration_from_performance_context() {
        let perf_ctx = crate::application::services::prioritization_service::PerformanceContext {
            average_latency_ms: 200.0,
            available_bandwidth_mbps: 50.0,
            error_rate: 0.02,
            cpu_usage: 0.7,
            memory_usage_percent: 75.0,
            connection_count: 5,
        };

        let session = StreamSession::from_performance_context(&perf_ctx);

        assert_eq!(session.current_latency_ms, 200.0);
        assert_eq!(session.current_bandwidth_mbps, 50.0);
        assert_eq!(session.current_error_rate, 0.02);
        assert_eq!(session.current_cpu_usage, 0.7);
        assert_eq!(session.current_memory_usage_percent, 75.0);
        assert_eq!(session.active_connection_count, 5);

        // Test round-trip conversion
        let converted_back = session.to_performance_context();
        assert_eq!(
            converted_back.average_latency_ms,
            perf_ctx.average_latency_ms
        );
        assert_eq!(
            converted_back.available_bandwidth_mbps,
            perf_ctx.available_bandwidth_mbps
        );
    }

    #[test]
    fn test_timeout_detection() {
        let config = StreamConfig {
            session_timeout_seconds: 1, // 1 second timeout
            ..StreamConfig::default()
        };

        let mut session = StreamSession {
            session_start_time: std::time::Instant::now() - std::time::Duration::from_secs(2),
            ..StreamSession::default()
        };
        session.last_metrics_update = std::time::Instant::now();

        let context = StreamContext::new(&config, &session);
        assert!(context.should_timeout());
    }
}
