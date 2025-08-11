//! Metrics collection and monitoring implementation

use async_trait::async_trait;
use parking_lot::RwLock;
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use crate::domain::{
    DomainResult,
    ports::MetricsCollector,
    value_objects::{SessionId, StreamId},
};

/// Performance metrics for PJS operations
#[derive(Debug, Clone)]
pub struct PerformanceMetrics {
    pub active_sessions: usize,
    pub total_sessions_created: u64,
    pub active_streams: usize,
    pub total_streams_created: u64,
    pub average_frame_processing_time: Duration,
    pub bytes_streamed: u64,
    pub frames_processed: u64,
    pub error_count: u64,
}

impl Default for PerformanceMetrics {
    fn default() -> Self {
        Self {
            active_sessions: 0,
            total_sessions_created: 0,
            active_streams: 0,
            total_streams_created: 0,
            average_frame_processing_time: Duration::from_millis(0),
            bytes_streamed: 0,
            frames_processed: 0,
            error_count: 0,
        }
    }
}

/// In-memory metrics collector with time-series data
#[derive(Debug, Clone)]
pub struct InMemoryMetricsCollector {
    metrics: Arc<RwLock<PerformanceMetrics>>,
    session_metrics: Arc<RwLock<HashMap<SessionId, SessionMetrics>>>,
    stream_metrics: Arc<RwLock<HashMap<StreamId, StreamMetrics>>>,
    time_series: Arc<RwLock<Vec<TimestampedMetrics>>>,
    max_time_series_entries: usize,
}

#[derive(Debug, Clone)]
pub struct SessionMetrics {
    session_id: SessionId,
    created_at: Instant,
    last_activity: Instant,
    streams_created: usize,
    bytes_processed: u64,
    frames_sent: u64,
    errors: u64,
    metadata: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct StreamMetrics {
    stream_id: StreamId,
    session_id: SessionId,
    created_at: Instant,
    completed_at: Option<Instant>,
    frames_generated: u64,
    bytes_sent: u64,
    average_priority: f64,
    processing_times: Vec<Duration>,
}

#[derive(Debug, Clone)]
pub struct TimestampedMetrics {
    timestamp: Instant,
    metrics: PerformanceMetrics,
}

impl InMemoryMetricsCollector {
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(RwLock::new(PerformanceMetrics::default())),
            session_metrics: Arc::new(RwLock::new(HashMap::new())),
            stream_metrics: Arc::new(RwLock::new(HashMap::new())),
            time_series: Arc::new(RwLock::new(Vec::new())),
            max_time_series_entries: 1000,
        }
    }
    
    /// Get current performance snapshot
    pub fn get_performance_snapshot(&self) -> PerformanceMetrics {
        self.metrics.read().clone()
    }
    
    /// Get metrics for specific session
    pub fn get_session_metrics(&self, session_id: SessionId) -> Option<SessionMetrics> {
        self.session_metrics.read().get(&session_id).cloned()
    }
    
    /// Get metrics for specific stream
    pub fn get_stream_metrics(&self, stream_id: StreamId) -> Option<StreamMetrics> {
        self.stream_metrics.read().get(&stream_id).cloned()
    }
    
    /// Get time series data for the last N minutes
    pub fn get_time_series(&self, minutes: u32) -> Vec<TimestampedMetrics> {
        let cutoff = Instant::now() - Duration::from_secs(minutes as u64 * 60);
        self.time_series
            .read()
            .iter()
            .filter(|entry| entry.timestamp > cutoff)
            .cloned()
            .collect()
    }
    
    /// Clear all metrics (for testing)
    pub fn clear(&self) {
        *self.metrics.write() = PerformanceMetrics::default();
        self.session_metrics.write().clear();
        self.stream_metrics.write().clear();
        self.time_series.write().clear();
    }
    
    /// Export metrics to Prometheus format
    pub fn export_prometheus(&self) -> String {
        let metrics = self.metrics.read();
        format!(
            r#"# HELP pjs_active_sessions Number of active PJS sessions
# TYPE pjs_active_sessions gauge
pjs_active_sessions {{}} {}

# HELP pjs_total_sessions_created Total number of sessions created
# TYPE pjs_total_sessions_created counter
pjs_total_sessions_created {{}} {}

# HELP pjs_active_streams Number of active streams
# TYPE pjs_active_streams gauge
pjs_active_streams {{}} {}

# HELP pjs_frames_processed_total Total frames processed
# TYPE pjs_frames_processed_total counter
pjs_frames_processed_total {{}} {}

# HELP pjs_bytes_streamed_total Total bytes streamed
# TYPE pjs_bytes_streamed_total counter
pjs_bytes_streamed_total {{}} {}

# HELP pjs_errors_total Total number of errors
# TYPE pjs_errors_total counter
pjs_errors_total {{}} {}

# HELP pjs_frame_processing_time_ms Average frame processing time in milliseconds
# TYPE pjs_frame_processing_time_ms gauge
pjs_frame_processing_time_ms {{}} {}
"#,
            metrics.active_sessions,
            metrics.total_sessions_created,
            metrics.active_streams,
            metrics.frames_processed,
            metrics.bytes_streamed,
            metrics.error_count,
            metrics.average_frame_processing_time.as_millis()
        )
    }
}

