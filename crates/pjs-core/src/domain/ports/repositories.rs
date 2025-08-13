//! Repository ports for data persistence
//!
//! These ports define the domain's requirements for data storage,
//! allowing infrastructure adapters to implement various storage backends.

use crate::domain::{
    DomainResult, 
    entities::{Frame, Stream},
    aggregates::StreamSession,
    value_objects::{SessionId, StreamId, Priority, JsonPath},
    events::DomainEvent,
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
    async fn save_session(&self, session: StreamSession, version: Option<u64>) -> DomainResult<u64>;
    
    /// Remove session and all associated data
    async fn remove_session(&self, session_id: SessionId) -> DomainResult<()>;
    
    /// Find sessions by criteria with pagination
    async fn find_sessions_by_criteria(
        &self, 
        criteria: SessionQueryCriteria,
        pagination: Pagination,
    ) -> DomainResult<SessionQueryResult>;
    
    /// Get session health metrics
    async fn get_session_health(&self, session_id: SessionId) -> DomainResult<SessionHealthSnapshot>;
    
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
    async fn get_stream(&self, stream_id: StreamId, use_cache: bool) -> DomainResult<Option<Stream>>;
    
    /// Delete stream and associated frames
    async fn delete_stream(&self, stream_id: StreamId) -> DomainResult<()>;
    
    /// Find streams by session with filtering
    async fn find_streams_by_session(
        &self,
        session_id: SessionId,
        filter: StreamFilter,
    ) -> DomainResult<Vec<Stream>>;
    
    /// Update stream status
    async fn update_stream_status(&self, stream_id: StreamId, status: StreamStatus) -> DomainResult<()>;
    
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
    async fn get_frame_priority_distribution(&self, stream_id: StreamId) -> DomainResult<PriorityDistribution>;
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
    async fn set_bytes(&self, key: &str, value: Vec<u8>, ttl: Option<std::time::Duration>) -> DomainResult<()>;
    
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
            let value = serde_json::from_slice(&bytes)
                .map_err(|e| crate::domain::DomainError::Logic(format!("Deserialization failed: {e}")))?;
            Ok(Some(value))
        } else {
            Ok(None)
        }
    }
    
    /// Set cached value with serialization
    async fn set_typed<T>(&self, key: &str, value: &T, ttl: Option<std::time::Duration>) -> DomainResult<()>
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
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
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