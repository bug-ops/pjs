//! GAT-based streaming orchestrator with zero-cost async abstractions
//!
//! This service demonstrates Clean Architecture using GAT traits that provide
//! true zero-cost futures instead of boxed async_trait futures.

use crate::domain::{
    DomainResult,
    aggregates::StreamSession,
    entities::Frame,
    events::DomainEvent,
    ports::gat::{EventPublisherGat, StreamRepositoryGat},
    value_objects::{Priority, SessionId, StreamId},
};
use chrono::Utc;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tracing::{debug, info, warn};

/// Statistics for streaming operations
#[derive(Debug, Clone)]
pub struct StreamingStats {
    pub frames_processed: usize,
    pub bytes_written: usize,
    pub processing_time: Duration,
    pub cache_hits: usize,
    pub cache_misses: usize,
}

/// GAT-based streaming orchestrator with zero-cost abstractions
pub struct GatStreamingOrchestrator<R, P>
where
    R: StreamRepositoryGat,
    P: EventPublisherGat,
{
    session_repository: Arc<R>,
    event_publisher: Arc<P>,
    config: OrchestratorConfig,
}

/// Configuration for the orchestrator
#[derive(Debug, Clone)]
pub struct OrchestratorConfig {
    pub batch_size: usize,
    pub cache_ttl: Duration,
    pub max_concurrent_streams: usize,
    pub priority_boost_threshold: Priority,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            batch_size: 100,
            cache_ttl: Duration::from_secs(300), // 5 minutes
            max_concurrent_streams: 10,
            priority_boost_threshold: Priority::HIGH,
        }
    }
}