impl Default for InMemoryMetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MetricsCollector for InMemoryMetricsCollector {
    async fn increment_counter(&self, name: &str, value: u64, _tags: HashMap<String, String>) -> DomainResult<()> {
        let mut metrics = self.metrics.write();
        
        match name {
            "sessions_created" => metrics.total_sessions_created += value,
            "streams_created" => metrics.total_streams_created += value,
            "frames_processed" => metrics.frames_processed += value,
            "bytes_streamed" => metrics.bytes_streamed += value,
            "errors" => metrics.error_count += value,
            _ => {}, // Unknown metric
        }
        
        Ok(())
    }
    
    async fn set_gauge(&self, name: &str, value: f64, _tags: HashMap<String, String>) -> DomainResult<()> {
        let mut metrics = self.metrics.write();
        
        match name {
            "active_sessions" => metrics.active_sessions = value as usize,
            "active_streams" => metrics.active_streams = value as usize,
            "average_frame_processing_time_ms" => {
                metrics.average_frame_processing_time = Duration::from_millis(value as u64);
            },
            _ => {}, // Unknown metric
        }
        
        Ok(())
    }
    
    async fn record_timing(&self, name: &str, duration: Duration, tags: HashMap<String, String>) -> DomainResult<()> {
        // Update average frame processing time
        if name == "frame_processing" {
            let mut metrics = self.metrics.write();
            let current_avg = metrics.average_frame_processing_time.as_millis() as f64;
            let new_duration = duration.as_millis() as f64;
            
            // Simple moving average calculation
            let processed = metrics.frames_processed as f64;
            if processed > 0.0 {
                let new_avg = (current_avg * processed + new_duration) / (processed + 1.0);
                metrics.average_frame_processing_time = Duration::from_millis(new_avg as u64);
            } else {
                metrics.average_frame_processing_time = duration;
            }
        }
        
        // Record timing for specific session or stream
        if let Some(session_id_str) = tags.get("session_id") {
            if let Ok(session_id) = SessionId::from_string(session_id_str) {
                let mut session_metrics = self.session_metrics.write();
                if let Some(session_metric) = session_metrics.get_mut(&session_id) {
                    session_metric.last_activity = Instant::now();
                }
            }
        }
        
        if let Some(stream_id_str) = tags.get("stream_id") {
            if let Ok(stream_id) = StreamId::from_string(stream_id_str) {
                let mut stream_metrics = self.stream_metrics.write();
                if let Some(stream_metric) = stream_metrics.get_mut(&stream_id) {
                    stream_metric.processing_times.push(duration);
                }
            }
        }
        
        Ok(())
    }
    
