//! Streaming orchestrator using Clean Architecture principles
//!
//! This service demonstrates how domain logic can now work with
//! abstract ports instead of concrete infrastructure implementations.

use crate::domain::{
    DomainResult,
    entities::{Frame},
    value_objects::{SessionId, StreamId, Priority},
    ports::{
        writer::{WriterFactory, WriterConfig},
        repositories::{
            StreamSessionRepository, FrameRepository, 
            EventRepository, CacheRepository
        },
        EventPublisher, MetricsCollector,
    },
    events::DomainEvent,
};
use async_trait::async_trait;
use chrono::Utc;
use std::{
    collections::HashMap,
    sync::Arc,
    time::Duration,
};

/// High-level streaming orchestrator that uses only domain abstractions
/// 
/// This service demonstrates Clean Architecture by depending only on
/// interfaces (ports) rather than concrete implementations.
pub struct StreamingOrchestrator {
    // Repository ports
    session_repository: Arc<dyn StreamSessionRepository>,
    frame_repository: Arc<dyn FrameRepository>,
    event_repository: Arc<dyn EventRepository>,
    cache: Arc<dyn CacheRepository>,
    
    // I/O ports
    writer_factory: Arc<dyn WriterFactory>,
    event_publisher: Arc<dyn EventPublisher>,
    metrics: Arc<dyn MetricsCollector>,
}

impl StreamingOrchestrator {
    pub fn new(
        session_repository: Arc<dyn StreamSessionRepository>,
        frame_repository: Arc<dyn FrameRepository>,
        event_repository: Arc<dyn EventRepository>,
        cache: Arc<dyn CacheRepository>,
        writer_factory: Arc<dyn WriterFactory>,
        event_publisher: Arc<dyn EventPublisher>,
        metrics: Arc<dyn MetricsCollector>,
    ) -> Self {
        Self {
            session_repository,
            frame_repository,
            event_repository,
            cache,
            writer_factory,
            event_publisher,
            metrics,
        }
    }

    /// Stream data with priority-based optimization
    /// 
    /// This method demonstrates how domain logic can work with abstract
    /// I/O operations without knowing the underlying transport mechanism.
    pub async fn stream_with_priority(
        &self,
        session_id: SessionId,
        stream_id: StreamId,
        priority_threshold: Priority,
        connection_id: &str,
    ) -> DomainResult<StreamingStats> {
        // 1. Validate session exists
        let _session = self.session_repository
            .find_session(session_id)
            .await?
            .ok_or_else(|| {
                crate::domain::DomainError::SessionNotFound(format!("Session {session_id} not found"))
            })?;

        // 2. Check cache for recent frames
        let cache_key = format!("frames:{}:{}", stream_id, priority_threshold.value());
        let cached_frames: Option<Vec<Frame>> = if let Some(bytes) = self.cache.get_bytes(&cache_key).await? {
            Some(serde_json::from_slice(&bytes)
                .map_err(|e| crate::domain::DomainError::Logic(format!("Cache deserialization failed: {e}")))?)
        } else {
            None
        };

        let frames = if let Some(cached) = cached_frames {
            cached
        } else {
            // 3. Fetch frames from repository with priority filtering
            let frame_query = self.frame_repository
                .get_frames_by_stream(
                    stream_id, 
                    Some(priority_threshold),
                    crate::domain::ports::repositories::Pagination {
                        offset: 0,
                        limit: 1000,
                        sort_by: Some("priority".to_string()),
                        sort_order: crate::domain::ports::repositories::SortOrder::Descending,
                    }
                )
                .await?;

            // 4. Cache the result for next time
            let serialized = serde_json::to_vec(&frame_query.frames)
                .map_err(|e| crate::domain::DomainError::Logic(format!("Cache serialization failed: {e}")))?;
            self.cache.set_bytes(
                &cache_key,
                serialized,
                Some(Duration::from_secs(300)), // 5 minute TTL
            ).await?;

            frame_query.frames
        };

        // 5. Create appropriate writer for the connection
        let writer_config = WriterConfig {
            buffer_size: 1024,
            write_timeout: Duration::from_secs(30),
            enable_compression: true,
            max_frame_size: 1024 * 1024,
            backpressure_strategy: crate::domain::ports::writer::BackpressureStrategy::DropLowPriority,
        };

        let mut frame_writer = self.writer_factory
            .create_frame_writer(connection_id, writer_config)
            .await?;

        // 6. Stream frames with priority handling
        let start_time = std::time::Instant::now();
        let mut stats = StreamingStats::default();

        // Group frames by priority for optimal streaming
        let mut priority_groups: HashMap<u8, Vec<Frame>> = HashMap::new();
        for frame in frames {
            let priority = frame.priority();
            priority_groups.entry(priority.value()).or_insert_with(Vec::new).push(frame);
        }

        // Stream in priority order (highest first)
        let mut sorted_priorities: Vec<u8> = priority_groups.keys().cloned().collect();
        sorted_priorities.sort_by(|a, b| b.cmp(a)); // Descending order

        for priority in sorted_priorities {
            if let Some(frames) = priority_groups.get(&priority) {
                // Use priority-aware streaming
                frame_writer.write_frames_by_priority(frames.clone()).await?;
                
                stats.frames_sent += frames.len() as u64;
                stats.bytes_sent += frames.iter()
                    .map(|f| f.estimated_size() as u64)
                    .sum::<u64>();
                
                // Record metrics for this priority level
                self.metrics.increment_counter(
                    "frames_streamed_by_priority",
                    frames.len() as u64,
                    maplit::hashmap! {
                        "priority".to_string() => priority.to_string(),
                        "session_id".to_string() => session_id.to_string(),
                    }
                ).await?;
            }
        }

        // TODO: Add flush method to FrameWriter trait or use close()
        // 7. Finalize streaming
        // frame_writer.flush().await?;
        let streaming_duration = start_time.elapsed();
        
        stats.duration = streaming_duration;
        stats.throughput_mbps = (stats.bytes_sent as f64 * 8.0) / 
            (streaming_duration.as_secs_f64() * 1_000_000.0);

        // 8. Record completion event
        let completion_event = DomainEvent::StreamCompleted {
            session_id,
            stream_id,
            timestamp: Utc::now(),
        };

        self.event_repository.store_event(completion_event.clone(), 0).await?;
        self.event_publisher.publish(completion_event).await?;

        // 9. Record final metrics
        self.metrics.record_timing(
            "stream_duration",
            streaming_duration,
            maplit::hashmap! {
                "session_id".to_string() => session_id.to_string(),
                "stream_id".to_string() => stream_id.to_string(),
            }
        ).await?;

        Ok(stats)
    }