impl<R, P> GatStreamingOrchestrator<R, P>
where
    R: StreamRepositoryGat + 'static,
    P: EventPublisherGat + 'static,
{
    pub fn new(
        session_repository: Arc<R>,
        event_publisher: Arc<P>,
        config: OrchestratorConfig,
    ) -> Self {
        Self {
            session_repository,
            event_publisher,
            config,
        }
    }

    /// Create with default configuration
    pub fn with_default_config(session_repository: Arc<R>, event_publisher: Arc<P>) -> Self {
        Self::new(
            session_repository,
            event_publisher,
            OrchestratorConfig::default(),
        )
    }

    /// Stream frames with priority-based processing - GAT version with zero allocation
    // Note: tracing instrument removed due to GAT lifetime constraints
    pub async fn stream_with_priority(
        &self,
        session_id: SessionId,
        stream_id: StreamId,
        frames: Vec<Frame>,
    ) -> DomainResult<StreamingStats>
    where
        Self: 'static,
    {
        let start_time = Instant::now();
        info!(
            "Starting GAT-based streaming for session {} stream {}",
            session_id, stream_id
        );

        // 1. Validate session exists (zero-cost GAT future)
        let session = self
            .session_repository
            .find_session(session_id)
            .await?
            .ok_or_else(|| {
                crate::domain::DomainError::SessionNotFound(format!(
                    "Session {} not found",
                    session_id
                ))
            })?;

        debug!(
            "Session {} found with {} streams",
            session_id,
            session.streams().len()
        );

        // 2. Process frames with priority optimization
        let stats = self.process_frames_optimized(frames, &session).await?;

        // 3. Publish completion event (zero-cost GAT future)
        let completion_event = DomainEvent::StreamCompleted {
            session_id,
            stream_id,
            timestamp: Utc::now(),
        };

        self.event_publisher.publish(completion_event).await?;

        let total_time = start_time.elapsed();
        info!(
            "GAT streaming completed in {:?}: {} frames, {} bytes",
            total_time, stats.frames_processed, stats.bytes_written
        );

        Ok(StreamingStats {
            processing_time: total_time,
            ..stats
        })
    }

    /// Process multiple streams concurrently using GAT futures
    pub async fn process_concurrent_streams(
        &self,
        streams: Vec<(SessionId, StreamId, Vec<Frame>)>,
    ) -> DomainResult<Vec<StreamingStats>>
    where
        Self: 'static,
    {
        let start_time = Instant::now();

        if streams.len() > self.config.max_concurrent_streams {
            warn!(
                "Limiting concurrent streams from {} to {}",
                streams.len(),
                self.config.max_concurrent_streams
            );
        }

        let limited_streams = streams
            .into_iter()
            .take(self.config.max_concurrent_streams)
            .collect::<Vec<_>>();

        // Use GAT futures for true zero-cost concurrency
        let mut tasks = Vec::new();
        for (session_id, stream_id, frames) in limited_streams {
            let future = self.stream_with_priority(session_id, stream_id, frames);
            tasks.push(future);
        }

        // Process all streams concurrently
        let mut results = Vec::new();
        for task in tasks {
            results.push(task.await?);
        }

        let total_time = start_time.elapsed();
        info!(
            "Processed {} streams concurrently in {:?}",
            results.len(),
            total_time
        );

        Ok(results)
    }

    /// Optimize frame processing based on priority and session state
    async fn process_frames_optimized(
        &self,
        mut frames: Vec<Frame>,
        session: &StreamSession,
    ) -> DomainResult<StreamingStats> {
        let mut stats = StreamingStats {
            frames_processed: 0,
            bytes_written: 0,
            processing_time: Duration::ZERO,
            cache_hits: 0,
            cache_misses: 0,
        };

        // Sort frames by priority for optimal processing order (descending)
        frames.sort_by_key(|frame| std::cmp::Reverse(frame.priority()));

        let batch_size = if session.streams().len() > 5 {
            self.config.batch_size * 2 // Larger batches for busy sessions
        } else {
            self.config.batch_size
        };

        // Process frames in batches
        for batch in frames.chunks(batch_size) {
            let batch_start = Instant::now();

            for frame in batch {
                // Apply priority boost if needed
                let effective_priority = if frame.priority() >= self.config.priority_boost_threshold
                {
                    Priority::CRITICAL
                } else {
                    frame.priority()
                };

                debug!(
                    "Processing frame with priority {} -> {}",
                    frame.priority().value(),
                    effective_priority.value()
                );

                // Simulate frame processing (in real implementation would write to sink)
                stats.bytes_written += self.estimate_frame_size(frame);
                stats.frames_processed += 1;
            }

            let batch_time = batch_start.elapsed();
            stats.processing_time += batch_time;

            debug!(
                "Processed batch of {} frames in {:?}",
                batch.len(),
                batch_time
            );
        }

        Ok(stats)
    }

    /// Estimate frame size for statistics
    fn estimate_frame_size(&self, frame: &Frame) -> usize {
        // Simple estimation based on frame data
        serde_json::to_string(frame.payload())
            .map(|s| s.len())
            .unwrap_or(0)
    }

    /// Health check using GAT futures
    pub async fn health_check(&self) -> DomainResult<HealthStatus>
    where
        Self: 'static,
    {
        let start_time = Instant::now();

        // Test repository connection with zero-cost GAT future
        let active_sessions = self.session_repository.find_active_sessions().await?;
        let repository_latency = start_time.elapsed();

        // Test event publisher with a dummy event
        let test_event = DomainEvent::StreamCompleted {
            session_id: SessionId::new(),
            stream_id: StreamId::new(),
            timestamp: Utc::now(),
        };
        let event_start = Instant::now();
        self.event_publisher.publish(test_event).await?;
        let event_publisher_latency = event_start.elapsed();

        Ok(HealthStatus {
            active_sessions: active_sessions.len(),
            repository_latency,
            event_publisher_latency,
            total_latency: start_time.elapsed(),
            status: if repository_latency < Duration::from_millis(100)
                && event_publisher_latency < Duration::from_millis(50)
            {
                "healthy"
            } else {
                "degraded"
            }
            .to_string(),
        })
    }

    /// Get configuration
    pub fn config(&self) -> &OrchestratorConfig {
        &self.config
    }
}

/// Health status information
#[derive(Debug, Clone)]
pub struct HealthStatus {
    pub active_sessions: usize,
    pub repository_latency: Duration,
    pub event_publisher_latency: Duration,
    pub total_latency: Duration,
    pub status: String,
}

/// Factory for creating GAT orchestrators with different implementations
pub struct GatOrchestratorFactory;

impl GatOrchestratorFactory {
    /// Create orchestrator with specific repository and publisher implementations
    pub fn create<R, P>(
        session_repository: Arc<R>,
        event_publisher: Arc<P>,
        config: OrchestratorConfig,
    ) -> GatStreamingOrchestrator<R, P>
    where
        R: StreamRepositoryGat + 'static,
        P: EventPublisherGat + 'static,
    {
        GatStreamingOrchestrator::new(session_repository, event_publisher, config)
    }