    async fn record_session_created(&self, session_id: SessionId, metadata: HashMap<String, String>) -> DomainResult<()> {
        // Update global metrics
        {
            let mut metrics = self.metrics.write();
            metrics.total_sessions_created += 1;
            metrics.active_sessions += 1;
        }
        
        // Create session metrics
        let session_metric = SessionMetrics {
            session_id,
            created_at: Instant::now(),
            last_activity: Instant::now(),
            streams_created: 0,
            bytes_processed: 0,
            frames_sent: 0,
            errors: 0,
            metadata,
        };
        
        self.session_metrics.write().insert(session_id, session_metric);
        
        // Record in time series
        self.record_time_series_snapshot().await?;
        
        Ok(())
    }
    
    async fn record_session_ended(&self, session_id: SessionId) -> DomainResult<()> {
        // Update global metrics
        {
            let mut metrics = self.metrics.write();
            if metrics.active_sessions > 0 {
                metrics.active_sessions -= 1;
            }
        }
        
        // Remove session metrics
        self.session_metrics.write().remove(&session_id);
        
        // Record in time series
        self.record_time_series_snapshot().await?;
        
        Ok(())
    }
    
    async fn record_stream_created(&self, stream_id: StreamId, session_id: SessionId) -> DomainResult<()> {
        // Update global metrics
        {
            let mut metrics = self.metrics.write();
            metrics.total_streams_created += 1;
            metrics.active_streams += 1;
        }
        
        // Update session metrics
        {
            let mut session_metrics = self.session_metrics.write();
            if let Some(session_metric) = session_metrics.get_mut(&session_id) {
                session_metric.streams_created += 1;
                session_metric.last_activity = Instant::now();
            }
        }
        
        // Create stream metrics
        let stream_metric = StreamMetrics {
            stream_id,
            session_id,
            created_at: Instant::now(),
            completed_at: None,
            frames_generated: 0,
            bytes_sent: 0,
            average_priority: 0.0,
            processing_times: Vec::new(),
        };
        
        self.stream_metrics.write().insert(stream_id, stream_metric);
        
        Ok(())
    }
    
    async fn record_stream_completed(&self, stream_id: StreamId) -> DomainResult<()> {
        // Update global metrics
        {
            let mut metrics = self.metrics.write();
            if metrics.active_streams > 0 {
                metrics.active_streams -= 1;
            }
        }
        
        // Update stream metrics
        {
            let mut stream_metrics = self.stream_metrics.write();
            if let Some(stream_metric) = stream_metrics.get_mut(&stream_id) {
                stream_metric.completed_at = Some(Instant::now());
            }
        }
        
        Ok(())
    }
}

impl InMemoryMetricsCollector {
    async fn record_time_series_snapshot(&self) -> DomainResult<()> {
        let snapshot = TimestampedMetrics {
            timestamp: Instant::now(),
            metrics: self.metrics.read().clone(),
        };
        
        let mut time_series = self.time_series.write();
        time_series.push(snapshot);
        
        // Keep only recent entries
        if time_series.len() > self.max_time_series_entries {
            time_series.drain(..100); // Remove oldest 100 entries
        }
        
        Ok(())
    }
}

/// Prometheus-compatible metrics collector
#[cfg(feature = "prometheus-metrics")]
#[derive(Debug, Clone)]
pub struct PrometheusMetricsCollector {
    registry: prometheus::Registry,
    counters: Arc<RwLock<HashMap<String, prometheus::CounterVec>>>,
    gauges: Arc<RwLock<HashMap<String, prometheus::GaugeVec>>>,
    histograms: Arc<RwLock<HashMap<String, prometheus::HistogramVec>>>,
}