    /// Adaptive streaming that adjusts to network conditions
    /// 
    /// Demonstrates how domain logic can use connection monitoring
    /// to make intelligent streaming decisions.
    pub async fn adaptive_stream(
        &self,
        session_id: SessionId,
        stream_id: StreamId,
        connection_id: &str,
    ) -> DomainResult<StreamingStats> {
        // This would use ConnectionMonitor to adapt streaming strategy
        // based on real-time network conditions
        
        // For now, implement a simplified version
        let base_priority = Priority::MEDIUM;
        self.stream_with_priority(session_id, stream_id, base_priority, connection_id).await
    }

    /// Batch multiple streams efficiently
    /// 
    /// Shows how complex domain operations can be built from simple ports.
    pub async fn batch_stream_multiple(
        &self,
        streams: Vec<(SessionId, StreamId)>,
        connection_id: &str,
    ) -> DomainResult<Vec<StreamingStats>> {
        let mut results = Vec::new();

        for (session_id, stream_id) in streams {
            let stats = self.adaptive_stream(session_id, stream_id, connection_id).await?;
            results.push(stats);
        }

        Ok(results)
    }

    /// Get streaming performance metrics
    pub async fn get_performance_metrics(&self) -> DomainResult<StreamingPerformanceMetrics> {
        let cache_stats = self.cache.get_stats().await?;
        
        Ok(StreamingPerformanceMetrics {
            cache_hit_rate: cache_stats.hit_rate,
            cache_memory_usage: cache_stats.memory_usage_bytes,
            total_sessions: 0, // Would get from session repository
            active_streams: 0,  // Would get from stream repository
        })
    }
}

/// Statistics from a streaming operation
#[derive(Debug, Clone, Default)]
pub struct StreamingStats {
    pub frames_sent: u64,
    pub bytes_sent: u64,
    pub duration: Duration,
    pub throughput_mbps: f64,
    pub errors_encountered: u32,
}

/// Overall performance metrics
#[derive(Debug, Clone)]
pub struct StreamingPerformanceMetrics {
    pub cache_hit_rate: f64,
    pub cache_memory_usage: u64,
    pub total_sessions: u64,
    pub active_streams: u64,
}