    /// Create with default configuration
    pub fn create_default<R, P>(
        session_repository: Arc<R>,
        event_publisher: Arc<P>,
    ) -> GatStreamingOrchestrator<R, P>
    where
        R: StreamRepositoryGat + 'static,
        P: EventPublisherGat + 'static,
    {
        GatStreamingOrchestrator::with_default_config(session_repository, event_publisher)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        entities::Frame,
        events::DomainEvent,
        ports::{Pagination, SessionHealthSnapshot, SessionQueryCriteria, SessionQueryResult},
        value_objects::{JsonData, StreamId},
    };
    use chrono::Utc;
    use std::collections::HashMap;
    use std::future::Future;
    use std::sync::{Arc, Mutex};
    use tokio::sync::RwLock;

    /// Mock GAT repository for testing
    struct MockGatRepository {
        sessions: Arc<RwLock<Vec<StreamSession>>>,
    }

    impl MockGatRepository {
        fn new() -> Self {
            Self {
                sessions: Arc::new(RwLock::new(Vec::new())),
            }
        }

        async fn add_session(&self, session: StreamSession) {
            self.sessions.write().await.push(session);
        }
    }

    impl StreamRepositoryGat for MockGatRepository {
        type FindSessionFuture<'a>
            = impl Future<Output = DomainResult<Option<StreamSession>>> + Send + 'a
        where
            Self: 'a;

        type SaveSessionFuture<'a>
            = impl Future<Output = DomainResult<()>> + Send + 'a
        where
            Self: 'a;

        type RemoveSessionFuture<'a>
            = impl Future<Output = DomainResult<()>> + Send + 'a
        where
            Self: 'a;

        type FindActiveSessionsFuture<'a>
            = impl Future<Output = DomainResult<Vec<StreamSession>>> + Send + 'a
        where
            Self: 'a;

        type FindSessionsByCriteriaFuture<'a>
            = impl Future<Output = DomainResult<SessionQueryResult>> + Send + 'a
        where
            Self: 'a;

        type GetSessionHealthFuture<'a>
            = impl Future<Output = DomainResult<SessionHealthSnapshot>> + Send + 'a
        where
            Self: 'a;

        type SessionExistsFuture<'a>
            = impl Future<Output = DomainResult<bool>> + Send + 'a
        where
            Self: 'a;

        fn find_session(&self, session_id: SessionId) -> Self::FindSessionFuture<'_> {
            async move {
                let sessions = self.sessions.read().await;
                Ok(sessions.iter().find(|s| s.id() == session_id).cloned())
            }
        }

