//! Repository ports for data persistence
//!
//! These ports define the domain's requirements for data storage,
//! allowing infrastructure adapters to implement various storage backends.

use crate::domain::{
    DomainResult,
    aggregates::StreamSession,
    entities::{Frame, Stream},
    events::DomainEvent,
    value_objects::{JsonPath, Priority, SessionId, StreamId},
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// Enhanced repository for stream sessions with transactional support
#[async_trait]
pub trait StreamSessionRepository: Send + Sync {
    /// Start a new transaction
    async fn begin_transaction(&self) -> DomainResult<Box<dyn SessionTransaction>>;

    /// Find session by ID with read consistency guarantees
    async fn find_session(&self, session_id: SessionId) -> DomainResult<Option<StreamSession>>;

    /// Save session with optimistic concurrency control
    async fn save_session(&self, session: StreamSession, version: Option<u64>)
    -> DomainResult<u64>;

    /// Remove session and all associated data
    async fn remove_session(&self, session_id: SessionId) -> DomainResult<()>;

    /// Find sessions by criteria with pagination
    async fn find_sessions_by_criteria(
        &self,
        criteria: SessionQueryCriteria,
        pagination: Pagination,
    ) -> DomainResult<SessionQueryResult>;

    /// Get session health metrics
    async fn get_session_health(
        &self,
        session_id: SessionId,
    ) -> DomainResult<SessionHealthSnapshot>;

    /// Check if session exists (lightweight operation)
    async fn session_exists(&self, session_id: SessionId) -> DomainResult<bool>;
}

/// Transaction interface for session operations
#[async_trait]
pub trait SessionTransaction: Send + Sync {
    /// Save session within transaction
    async fn save_session(&self, session: StreamSession) -> DomainResult<()>;

    /// Remove session within transaction
    async fn remove_session(&self, session_id: SessionId) -> DomainResult<()>;

    /// Add stream to session within transaction
    async fn add_stream(&self, session_id: SessionId, stream: Stream) -> DomainResult<()>;

    /// Commit all changes
    async fn commit(self: Box<Self>) -> DomainResult<()>;

    /// Rollback all changes
    async fn rollback(self: Box<Self>) -> DomainResult<()>;
}

/// Repository for stream data with efficient querying
#[async_trait]
pub trait StreamDataRepository: Send + Sync {
    /// Store stream with metadata indexing
    async fn store_stream(&self, stream: Stream, metadata: StreamMetadata) -> DomainResult<()>;

    /// Get stream by ID with caching hints
    async fn get_stream(
        &self,
        stream_id: StreamId,
        use_cache: bool,
    ) -> DomainResult<Option<Stream>>;

    /// Delete stream and associated frames
    async fn delete_stream(&self, stream_id: StreamId) -> DomainResult<()>;

    /// Find streams by session with filtering
    async fn find_streams_by_session(
        &self,
        session_id: SessionId,
        filter: StreamFilter,
    ) -> DomainResult<Vec<Stream>>;

    /// Update stream status
    async fn update_stream_status(
        &self,
        stream_id: StreamId,
        status: StreamStatus,
    ) -> DomainResult<()>;

    /// Get stream statistics
    async fn get_stream_statistics(&self, stream_id: StreamId) -> DomainResult<StreamStatistics>;
}

/// Repository for frame data with priority-based access
#[async_trait]
pub trait FrameRepository: Send + Sync {
    /// Store frame with priority indexing
    async fn store_frame(&self, frame: Frame) -> DomainResult<()>;

    /// Store multiple frames efficiently
    async fn store_frames(&self, frames: Vec<Frame>) -> DomainResult<()>;

    /// Get frames by stream with priority filtering
    async fn get_frames_by_stream(
        &self,
        stream_id: StreamId,
        priority_filter: Option<Priority>,
        pagination: Pagination,
    ) -> DomainResult<FrameQueryResult>;

    /// Get frames by JSON path
    async fn get_frames_by_path(
        &self,
        stream_id: StreamId,
        path: JsonPath,
    ) -> DomainResult<Vec<Frame>>;

    /// Delete frames older than specified time
    async fn cleanup_old_frames(&self, older_than: DateTime<Utc>) -> DomainResult<u64>;

    /// Get frame count by priority distribution
    async fn get_frame_priority_distribution(
        &self,
        stream_id: StreamId,
    ) -> DomainResult<PriorityDistribution>;
}

/// Repository for domain events with event sourcing support
#[async_trait]
pub trait EventRepository: Send + Sync {
    /// Store domain event with ordering guarantees
    async fn store_event(&self, event: DomainEvent, sequence: u64) -> DomainResult<()>;

    /// Store multiple events as atomic batch
    async fn store_events(&self, events: Vec<DomainEvent>) -> DomainResult<()>;

    /// Get events for session in chronological order
    async fn get_events_for_session(
        &self,
        session_id: SessionId,
        from_sequence: Option<u64>,
        limit: Option<usize>,
    ) -> DomainResult<Vec<DomainEvent>>;

    /// Get events for stream
    async fn get_events_for_stream(
        &self,
        stream_id: StreamId,
        from_sequence: Option<u64>,
        limit: Option<usize>,
    ) -> DomainResult<Vec<DomainEvent>>;

    /// Get events by type
    async fn get_events_by_type(
        &self,
        event_types: Vec<String>,
        time_range: Option<(DateTime<Utc>, DateTime<Utc>)>,
    ) -> DomainResult<Vec<DomainEvent>>;

    /// Get latest sequence number
    async fn get_latest_sequence(&self) -> DomainResult<u64>;

    /// Replay events for session reconstruction
    async fn replay_session_events(&self, session_id: SessionId) -> DomainResult<Vec<DomainEvent>>;
}

/// Cache abstraction for performance optimization
/// Using bytes for object-safe interface
#[async_trait]
pub trait CacheRepository: Send + Sync {
    /// Get cached value as bytes
    async fn get_bytes(&self, key: &str) -> DomainResult<Option<Vec<u8>>>;

    /// Set cached value as bytes with TTL
    async fn set_bytes(
        &self,
        key: &str,
        value: Vec<u8>,
        ttl: Option<std::time::Duration>,
    ) -> DomainResult<()>;

    /// Remove cached value
    async fn remove(&self, key: &str) -> DomainResult<()>;

    /// Clear all cached values with prefix
    async fn clear_prefix(&self, prefix: &str) -> DomainResult<()>;

    /// Get cache statistics
    async fn get_stats(&self) -> DomainResult<CacheStatistics>;
}

/// Helper extension trait for type-safe cache operations
/// This can be used as extension methods on implementers
#[allow(async_fn_in_trait)]
pub trait CacheExtensions: CacheRepository {
    /// Get cached value with deserialization
    async fn get_typed<T>(&self, key: &str) -> DomainResult<Option<T>>
    where
        T: serde::de::DeserializeOwned,
    {
        if let Some(bytes) = self.get_bytes(key).await? {
            let value = serde_json::from_slice(&bytes).map_err(|e| {
                crate::domain::DomainError::Logic(format!("Deserialization failed: {e}"))
            })?;
            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    /// Set cached value with serialization
    async fn set_typed<T>(
        &self,
        key: &str,
        value: &T,
        ttl: Option<std::time::Duration>,
    ) -> DomainResult<()>
    where
        T: serde::Serialize,
    {
        let bytes = serde_json::to_vec(value)
            .map_err(|e| crate::domain::DomainError::Logic(format!("Serialization failed: {e}")))?;
        self.set_bytes(key, bytes, ttl).await
    }
}

// Blanket implementation for all CacheRepository implementers
impl<T: CacheRepository> CacheExtensions for T {}

// Supporting types for query operations

/// Criteria for session queries
#[derive(Debug, Clone, Default)]
pub struct SessionQueryCriteria {
    pub states: Option<Vec<String>>,
    pub created_after: Option<DateTime<Utc>>,
    pub created_before: Option<DateTime<Utc>>,
    pub client_info_pattern: Option<String>,
    pub has_active_streams: Option<bool>,
    pub min_stream_count: Option<usize>,
    pub max_stream_count: Option<usize>,
}

/// Pagination parameters
#[derive(Debug, Clone)]
pub struct Pagination {
    pub offset: usize,
    pub limit: usize,
    pub sort_by: Option<String>,
    pub sort_order: SortOrder,
}

impl Default for Pagination {
    fn default() -> Self {
        Self {
            offset: 0,
            limit: 50,
            sort_by: None,
            sort_order: SortOrder::Ascending,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SortOrder {
    Ascending,
    Descending,
}

/// Result of session query with metadata
#[derive(Debug, Clone)]
pub struct SessionQueryResult {
    pub sessions: Vec<StreamSession>,
    pub total_count: usize,
    pub has_more: bool,
    pub query_duration_ms: u64,
}

/// Snapshot of session health
#[derive(Debug, Clone)]
pub struct SessionHealthSnapshot {
    pub session_id: SessionId,
    pub is_healthy: bool,
    pub active_streams: usize,
    pub total_frames: u64,
    pub last_activity: DateTime<Utc>,
    pub error_rate: f64,
    pub metrics: HashMap<String, f64>,
}

/// Filter for stream queries
#[derive(Debug, Clone, Default)]
pub struct StreamFilter {
    pub statuses: Option<Vec<StreamStatus>>,
    pub min_priority: Option<Priority>,
    pub max_priority: Option<Priority>,
    pub created_after: Option<DateTime<Utc>>,
    pub has_frames: Option<bool>,
}

/// Stream status enumeration
#[derive(Debug, Clone, PartialEq)]
pub enum StreamStatus {
    Created,
    Active,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

/// Stream metadata for indexing
#[derive(Debug, Clone, Default)]
pub struct StreamMetadata {
    pub tags: HashMap<String, String>,
    pub content_type: Option<String>,
    pub estimated_size: Option<u64>,
    pub priority_hints: Vec<Priority>,
}

/// Stream statistics
#[derive(Debug, Clone)]
pub struct StreamStatistics {
    pub total_frames: u64,
    pub total_bytes: u64,
    pub priority_distribution: PriorityDistribution,
    pub avg_frame_size: f64,
    pub creation_time: DateTime<Utc>,
    pub completion_time: Option<DateTime<Utc>>,
    pub processing_duration: Option<std::time::Duration>,
}

/// Result of frame queries
#[derive(Debug, Clone)]
pub struct FrameQueryResult {
    pub frames: Vec<Frame>,
    pub total_count: usize,
    pub has_more: bool,
    pub highest_priority: Option<Priority>,
    pub lowest_priority: Option<Priority>,
}

// Use the canonical PriorityDistribution from events
pub use crate::domain::events::PriorityDistribution;

/// Cache performance statistics
#[derive(Debug, Clone)]
pub struct CacheStatistics {
    pub hit_rate: f64,
    pub miss_rate: f64,
    pub total_keys: u64,
    pub memory_usage_bytes: u64,
    pub eviction_count: u64,
}

impl Default for CacheStatistics {
    fn default() -> Self {
        Self {
            hit_rate: 0.0,
            miss_rate: 0.0,
            total_keys: 0,
            memory_usage_bytes: 0,
            eviction_count: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::Mutex;

    // Mock cache repository for testing CacheExtensions
    struct MockCacheRepository {
        store: Arc<Mutex<HashMap<String, Vec<u8>>>>,
        fail_on_get: bool,
        fail_on_set: bool,
    }

    impl MockCacheRepository {
        fn new() -> Self {
            Self {
                store: Arc::new(Mutex::new(HashMap::new())),
                fail_on_get: false,
                fail_on_set: false,
            }
        }

        fn with_get_failure() -> Self {
            Self {
                store: Arc::new(Mutex::new(HashMap::new())),
                fail_on_get: true,
                fail_on_set: false,
            }
        }

        fn with_set_failure() -> Self {
            Self {
                store: Arc::new(Mutex::new(HashMap::new())),
                fail_on_get: false,
                fail_on_set: true,
            }
        }
    }

    #[async_trait]
    impl CacheRepository for MockCacheRepository {
        async fn get_bytes(&self, key: &str) -> DomainResult<Option<Vec<u8>>> {
            if self.fail_on_get {
                return Err(crate::domain::DomainError::Logic("Get failed".into()));
            }
            Ok(self.store.lock().await.get(key).cloned())
        }

        async fn set_bytes(
            &self,
            key: &str,
            value: Vec<u8>,
            _ttl: Option<Duration>,
        ) -> DomainResult<()> {
            if self.fail_on_set {
                return Err(crate::domain::DomainError::Logic("Set failed".into()));
            }
            self.store.lock().await.insert(key.to_string(), value);
            Ok(())
        }

        async fn remove(&self, key: &str) -> DomainResult<()> {
            self.store.lock().await.remove(key);
            Ok(())
        }

        async fn clear_prefix(&self, prefix: &str) -> DomainResult<()> {
            let mut store = self.store.lock().await;
            store.retain(|k, _| !k.starts_with(prefix));
            Ok(())
        }

        async fn get_stats(&self) -> DomainResult<CacheStatistics> {
            let store = self.store.lock().await;
            Ok(CacheStatistics {
                total_keys: store.len() as u64,
                ..Default::default()
            })
        }
    }

    // Tests for Pagination
    #[test]
    fn test_pagination_default() {
        let pagination = Pagination::default();
        assert_eq!(pagination.offset, 0);
        assert_eq!(pagination.limit, 50);
        assert!(pagination.sort_by.is_none());
        assert_eq!(pagination.sort_order, SortOrder::Ascending);
    }

    #[test]
    fn test_pagination_custom() {
        let pagination = Pagination {
            offset: 10,
            limit: 100,
            sort_by: Some("created_at".to_string()),
            sort_order: SortOrder::Descending,
        };
        assert_eq!(pagination.offset, 10);
        assert_eq!(pagination.limit, 100);
        assert_eq!(pagination.sort_by, Some("created_at".to_string()));
        assert_eq!(pagination.sort_order, SortOrder::Descending);
    }

    #[test]
    fn test_pagination_debug() {
        let pagination = Pagination::default();
        let debug = format!("{:?}", pagination);
        assert!(debug.contains("Pagination"));
        assert!(debug.contains("offset: 0"));
        assert!(debug.contains("limit: 50"));
    }

    #[test]
    fn test_pagination_clone() {
        let pagination = Pagination {
            offset: 5,
            limit: 25,
            sort_by: Some("name".to_string()),
            sort_order: SortOrder::Ascending,
        };
        let cloned = pagination.clone();
        assert_eq!(cloned.offset, pagination.offset);
        assert_eq!(cloned.limit, pagination.limit);
        assert_eq!(cloned.sort_by, pagination.sort_by);
        assert_eq!(cloned.sort_order, pagination.sort_order);
    }

    // Tests for SortOrder
    #[test]
    fn test_sort_order_equality() {
        assert_eq!(SortOrder::Ascending, SortOrder::Ascending);
        assert_eq!(SortOrder::Descending, SortOrder::Descending);
        assert_ne!(SortOrder::Ascending, SortOrder::Descending);
    }

    #[test]
    fn test_sort_order_debug() {
        assert_eq!(format!("{:?}", SortOrder::Ascending), "Ascending");
        assert_eq!(format!("{:?}", SortOrder::Descending), "Descending");
    }

    #[test]
    fn test_sort_order_clone() {
        let order = SortOrder::Ascending;
        let cloned = order.clone();
        assert_eq!(order, cloned);
    }

    // Tests for SessionQueryCriteria
    #[test]
    fn test_session_query_criteria_default() {
        let criteria = SessionQueryCriteria::default();
        assert!(criteria.states.is_none());
        assert!(criteria.created_after.is_none());
        assert!(criteria.created_before.is_none());
        assert!(criteria.client_info_pattern.is_none());
        assert!(criteria.has_active_streams.is_none());
        assert!(criteria.min_stream_count.is_none());
        assert!(criteria.max_stream_count.is_none());
    }

    #[test]
    fn test_session_query_criteria_with_states() {
        let criteria = SessionQueryCriteria {
            states: Some(vec!["active".to_string(), "pending".to_string()]),
            ..Default::default()
        };
        assert!(criteria.states.is_some());
        let states = criteria.states.unwrap();
        assert_eq!(states.len(), 2);
        assert!(states.contains(&"active".to_string()));
    }

    #[test]
    fn test_session_query_criteria_with_time_range() {
        let now = Utc::now();
        let criteria = SessionQueryCriteria {
            created_after: Some(now - chrono::Duration::hours(1)),
            created_before: Some(now),
            ..Default::default()
        };
        assert!(criteria.created_after.is_some());
        assert!(criteria.created_before.is_some());
    }

    #[test]
    fn test_session_query_criteria_with_stream_counts() {
        let criteria = SessionQueryCriteria {
            min_stream_count: Some(1),
            max_stream_count: Some(10),
            has_active_streams: Some(true),
            ..Default::default()
        };
        assert_eq!(criteria.min_stream_count, Some(1));
        assert_eq!(criteria.max_stream_count, Some(10));
        assert_eq!(criteria.has_active_streams, Some(true));
    }

    #[test]
    fn test_session_query_criteria_debug() {
        let criteria = SessionQueryCriteria::default();
        let debug = format!("{:?}", criteria);
        assert!(debug.contains("SessionQueryCriteria"));
    }

    #[test]
    fn test_session_query_criteria_clone() {
        let criteria = SessionQueryCriteria {
            states: Some(vec!["active".to_string()]),
            client_info_pattern: Some("test*".to_string()),
            ..Default::default()
        };
        let cloned = criteria.clone();
        assert_eq!(cloned.states, criteria.states);
        assert_eq!(cloned.client_info_pattern, criteria.client_info_pattern);
    }

    // Tests for StreamFilter
    #[test]
    fn test_stream_filter_default() {
        let filter = StreamFilter::default();
        assert!(filter.statuses.is_none());
        assert!(filter.min_priority.is_none());
        assert!(filter.max_priority.is_none());
        assert!(filter.created_after.is_none());
        assert!(filter.has_frames.is_none());
    }

    #[test]
    fn test_stream_filter_with_statuses() {
        let filter = StreamFilter {
            statuses: Some(vec![StreamStatus::Active, StreamStatus::Paused]),
            ..Default::default()
        };
        let statuses = filter.statuses.unwrap();
        assert_eq!(statuses.len(), 2);
        assert!(statuses.contains(&StreamStatus::Active));
        assert!(statuses.contains(&StreamStatus::Paused));
    }

    #[test]
    fn test_stream_filter_with_priority_range() {
        let filter = StreamFilter {
            min_priority: Some(Priority::LOW),
            max_priority: Some(Priority::CRITICAL),
            ..Default::default()
        };
        assert!(filter.min_priority.is_some());
        assert!(filter.max_priority.is_some());
    }

    #[test]
    fn test_stream_filter_clone() {
        let filter = StreamFilter {
            has_frames: Some(true),
            created_after: Some(Utc::now()),
            ..Default::default()
        };
        let cloned = filter.clone();
        assert_eq!(cloned.has_frames, filter.has_frames);
        assert_eq!(cloned.created_after, filter.created_after);
    }

    // Tests for StreamStatus
    #[test]
    fn test_stream_status_all_variants() {
        let statuses = vec![
            StreamStatus::Created,
            StreamStatus::Active,
            StreamStatus::Paused,
            StreamStatus::Completed,
            StreamStatus::Failed,
            StreamStatus::Cancelled,
        ];
        assert_eq!(statuses.len(), 6);
    }

    #[test]
    fn test_stream_status_equality() {
        assert_eq!(StreamStatus::Created, StreamStatus::Created);
        assert_eq!(StreamStatus::Active, StreamStatus::Active);
        assert_ne!(StreamStatus::Created, StreamStatus::Active);
        assert_ne!(StreamStatus::Completed, StreamStatus::Failed);
    }

    #[test]
    fn test_stream_status_debug() {
        assert_eq!(format!("{:?}", StreamStatus::Created), "Created");
        assert_eq!(format!("{:?}", StreamStatus::Active), "Active");
        assert_eq!(format!("{:?}", StreamStatus::Paused), "Paused");
        assert_eq!(format!("{:?}", StreamStatus::Completed), "Completed");
        assert_eq!(format!("{:?}", StreamStatus::Failed), "Failed");
        assert_eq!(format!("{:?}", StreamStatus::Cancelled), "Cancelled");
    }

    #[test]
    fn test_stream_status_clone() {
        let status = StreamStatus::Active;
        let cloned = status.clone();
        assert_eq!(status, cloned);
    }

    // Tests for StreamMetadata
    #[test]
    fn test_stream_metadata_default() {
        let metadata = StreamMetadata::default();
        assert!(metadata.tags.is_empty());
        assert!(metadata.content_type.is_none());
        assert!(metadata.estimated_size.is_none());
        assert!(metadata.priority_hints.is_empty());
    }

    #[test]
    fn test_stream_metadata_with_tags() {
        let mut tags = HashMap::new();
        tags.insert("env".to_string(), "production".to_string());
        tags.insert("version".to_string(), "1.0".to_string());

        let metadata = StreamMetadata {
            tags,
            content_type: Some("application/json".to_string()),
            estimated_size: Some(1024),
            priority_hints: vec![Priority::HIGH, Priority::MEDIUM],
        };

        assert_eq!(metadata.tags.len(), 2);
        assert_eq!(metadata.tags.get("env"), Some(&"production".to_string()));
        assert_eq!(metadata.content_type, Some("application/json".to_string()));
        assert_eq!(metadata.estimated_size, Some(1024));
        assert_eq!(metadata.priority_hints.len(), 2);
    }

    #[test]
    fn test_stream_metadata_clone() {
        let mut tags = HashMap::new();
        tags.insert("key".to_string(), "value".to_string());
        let metadata = StreamMetadata {
            tags,
            content_type: Some("text/plain".to_string()),
            estimated_size: Some(512),
            priority_hints: vec![Priority::LOW],
        };
        let cloned = metadata.clone();
        assert_eq!(cloned.tags, metadata.tags);
        assert_eq!(cloned.content_type, metadata.content_type);
        assert_eq!(cloned.estimated_size, metadata.estimated_size);
        assert_eq!(cloned.priority_hints, metadata.priority_hints);
    }

    // Tests for CacheStatistics
    #[test]
    fn test_cache_statistics_default() {
        let stats = CacheStatistics::default();
        assert_eq!(stats.hit_rate, 0.0);
        assert_eq!(stats.miss_rate, 0.0);
        assert_eq!(stats.total_keys, 0);
        assert_eq!(stats.memory_usage_bytes, 0);
        assert_eq!(stats.eviction_count, 0);
    }

    #[test]
    fn test_cache_statistics_custom() {
        let stats = CacheStatistics {
            hit_rate: 0.85,
            miss_rate: 0.15,
            total_keys: 1000,
            memory_usage_bytes: 1024 * 1024,
            eviction_count: 50,
        };
        assert_eq!(stats.hit_rate, 0.85);
        assert_eq!(stats.miss_rate, 0.15);
        assert_eq!(stats.total_keys, 1000);
        assert_eq!(stats.memory_usage_bytes, 1024 * 1024);
        assert_eq!(stats.eviction_count, 50);
    }

    #[test]
    fn test_cache_statistics_debug() {
        let stats = CacheStatistics::default();
        let debug = format!("{:?}", stats);
        assert!(debug.contains("CacheStatistics"));
        assert!(debug.contains("hit_rate"));
        assert!(debug.contains("miss_rate"));
    }

    #[test]
    fn test_cache_statistics_clone() {
        let stats = CacheStatistics {
            hit_rate: 0.75,
            miss_rate: 0.25,
            total_keys: 500,
            memory_usage_bytes: 2048,
            eviction_count: 10,
        };
        let cloned = stats.clone();
        assert_eq!(cloned.hit_rate, stats.hit_rate);
        assert_eq!(cloned.miss_rate, stats.miss_rate);
        assert_eq!(cloned.total_keys, stats.total_keys);
        assert_eq!(cloned.memory_usage_bytes, stats.memory_usage_bytes);
        assert_eq!(cloned.eviction_count, stats.eviction_count);
    }

    // Tests for SessionQueryResult
    #[test]
    fn test_session_query_result() {
        let result = SessionQueryResult {
            sessions: vec![],
            total_count: 100,
            has_more: true,
            query_duration_ms: 50,
        };
        assert!(result.sessions.is_empty());
        assert_eq!(result.total_count, 100);
        assert!(result.has_more);
        assert_eq!(result.query_duration_ms, 50);
    }

    #[test]
    fn test_session_query_result_debug() {
        let result = SessionQueryResult {
            sessions: vec![],
            total_count: 0,
            has_more: false,
            query_duration_ms: 0,
        };
        let debug = format!("{:?}", result);
        assert!(debug.contains("SessionQueryResult"));
    }

    #[test]
    fn test_session_query_result_clone() {
        let result = SessionQueryResult {
            sessions: vec![],
            total_count: 25,
            has_more: false,
            query_duration_ms: 10,
        };
        let cloned = result.clone();
        assert_eq!(cloned.total_count, result.total_count);
        assert_eq!(cloned.has_more, result.has_more);
        assert_eq!(cloned.query_duration_ms, result.query_duration_ms);
    }

    // Tests for SessionHealthSnapshot
    #[test]
    fn test_session_health_snapshot() {
        let mut metrics = HashMap::new();
        metrics.insert("cpu_usage".to_string(), 0.5);
        metrics.insert("memory_usage".to_string(), 0.75);

        let session_id = SessionId::new();
        let snapshot = SessionHealthSnapshot {
            session_id,
            is_healthy: true,
            active_streams: 5,
            total_frames: 1000,
            last_activity: Utc::now(),
            error_rate: 0.01,
            metrics,
        };

        assert_eq!(snapshot.session_id, session_id);
        assert!(snapshot.is_healthy);
        assert_eq!(snapshot.active_streams, 5);
        assert_eq!(snapshot.total_frames, 1000);
        assert_eq!(snapshot.error_rate, 0.01);
        assert_eq!(snapshot.metrics.len(), 2);
    }

    #[test]
    fn test_session_health_snapshot_unhealthy() {
        let snapshot = SessionHealthSnapshot {
            session_id: SessionId::new(),
            is_healthy: false,
            active_streams: 0,
            total_frames: 0,
            last_activity: Utc::now(),
            error_rate: 0.5,
            metrics: HashMap::new(),
        };
        assert!(!snapshot.is_healthy);
        assert_eq!(snapshot.error_rate, 0.5);
    }

    #[test]
    fn test_session_health_snapshot_clone() {
        let mut metrics = HashMap::new();
        metrics.insert("test".to_string(), 1.0);
        let snapshot = SessionHealthSnapshot {
            session_id: SessionId::new(),
            is_healthy: true,
            active_streams: 3,
            total_frames: 500,
            last_activity: Utc::now(),
            error_rate: 0.0,
            metrics,
        };
        let cloned = snapshot.clone();
        assert_eq!(cloned.session_id, snapshot.session_id);
        assert_eq!(cloned.is_healthy, snapshot.is_healthy);
        assert_eq!(cloned.active_streams, snapshot.active_streams);
    }

    // Tests for FrameQueryResult
    #[test]
    fn test_frame_query_result() {
        let result = FrameQueryResult {
            frames: vec![],
            total_count: 50,
            has_more: true,
            highest_priority: Some(Priority::CRITICAL),
            lowest_priority: Some(Priority::BACKGROUND),
        };
        assert!(result.frames.is_empty());
        assert_eq!(result.total_count, 50);
        assert!(result.has_more);
        assert_eq!(result.highest_priority, Some(Priority::CRITICAL));
        assert_eq!(result.lowest_priority, Some(Priority::BACKGROUND));
    }

    #[test]
    fn test_frame_query_result_no_priority() {
        let result = FrameQueryResult {
            frames: vec![],
            total_count: 0,
            has_more: false,
            highest_priority: None,
            lowest_priority: None,
        };
        assert!(result.highest_priority.is_none());
        assert!(result.lowest_priority.is_none());
    }

    #[test]
    fn test_frame_query_result_clone() {
        let result = FrameQueryResult {
            frames: vec![],
            total_count: 10,
            has_more: false,
            highest_priority: Some(Priority::HIGH),
            lowest_priority: Some(Priority::LOW),
        };
        let cloned = result.clone();
        assert_eq!(cloned.total_count, result.total_count);
        assert_eq!(cloned.has_more, result.has_more);
        assert_eq!(cloned.highest_priority, result.highest_priority);
        assert_eq!(cloned.lowest_priority, result.lowest_priority);
    }

    // Tests for StreamStatistics
    #[test]
    fn test_stream_statistics() {
        let stats = StreamStatistics {
            total_frames: 100,
            total_bytes: 1024 * 1024,
            priority_distribution: PriorityDistribution::default(),
            avg_frame_size: 10240.0,
            creation_time: Utc::now(),
            completion_time: Some(Utc::now()),
            processing_duration: Some(Duration::from_secs(60)),
        };
        assert_eq!(stats.total_frames, 100);
        assert_eq!(stats.total_bytes, 1024 * 1024);
        assert_eq!(stats.avg_frame_size, 10240.0);
        assert!(stats.completion_time.is_some());
        assert!(stats.processing_duration.is_some());
    }

    #[test]
    fn test_stream_statistics_in_progress() {
        let stats = StreamStatistics {
            total_frames: 50,
            total_bytes: 512 * 1024,
            priority_distribution: PriorityDistribution::default(),
            avg_frame_size: 10485.76,
            creation_time: Utc::now(),
            completion_time: None,
            processing_duration: None,
        };
        assert!(stats.completion_time.is_none());
        assert!(stats.processing_duration.is_none());
    }

    #[test]
    fn test_stream_statistics_clone() {
        let stats = StreamStatistics {
            total_frames: 25,
            total_bytes: 256 * 1024,
            priority_distribution: PriorityDistribution::default(),
            avg_frame_size: 10485.76,
            creation_time: Utc::now(),
            completion_time: None,
            processing_duration: None,
        };
        let cloned = stats.clone();
        assert_eq!(cloned.total_frames, stats.total_frames);
        assert_eq!(cloned.total_bytes, stats.total_bytes);
        assert_eq!(cloned.avg_frame_size, stats.avg_frame_size);
    }

    // Tests for CacheExtensions trait
    #[tokio::test]
    async fn test_cache_extensions_set_and_get_typed() {
        let cache = MockCacheRepository::new();

        #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
        struct TestData {
            name: String,
            value: i32,
        }

        let data = TestData {
            name: "test".to_string(),
            value: 42,
        };

        cache.set_typed("key1", &data, None).await.unwrap();
        let retrieved: Option<TestData> = cache.get_typed("key1").await.unwrap();

        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap(), data);
    }

    #[tokio::test]
    async fn test_cache_extensions_get_typed_not_found() {
        let cache = MockCacheRepository::new();
        let result: Option<String> = cache.get_typed::<String>("nonexistent").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_cache_extensions_set_typed_with_ttl() {
        let cache = MockCacheRepository::new();
        let ttl = Some(Duration::from_secs(60));
        cache.set_typed("key", &"value", ttl).await.unwrap();
        let result: Option<String> = cache.get_typed("key").await.unwrap();
        assert_eq!(result, Some("value".to_string()));
    }

    #[tokio::test]
    async fn test_cache_extensions_get_typed_invalid_json() {
        let cache = MockCacheRepository::new();
        cache
            .set_bytes("key", b"invalid json{{{".to_vec(), None)
            .await
            .unwrap();
        let result: DomainResult<Option<HashMap<String, String>>> = cache.get_typed("key").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        let err_msg = format!("{:?}", err);
        assert!(err_msg.contains("Deserialization failed"));
    }

    #[tokio::test]
    async fn test_cache_extensions_get_bytes_failure() {
        let cache = MockCacheRepository::with_get_failure();
        let result: DomainResult<Option<String>> = cache.get_typed("key").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_cache_extensions_set_bytes_failure() {
        let cache = MockCacheRepository::with_set_failure();
        let result = cache.set_typed("key", &"value", None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_cache_extensions_complex_type() {
        let cache = MockCacheRepository::new();

        #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
        struct ComplexData {
            items: Vec<String>,
            metadata: HashMap<String, i64>,
        }

        let mut metadata = HashMap::new();
        metadata.insert("count".to_string(), 100);
        metadata.insert("size".to_string(), 1024);

        let data = ComplexData {
            items: vec!["a".to_string(), "b".to_string(), "c".to_string()],
            metadata,
        };

        cache.set_typed("complex", &data, None).await.unwrap();
        let retrieved: Option<ComplexData> = cache.get_typed("complex").await.unwrap();

        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.items.len(), 3);
        assert_eq!(retrieved.metadata.get("count"), Some(&100));
    }

    // Tests for MockCacheRepository (to ensure test infrastructure works)
    #[tokio::test]
    async fn test_mock_cache_repository_remove() {
        let cache = MockCacheRepository::new();
        cache
            .set_bytes("key", b"value".to_vec(), None)
            .await
            .unwrap();
        assert!(cache.get_bytes("key").await.unwrap().is_some());
        cache.remove("key").await.unwrap();
        assert!(cache.get_bytes("key").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_mock_cache_repository_clear_prefix() {
        let cache = MockCacheRepository::new();
        cache
            .set_bytes("prefix:key1", b"value1".to_vec(), None)
            .await
            .unwrap();
        cache
            .set_bytes("prefix:key2", b"value2".to_vec(), None)
            .await
            .unwrap();
        cache
            .set_bytes("other:key3", b"value3".to_vec(), None)
            .await
            .unwrap();

        cache.clear_prefix("prefix:").await.unwrap();

        assert!(cache.get_bytes("prefix:key1").await.unwrap().is_none());
        assert!(cache.get_bytes("prefix:key2").await.unwrap().is_none());
        assert!(cache.get_bytes("other:key3").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn test_mock_cache_repository_get_stats() {
        let cache = MockCacheRepository::new();
        cache
            .set_bytes("key1", b"value1".to_vec(), None)
            .await
            .unwrap();
        cache
            .set_bytes("key2", b"value2".to_vec(), None)
            .await
            .unwrap();

        let stats = cache.get_stats().await.unwrap();
        assert_eq!(stats.total_keys, 2);
    }
}