/// Factory for creating streaming orchestrators with dependency injection
/// 
/// This demonstrates the Dependency Injection pattern working with
/// Clean Architecture principles.
pub struct StreamingOrchestratorFactory;

impl StreamingOrchestratorFactory {
    // TODO: Re-enable when infrastructure module is fixed
    // pub fn create_with_in_memory_adapters() -> StreamingOrchestrator {
        // use crate::infrastructure::adapters::{
        //     InMemoryStreamSessionRepository,
        //     InMemoryFrameRepository, 
        //     InMemoryCache,
        //     TokioWriterFactory,
        // };
        
        // Create all dependencies using concrete adapters
        // let session_repository = Arc::new(InMemoryStreamSessionRepository::new()) 
        //     as Arc<dyn StreamSessionRepository>;
        
        // let frame_repository = Arc::new(InMemoryFrameRepository::new())
        //     as Arc<dyn FrameRepository>;
            
        // let cache = Arc::new(InMemoryCache::new())
        //     as Arc<dyn CacheRepository>;
            
        // let writer_factory = Arc::new(TokioWriterFactory)
        //     as Arc<dyn WriterFactory>;
            
        // For event repository, we'd create a concrete implementation
        // For now, use a placeholder
        // let event_repository = Arc::new(MockEventRepository)
        //     as Arc<dyn EventRepository>;
            
        // let event_publisher = Arc::new(MockEventPublisher)
        //     as Arc<dyn EventPublisher>;
            
        // let metrics = Arc::new(MockMetricsCollector)
        //     as Arc<dyn MetricsCollector>;

        // StreamingOrchestrator::new(
        //     session_repository,
        //     frame_repository,
        //     event_repository,
        //     cache,
        //     writer_factory,
        //     event_publisher,
        //     metrics,
        // )
    // }
}

// Mock implementations for demonstration
pub struct MockEventRepository;

#[async_trait]
impl EventRepository for MockEventRepository {
    async fn store_event(&self, _event: DomainEvent, _sequence: u64) -> DomainResult<()> {
        Ok(())
    }
    
    async fn store_events(&self, _events: Vec<DomainEvent>) -> DomainResult<()> {
        Ok(())
    }
    
    async fn get_events_for_session(&self, _session_id: SessionId, _from_sequence: Option<u64>, _limit: Option<usize>) -> DomainResult<Vec<DomainEvent>> {
        Ok(Vec::new())
    }
    
    async fn get_events_for_stream(&self, _stream_id: StreamId, _from_sequence: Option<u64>, _limit: Option<usize>) -> DomainResult<Vec<DomainEvent>> {
        Ok(Vec::new())
    }
    
    async fn get_events_by_type(&self, _event_types: Vec<String>, _time_range: Option<(chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)>) -> DomainResult<Vec<DomainEvent>> {
        Ok(Vec::new())
    }
    
    async fn get_latest_sequence(&self) -> DomainResult<u64> {
        Ok(0)
    }
    
    async fn replay_session_events(&self, _session_id: SessionId) -> DomainResult<Vec<DomainEvent>> {
        Ok(Vec::new())
    }
}

pub struct MockEventPublisher;

#[async_trait]
impl EventPublisher for MockEventPublisher {
    async fn publish(&self, _event: DomainEvent) -> DomainResult<()> {
        Ok(())
    }
    
    async fn publish_batch(&self, _events: Vec<DomainEvent>) -> DomainResult<()> {
        Ok(())
    }
}

pub struct MockMetricsCollector;

#[async_trait]
impl MetricsCollector for MockMetricsCollector {
    async fn increment_counter(&self, _name: &str, _value: u64, _tags: HashMap<String, String>) -> DomainResult<()> {
        Ok(())
    }
    
    async fn set_gauge(&self, _name: &str, _value: f64, _tags: HashMap<String, String>) -> DomainResult<()> {
        Ok(())
    }
    
    async fn record_timing(&self, _name: &str, _duration: Duration, _tags: HashMap<String, String>) -> DomainResult<()> {
        Ok(())
    }
    
    async fn record_session_created(&self, _session_id: SessionId, _metadata: HashMap<String, String>) -> DomainResult<()> {
        Ok(())
    }
    
    async fn record_session_ended(&self, _session_id: SessionId) -> DomainResult<()> {
        Ok(())
    }
    
    async fn record_stream_created(&self, _stream_id: StreamId, _session_id: SessionId) -> DomainResult<()> {
        Ok(())
    }
    
    async fn record_stream_completed(&self, _stream_id: StreamId) -> DomainResult<()> {
        Ok(())
    }
}