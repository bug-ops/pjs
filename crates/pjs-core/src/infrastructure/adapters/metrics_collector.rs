//! Metrics collection and monitoring implementation

use parking_lot::RwLock;
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use crate::domain::{
    DomainResult,
    ports::{MetricsCollectorGat, SessionMetricsGat},
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
    pub session_id: SessionId,
    pub created_at: Instant,
    pub last_activity: Instant,
    pub streams_created: usize,
    pub bytes_processed: u64,
    pub frames_sent: u64,
    pub errors: u64,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct StreamMetrics {
    pub stream_id: StreamId,
    pub session_id: SessionId,
    pub created_at: Instant,
    pub completed_at: Option<Instant>,
    pub frames_generated: u64,
    pub bytes_sent: u64,
    pub average_priority: f64,
    pub processing_times: Vec<Duration>,
}

#[derive(Debug, Clone)]
pub struct TimestampedMetrics {
    pub timestamp: Instant,
    pub metrics: PerformanceMetrics,
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

impl MetricsCollectorGat for InMemoryMetricsCollector {
    type IncrementCounterFuture<'a>
        = impl std::future::Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    type SetGaugeFuture<'a>
        = impl std::future::Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    type RecordTimingFuture<'a>
        = impl std::future::Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    fn increment_counter<'a>(
        &'a self,
        name: &'a str,
        value: u64,
        _tags: HashMap<String, String>,
    ) -> Self::IncrementCounterFuture<'a> {
        async move {
            let mut metrics = self.metrics.write();

            match name {
                "sessions_created" => metrics.total_sessions_created += value,
                "streams_created" => metrics.total_streams_created += value,
                "frames_processed" => metrics.frames_processed += value,
                "bytes_streamed" => metrics.bytes_streamed += value,
                "errors" => metrics.error_count += value,
                _ => {} // Unknown metric
            }

            Ok(())
        }
    }

    fn set_gauge<'a>(
        &'a self,
        name: &'a str,
        value: f64,
        _tags: HashMap<String, String>,
    ) -> Self::SetGaugeFuture<'a> {
        async move {
            let mut metrics = self.metrics.write();

            match name {
                "active_sessions" => metrics.active_sessions = value as usize,
                "active_streams" => metrics.active_streams = value as usize,
                "average_frame_processing_time_ms" => {
                    metrics.average_frame_processing_time = Duration::from_millis(value as u64);
                }
                _ => {} // Unknown metric
            }

            Ok(())
        }
    }

    fn record_timing<'a>(
        &'a self,
        name: &'a str,
        duration: Duration,
        tags: HashMap<String, String>,
    ) -> Self::RecordTimingFuture<'a> {
        async move {
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
            if let Some(session_id_str) = tags.get("session_id")
                && let Ok(session_id) = SessionId::from_string(session_id_str)
            {
                let mut session_metrics = self.session_metrics.write();
                if let Some(session_metric) = session_metrics.get_mut(&session_id) {
                    session_metric.last_activity = Instant::now();
                }
            }

            if let Some(stream_id_str) = tags.get("stream_id")
                && let Ok(stream_id) = StreamId::from_string(stream_id_str)
            {
                let mut stream_metrics = self.stream_metrics.write();
                if let Some(stream_metric) = stream_metrics.get_mut(&stream_id) {
                    stream_metric.processing_times.push(duration);
                }
            }

            Ok(())
        }
    }
}