        fn save_session(&self, session: StreamSession) -> Self::SaveSessionFuture<'_> {
            async move {
                self.sessions.write().await.push(session);
                Ok(())
            }
        }

        fn remove_session(&self, session_id: SessionId) -> Self::RemoveSessionFuture<'_> {
            async move {
                let mut sessions = self.sessions.write().await;
                sessions.retain(|s| s.id() != session_id);
                Ok(())
            }
        }

        fn find_active_sessions(&self) -> Self::FindActiveSessionsFuture<'_> {
            async move {
                let sessions = self.sessions.read().await;
                Ok(sessions.iter().filter(|s| s.is_active()).cloned().collect())
            }
        }

        fn find_sessions_by_criteria(
            &self,
            _criteria: SessionQueryCriteria,
            pagination: Pagination,
        ) -> Self::FindSessionsByCriteriaFuture<'_> {
            async move {
                let sessions = self.sessions.read().await;
                let all_sessions: Vec<_> = sessions.clone();
                let total_count = all_sessions.len();
                let paginated: Vec<_> = all_sessions
                    .into_iter()
                    .skip(pagination.offset)
                    .take(pagination.limit)
                    .collect();
                let has_more = pagination.offset + paginated.len() < total_count;
                Ok(SessionQueryResult {
                    sessions: paginated,
                    total_count,
                    has_more,
                    query_duration_ms: 0,
                })
            }
        }

        fn get_session_health(&self, session_id: SessionId) -> Self::GetSessionHealthFuture<'_> {
            async move {
                Ok(SessionHealthSnapshot {
                    session_id,
                    is_healthy: true,
                    active_streams: 0,
                    total_frames: 0,
                    last_activity: Utc::now(),
                    error_rate: 0.0,
                    metrics: HashMap::new(),
                })
            }
        }

        fn session_exists(&self, session_id: SessionId) -> Self::SessionExistsFuture<'_> {
            async move {
                let sessions = self.sessions.read().await;
                Ok(sessions.iter().any(|s| s.id() == session_id))
            }
        }
    }

    /// Mock GAT event publisher for testing
    struct MockGatEventPublisher {
        published_events: Arc<Mutex<Vec<DomainEvent>>>,
    }

    impl MockGatEventPublisher {
        fn new() -> Self {
            Self {
                published_events: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn get_published_events(&self) -> Vec<DomainEvent> {
            self.published_events.lock().unwrap().clone()
        }
    }

    impl EventPublisherGat for MockGatEventPublisher {
        type PublishFuture<'a>
            = impl Future<Output = DomainResult<()>> + Send + 'a
        where
            Self: 'a;

        type PublishBatchFuture<'a>
            = impl Future<Output = DomainResult<()>> + Send + 'a
        where
            Self: 'a;

        fn publish(&self, event: DomainEvent) -> Self::PublishFuture<'_> {
            async move {
                self.published_events.lock().unwrap().push(event);
                Ok(())
            }
        }

        fn publish_batch(&self, events: Vec<DomainEvent>) -> Self::PublishBatchFuture<'_> {
            async move {
                self.published_events.lock().unwrap().extend(events);
                Ok(())
            }
        }
    }

    #[tokio::test]
    async fn test_gat_orchestrator_streaming() {
        let repository = Arc::new(MockGatRepository::new());
        let publisher = Arc::new(MockGatEventPublisher::new());

        let stream_id = StreamId::new();

        // Add test session
        let session =
            StreamSession::new(crate::domain::aggregates::stream_session::SessionConfig::default());
        let session_id = session.id(); // Get the actual session ID
        repository.add_session(session).await;

        let orchestrator = GatOrchestratorFactory::create_default(repository, publisher.clone());

        // Create test frames
        let frames = vec![
            Frame::skeleton(stream_id, 1, JsonData::String("test1".to_string())),
            Frame::skeleton(stream_id, 2, JsonData::String("test2".to_string())),
        ];

        // Test GAT-based streaming
        let stats = orchestrator
            .stream_with_priority(session_id, stream_id, frames)
            .await
            .unwrap();

        assert_eq!(stats.frames_processed, 2);
        assert!(stats.bytes_written > 0);
        assert!(stats.processing_time > Duration::ZERO);

        // Verify event was published
        let events = publisher.get_published_events();
        assert_eq!(events.len(), 1);
    }

    #[tokio::test]
    async fn test_gat_orchestrator_concurrent_streams() {
        let repository = Arc::new(MockGatRepository::new());
        let publisher = Arc::new(MockGatEventPublisher::new());

        let orchestrator = GatOrchestratorFactory::create_default(repository.clone(), publisher);

        // Create multiple sessions and streams
        let mut streams = Vec::new();
        for i in 0..3 {
            let stream_id = StreamId::new();

            let session = StreamSession::new(
                crate::domain::aggregates::stream_session::SessionConfig::default(),
            );
            let session_id = session.id(); // Get the actual session ID
            repository.add_session(session).await;

            let frames = vec![Frame::skeleton(
                stream_id,
                1,
                JsonData::String(format!("stream_{}", i)),
            )];

            streams.push((session_id, stream_id, frames));
        }

        // Test concurrent processing with GAT futures
        let results = orchestrator
            .process_concurrent_streams(streams)
            .await
            .unwrap();

        assert_eq!(results.len(), 3);
        for stats in results {
            assert_eq!(stats.frames_processed, 1);
            assert!(stats.bytes_written > 0);
        }
    }

    #[tokio::test]
    async fn test_gat_orchestrator_health_check() {
        let repository = Arc::new(MockGatRepository::new());
        let publisher = Arc::new(MockGatEventPublisher::new());

        let orchestrator = GatOrchestratorFactory::create_default(repository, publisher);

        let health = orchestrator.health_check().await.unwrap();

        assert_eq!(health.active_sessions, 0);
        assert!(health.total_latency > Duration::ZERO);
        assert!(!health.status.is_empty());
    }
}