#[cfg(feature = "prometheus-metrics")]
impl PrometheusMetricsCollector {
    pub fn new() -> Self {
        Self {
            registry: prometheus::Registry::new(),
            counters: Arc::new(RwLock::new(HashMap::new())),
            gauges: Arc::new(RwLock::new(HashMap::new())),
            histograms: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    pub fn registry(&self) -> &prometheus::Registry {
        &self.registry
    }
}

#[cfg(feature = "prometheus-metrics")]
#[async_trait]
impl MetricsCollector for PrometheusMetricsCollector {
    async fn increment_counter(&self, name: &str, value: u64, tags: HashMap<String, String>) -> DomainResult<()> {
        let mut counters = self.counters.write();
        
        let counter = counters.entry(name.to_string()).or_insert_with(|| {
            let opts = prometheus::Opts::new(name, format!("PJS counter metric: {}", name));
            let labels: Vec<_> = tags.keys().cloned().collect();
            prometheus::CounterVec::new(opts, &labels).unwrap()
        });
        
        let label_values: Vec<_> = tags.values().map(|v| v.as_str()).collect();
        counter.with_label_values(&label_values).inc_by(value as f64);
        
        Ok(())
    }
    
    async fn set_gauge(&self, name: &str, value: f64, tags: HashMap<String, String>) -> DomainResult<()> {
        let mut gauges = self.gauges.write();
        
        let gauge = gauges.entry(name.to_string()).or_insert_with(|| {
            let opts = prometheus::Opts::new(name, format!("PJS gauge metric: {}", name));
            let labels: Vec<_> = tags.keys().cloned().collect();
            prometheus::GaugeVec::new(opts, &labels).unwrap()
        });
        
        let label_values: Vec<_> = tags.values().map(|v| v.as_str()).collect();
        gauge.with_label_values(&label_values).set(value);
        
        Ok(())
    }
    
    async fn record_timing(&self, name: &str, duration: Duration, tags: HashMap<String, String>) -> DomainResult<()> {
        let mut histograms = self.histograms.write();
        
        let histogram = histograms.entry(name.to_string()).or_insert_with(|| {
            let opts = prometheus::HistogramOpts::new(name, format!("PJS timing metric: {}", name));
            let labels: Vec<_> = tags.keys().cloned().collect();
            prometheus::HistogramVec::new(opts, &labels).unwrap()
        });
        
        let label_values: Vec<_> = tags.values().map(|v| v.as_str()).collect();
        histogram.with_label_values(&label_values).observe(duration.as_secs_f64());
        
        Ok(())
    }
    
    async fn record_session_created(&self, session_id: SessionId, metadata: HashMap<String, String>) -> DomainResult<()> {
        let mut tags = metadata;
        tags.insert("session_id".to_string(), session_id.to_string());
        
        self.increment_counter("pjs_sessions_created_total", 1, tags.clone()).await?;
        self.set_gauge("pjs_active_sessions", 1.0, tags).await?;
        
        Ok(())
    }
    
    async fn record_session_ended(&self, session_id: SessionId) -> DomainResult<()> {
        let mut tags = HashMap::new();
        tags.insert("session_id".to_string(), session_id.to_string());
        
        self.set_gauge("pjs_active_sessions", 0.0, tags).await?;
        
        Ok(())
    }
    
    async fn record_stream_created(&self, stream_id: StreamId, session_id: SessionId) -> DomainResult<()> {
        let mut tags = HashMap::new();
        tags.insert("stream_id".to_string(), stream_id.to_string());
        tags.insert("session_id".to_string(), session_id.to_string());
        
        self.increment_counter("pjs_streams_created_total", 1, tags.clone()).await?;
        self.set_gauge("pjs_active_streams", 1.0, tags).await?;
        
        Ok(())
    }
    
    async fn record_stream_completed(&self, stream_id: StreamId) -> DomainResult<()> {
        let mut tags = HashMap::new();
        tags.insert("stream_id".to_string(), stream_id.to_string());
        
        self.set_gauge("pjs_active_streams", 0.0, tags).await?;
        
        Ok(())
    }
}

/// Composite metrics collector that sends to multiple destinations
#[derive(Clone)]
pub struct CompositeMetricsCollector {
    collectors: Vec<Arc<dyn MetricsCollector + Send + Sync>>,
}

impl CompositeMetricsCollector {
    pub fn new() -> Self {
        Self {
            collectors: Vec::new(),
        }
    }
    
    pub fn add_collector<C>(mut self, collector: C) -> Self
    where
        C: MetricsCollector + Send + Sync + 'static,
    {
        self.collectors.push(Arc::new(collector));
        self
    }
}

impl Default for CompositeMetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MetricsCollector for CompositeMetricsCollector {
    async fn increment_counter(&self, name: &str, value: u64, tags: HashMap<String, String>) -> DomainResult<()> {
        for collector in &self.collectors {
            let _ = collector.increment_counter(name, value, tags.clone()).await;
        }
        Ok(())
    }
    
    async fn set_gauge(&self, name: &str, value: f64, tags: HashMap<String, String>) -> DomainResult<()> {
        for collector in &self.collectors {
            let _ = collector.set_gauge(name, value, tags.clone()).await;
        }
        Ok(())
    }
    
    async fn record_timing(&self, name: &str, duration: Duration, tags: HashMap<String, String>) -> DomainResult<()> {
        for collector in &self.collectors {
            let _ = collector.record_timing(name, duration, tags.clone()).await;
        }
        Ok(())
    }
    
    async fn record_session_created(&self, session_id: SessionId, metadata: HashMap<String, String>) -> DomainResult<()> {
        for collector in &self.collectors {
            let _ = collector.record_session_created(session_id, metadata.clone()).await;
        }
        Ok(())
    }
    
    async fn record_session_ended(&self, session_id: SessionId) -> DomainResult<()> {
        for collector in &self.collectors {
            let _ = collector.record_session_ended(session_id).await;
        }
        Ok(())
    }
    
    async fn record_stream_created(&self, stream_id: StreamId, session_id: SessionId) -> DomainResult<()> {
        for collector in &self.collectors {
            let _ = collector.record_stream_created(stream_id, session_id).await;
        }
        Ok(())
    }
    
    async fn record_stream_completed(&self, stream_id: StreamId) -> DomainResult<()> {
        for collector in &self.collectors {
            let _ = collector.record_stream_completed(stream_id).await;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_in_memory_metrics_collector() {
        let collector = InMemoryMetricsCollector::new();
        let session_id = SessionId::new();
        
        // Test session creation
        collector.record_session_created(session_id, HashMap::new()).await.unwrap();
        
        let snapshot = collector.get_performance_snapshot();
        assert_eq!(snapshot.active_sessions, 1);
        assert_eq!(snapshot.total_sessions_created, 1);
        
        // Test session metrics
        let session_metrics = collector.get_session_metrics(session_id).unwrap();
        assert_eq!(session_metrics.session_id, session_id);
        
        // Test session ending
        collector.record_session_ended(session_id).await.unwrap();
        let snapshot = collector.get_performance_snapshot();
        assert_eq!(snapshot.active_sessions, 0);
    }
    
    #[tokio::test]
    async fn test_prometheus_export() {
        let collector = InMemoryMetricsCollector::new();
        
        collector.increment_counter("frames_processed", 100, HashMap::new()).await.unwrap();
        collector.set_gauge("active_sessions", 5.0, HashMap::new()).await.unwrap();
        
        let prometheus_output = collector.export_prometheus();
        assert!(prometheus_output.contains("pjs_frames_processed_total {} 100"));
        assert!(prometheus_output.contains("pjs_active_sessions {} 5"));
    }
    
    #[tokio::test]
    async fn test_composite_metrics_collector() {
        let collector1 = InMemoryMetricsCollector::new();
        let collector2 = InMemoryMetricsCollector::new();
        
        let composite = CompositeMetricsCollector::new()
            .add_collector(collector1.clone())
            .add_collector(collector2.clone());
        
        let session_id = SessionId::new();
        composite.record_session_created(session_id, HashMap::new()).await.unwrap();
        
        assert_eq!(collector1.get_performance_snapshot().total_sessions_created, 1);
        assert_eq!(collector2.get_performance_snapshot().total_sessions_created, 1);
    }
}