impl SessionMetricsGat for InMemoryMetricsCollector {
    type RecordSessionCreatedFuture<'a>
        = impl std::future::Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    type RecordSessionEndedFuture<'a>
        = impl std::future::Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    type RecordStreamCreatedFuture<'a>
        = impl std::future::Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    type RecordStreamCompletedFuture<'a>
        = impl std::future::Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    fn record_session_created(
        &self,
        session_id: SessionId,
        metadata: HashMap<String, String>,
    ) -> Self::RecordSessionCreatedFuture<'_> {
        async move {
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

            self.session_metrics
                .write()
                .insert(session_id, session_metric);

            // Record in time series
            self.record_time_series_snapshot().await?;

            Ok(())
        }
    }

    fn record_session_ended(&self, session_id: SessionId) -> Self::RecordSessionEndedFuture<'_> {
        async move {
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
    }

    fn record_stream_created(
        &self,
        stream_id: StreamId,
        session_id: SessionId,
    ) -> Self::RecordStreamCreatedFuture<'_> {
        async move {
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
    }

    fn record_stream_completed(
        &self,
        stream_id: StreamId,
    ) -> Self::RecordStreamCompletedFuture<'_> {
        async move {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_in_memory_metrics_collector() {
        let collector = InMemoryMetricsCollector::new();
        let session_id = SessionId::new();

        // Test session creation
        collector
            .record_session_created(session_id, HashMap::new())
            .await
            .unwrap();

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

        collector
            .increment_counter("frames_processed", 100, HashMap::new())
            .await
            .unwrap();
        collector
            .set_gauge("active_sessions", 5.0, HashMap::new())
            .await
            .unwrap();

        let prometheus_output = collector.export_prometheus();
        assert!(prometheus_output.contains("pjs_frames_processed_total {} 100"));
        assert!(prometheus_output.contains("pjs_active_sessions {} 5"));
    }

    #[tokio::test]
    async fn test_record_stream_created() {
        let collector = InMemoryMetricsCollector::new();
        let session_id = SessionId::new();
        let stream_id = StreamId::new();

        // Create session first
        collector
            .record_session_created(session_id, HashMap::new())
            .await
            .unwrap();

        // Record stream creation
        collector
            .record_stream_created(stream_id, session_id)
            .await
            .unwrap();

        let snapshot = collector.get_performance_snapshot();
        assert_eq!(snapshot.active_streams, 1);
        assert_eq!(snapshot.total_streams_created, 1);

        // Verify session metrics updated
        let session_metrics = collector.get_session_metrics(session_id).unwrap();
        assert_eq!(session_metrics.streams_created, 1);

        // Verify stream metrics created
        let stream_metrics = collector.get_stream_metrics(stream_id).unwrap();
        assert_eq!(stream_metrics.stream_id, stream_id);
        assert_eq!(stream_metrics.session_id, session_id);
    }

    #[tokio::test]
    async fn test_record_stream_completed() {
        let collector = InMemoryMetricsCollector::new();
        let session_id = SessionId::new();
        let stream_id = StreamId::new();

        // Setup: create session and stream
        collector
            .record_session_created(session_id, HashMap::new())
            .await
            .unwrap();
        collector
            .record_stream_created(stream_id, session_id)
            .await
            .unwrap();

        assert_eq!(collector.get_performance_snapshot().active_streams, 1);

        // Complete the stream
        collector.record_stream_completed(stream_id).await.unwrap();

        let snapshot = collector.get_performance_snapshot();
        assert_eq!(snapshot.active_streams, 0);

        // Verify stream metrics marked as completed
        let stream_metrics = collector.get_stream_metrics(stream_id).unwrap();
        assert!(stream_metrics.completed_at.is_some());
    }

    #[tokio::test]
    async fn test_record_timing_with_session_tags() {
        let collector = InMemoryMetricsCollector::new();
        let session_id = SessionId::new();

        // Create session
        collector
            .record_session_created(session_id, HashMap::new())
            .await
            .unwrap();

        let session_metrics_before = collector.get_session_metrics(session_id).unwrap();
        let last_activity_before = session_metrics_before.last_activity;

        // Wait briefly to ensure time difference
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        // Record timing with session tag
        let mut tags = HashMap::new();
        tags.insert("session_id".to_string(), session_id.to_string());

        collector
            .record_timing("operation", std::time::Duration::from_millis(50), tags)
            .await
            .unwrap();

        // Verify last_activity was updated
        let session_metrics_after = collector.get_session_metrics(session_id).unwrap();
        assert!(session_metrics_after.last_activity > last_activity_before);
    }

    #[tokio::test]
    async fn test_record_timing_with_stream_tags() {
        let collector = InMemoryMetricsCollector::new();
        let session_id = SessionId::new();
        let stream_id = StreamId::new();

        // Setup: create session and stream
        collector
            .record_session_created(session_id, HashMap::new())
            .await
            .unwrap();
        collector
            .record_stream_created(stream_id, session_id)
            .await
            .unwrap();

        // Record timing with stream tag
        let mut tags = HashMap::new();
        tags.insert("stream_id".to_string(), stream_id.to_string());

        collector
            .record_timing(
                "stream_operation",
                std::time::Duration::from_millis(75),
                tags,
            )
            .await
            .unwrap();

        // Verify processing_times was updated
        let stream_metrics = collector.get_stream_metrics(stream_id).unwrap();
        assert_eq!(stream_metrics.processing_times.len(), 1);
        assert_eq!(
            stream_metrics.processing_times[0],
            std::time::Duration::from_millis(75)
        );
    }

    #[tokio::test]
    async fn test_time_series_snapshot_boundary() {
        let collector = InMemoryMetricsCollector::new();

        // Record multiple snapshots
        for i in 0..5 {
            let session_id = SessionId::new();
            let mut metadata = HashMap::new();
            metadata.insert("iteration".to_string(), i.to_string());

            collector
                .record_session_created(session_id, metadata)
                .await
                .unwrap();
        }

        // Verify time series contains snapshots
        let time_series = collector.get_time_series(60);
        assert!(time_series.len() >= 5);

        // Verify each snapshot has increasing session counts
        for (i, snapshot) in time_series.iter().enumerate() {
            assert!(snapshot.metrics.total_sessions_created >= (i + 1) as u64);
        }
    }
}
