//! GAT-based domain ports with zero-cost async abstractions
//!
//! This module provides Generic Associated Type (GAT) versions of domain ports
//! that eliminate the runtime overhead of async_trait while maintaining the same
//! API contract. These implementations use true zero-cost futures.
//!
//! # Macro-Based Declarations
//!
//! Traits are declared using the `gat_port!` macro which converts ergonomic
//! async fn syntax into proper GAT trait definitions with associated future types.

use crate::domain::{
    DomainResult,
    aggregates::StreamSession,
    entities::{Frame, Stream},
    events::DomainEvent,
    value_objects::{JsonPath, Priority, SessionId, StreamId},
};
use crate::gat_port;
use chrono::{DateTime, Utc};
use std::future::Future;
use std::time::Duration;

// Re-export supporting types for convenience
pub use super::repositories::{
    CacheExtensions, CacheStatistics, FrameQueryResult, Pagination, PriorityDistribution,
    SessionHealthSnapshot, SessionQueryCriteria, SessionQueryResult, SortOrder, StreamFilter,
    StreamMetadata, StreamStatistics, StreamStatus,
};
pub use super::writer::{
    BackpressureStrategy, ConnectionMetrics, ConnectionState, WriterConfig, WriterMetrics,
};

// ============================================================================
// Frame Source/Sink Ports
// ============================================================================

gat_port! {
    /// Zero-cost frame source with GAT futures
    ///
    /// Provides streaming access to frames with proper async iteration support.
    pub trait FrameSourceGat {
        /// Receive the next frame from the source
        async fn next_frame(&mut self) -> Option<Frame>;

        /// Check if source has more frames available
        async fn has_frames(&self) -> bool;

        /// Close the frame source
        async fn close(&mut self) -> ();
    }
}

gat_port! {
    /// Zero-cost frame sink with GAT futures
    ///
    /// Provides efficient frame transmission with batching support.
    pub trait FrameSinkGat {
        /// Send a frame to the destination
        async fn send_frame(&mut self, frame: Frame) -> ();

        /// Send multiple frames efficiently
        async fn send_frames(&mut self, frames: Vec<Frame>) -> ();

        /// Flush any buffered frames
        async fn flush(&mut self) -> ();

        /// Close the frame sink
        async fn close(&mut self) -> ();
    }
}

// ============================================================================
// Repository Ports
// ============================================================================

gat_port! {
    /// Zero-cost stream repository with GAT futures
    ///
    /// Manages session persistence with async operations.
    pub trait StreamRepositoryGat {
        /// Find session by ID
        async fn find_session(&self, session_id: SessionId) -> Option<StreamSession>;

        /// Save session (insert or update)
        async fn save_session(&self, session: StreamSession) -> ();

        /// Remove session
        async fn remove_session(&self, session_id: SessionId) -> ();

        /// Find all active sessions
        async fn find_active_sessions(&self) -> Vec<StreamSession>;

        /// Find sessions by criteria
        async fn find_sessions_by_criteria(
            &self,
            criteria: SessionQueryCriteria,
            pagination: Pagination
        ) -> SessionQueryResult;

        /// Get session health snapshot
        async fn get_session_health(&self, session_id: SessionId) -> SessionHealthSnapshot;

        /// Check if session exists
        async fn session_exists(&self, session_id: SessionId) -> bool;
    }
}

gat_port! {
    /// Zero-cost stream store with GAT futures
    ///
    /// Provides stream-level storage operations.
    pub trait StreamStoreGat {
        /// Store a stream
        async fn store_stream(&self, stream: Stream) -> ();

        /// Retrieve stream by ID
        async fn get_stream(&self, stream_id: StreamId) -> Option<Stream>;

        /// Delete stream
        async fn delete_stream(&self, stream_id: StreamId) -> ();

        /// List streams for session
        async fn list_streams_for_session(&self, session_id: SessionId) -> Vec<Stream>;

        /// Find streams by session with filter
        async fn find_streams_by_session(&self, session_id: SessionId, filter: StreamFilter) -> Vec<Stream>;

        /// Update stream status
        async fn update_stream_status(&self, stream_id: StreamId, status: StreamStatus) -> ();

        /// Get stream statistics
        async fn get_stream_statistics(&self, stream_id: StreamId) -> StreamStatistics;
    }
}

// ============================================================================
// Event Publisher Port
// ============================================================================

gat_port! {
    /// Zero-cost event publisher with GAT futures
    ///
    /// Publishes domain events for system integration.
    pub trait EventPublisherGat {
        /// Publish a single domain event
        async fn publish(&self, event: DomainEvent) -> ();

        /// Publish multiple domain events
        async fn publish_batch(&self, events: Vec<DomainEvent>) -> ();
    }
}

// ============================================================================
// Metrics Ports
// ============================================================================

/// Zero-cost metrics collector with GAT futures
///
/// Collects general metrics (counters, gauges, timings).
/// Methods have explicit lifetime parameters for borrowed string names.
pub trait MetricsCollectorGat: Send + Sync {
    /// Future type for incrementing counter
    type IncrementCounterFuture<'a>: Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    /// Future type for setting gauge
    type SetGaugeFuture<'a>: Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    /// Future type for recording timing
    type RecordTimingFuture<'a>: Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    /// Increment a counter metric
    fn increment_counter<'a>(
        &'a self,
        name: &'a str,
        value: u64,
        tags: std::collections::HashMap<String, String>,
    ) -> Self::IncrementCounterFuture<'a>;

    /// Set a gauge metric value
    fn set_gauge<'a>(
        &'a self,
        name: &'a str,
        value: f64,
        tags: std::collections::HashMap<String, String>,
    ) -> Self::SetGaugeFuture<'a>;

    /// Record timing information
    fn record_timing<'a>(
        &'a self,
        name: &'a str,
        duration: std::time::Duration,
        tags: std::collections::HashMap<String, String>,
    ) -> Self::RecordTimingFuture<'a>;
}

gat_port! {
    /// Zero-cost session/stream metrics collector with GAT futures
    ///
    /// Separate trait for session-level metrics following Interface Segregation Principle.
    pub trait SessionMetricsGat {
        /// Record session creation
        async fn record_session_created(
            &self,
            session_id: SessionId,
            metadata: std::collections::HashMap<String, String>
        ) -> ();

        /// Record session end
        async fn record_session_ended(&self, session_id: SessionId) -> ();

        /// Record stream creation
        async fn record_stream_created(&self, stream_id: StreamId, session_id: SessionId) -> ();

        /// Record stream completion
        async fn record_stream_completed(&self, stream_id: StreamId) -> ();
    }
}

// ============================================================================
// Additional Repository Ports
// ============================================================================

/// Zero-cost session transaction with GAT futures
///
/// Provides transactional operations for session management.
pub trait SessionTransactionGat: Send + Sync {
    type SaveSessionFuture<'a>: Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    type RemoveSessionFuture<'a>: Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    type AddStreamFuture<'a>: Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    type CommitFuture: Future<Output = DomainResult<()>> + Send;

    type RollbackFuture: Future<Output = DomainResult<()>> + Send;

    fn save_session(&self, session: StreamSession) -> Self::SaveSessionFuture<'_>;

    fn remove_session(&self, session_id: SessionId) -> Self::RemoveSessionFuture<'_>;

    fn add_stream(&self, session_id: SessionId, stream: Stream) -> Self::AddStreamFuture<'_>;

    fn commit(self: Box<Self>) -> Self::CommitFuture;

    fn rollback(self: Box<Self>) -> Self::RollbackFuture;
}

gat_port! {
    /// Zero-cost frame repository with GAT futures
    ///
    /// Manages frame persistence with priority indexing.
    pub trait FrameRepositoryGat {
        /// Store a single frame
        async fn store_frame(&self, frame: Frame) -> ();

        /// Store multiple frames efficiently
        async fn store_frames(&self, frames: Vec<Frame>) -> ();

        /// Get frames by stream with priority filtering
        async fn get_frames_by_stream(
            &self,
            stream_id: StreamId,
            priority_filter: Option<Priority>,
            pagination: Pagination
        ) -> FrameQueryResult;

        /// Get frames by JSON path
        async fn get_frames_by_path(&self, stream_id: StreamId, path: JsonPath) -> Vec<Frame>;

        /// Delete old frames
        async fn cleanup_old_frames(&self, older_than: DateTime<Utc>) -> u64;

        /// Get priority distribution
        async fn get_frame_priority_distribution(&self, stream_id: StreamId) -> PriorityDistribution;
    }
}

gat_port! {
    /// Zero-cost event store with GAT futures
    ///
    /// Event sourcing support for domain events.
    pub trait EventStoreGat {
        /// Store a single event with sequence number
        async fn store_event(&self, event: DomainEvent, sequence: u64) -> ();

        /// Store multiple events atomically
        async fn store_events(&self, events: Vec<DomainEvent>) -> ();

        /// Get events for session
        async fn get_events_for_session(
            &self,
            session_id: SessionId,
            from_sequence: Option<u64>,
            limit: Option<usize>
        ) -> Vec<DomainEvent>;

        /// Get events for stream
        async fn get_events_for_stream(
            &self,
            stream_id: StreamId,
            from_sequence: Option<u64>,
            limit: Option<usize>
        ) -> Vec<DomainEvent>;

        /// Get events by type
        async fn get_events_by_type(
            &self,
            event_types: Vec<String>,
            time_range: Option<(DateTime<Utc>, DateTime<Utc>)>
        ) -> Vec<DomainEvent>;

        /// Get latest sequence number
        async fn get_latest_sequence(&self) -> u64;

        /// Replay session events
        async fn replay_session_events(&self, session_id: SessionId) -> Vec<DomainEvent>;
    }
}

/// Zero-cost cache with GAT futures
///
/// Provides caching operations with borrowed string keys for performance.
pub trait CacheGat: Send + Sync {
    type GetBytesFuture<'a>: Future<Output = DomainResult<Option<Vec<u8>>>> + Send + 'a
    where
        Self: 'a;

    type SetBytesFuture<'a>: Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    type RemoveFuture<'a>: Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    type ClearPrefixFuture<'a>: Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    type GetStatsFuture<'a>: Future<Output = DomainResult<CacheStatistics>> + Send + 'a
    where
        Self: 'a;

    /// Get cached bytes
    fn get_bytes<'a>(&'a self, key: &'a str) -> Self::GetBytesFuture<'a>;

    /// Set cached bytes with TTL
    fn set_bytes<'a>(
        &'a self,
        key: &'a str,
        value: Vec<u8>,
        ttl: Option<Duration>,
    ) -> Self::SetBytesFuture<'a>;

    /// Remove cached value
    fn remove<'a>(&'a self, key: &'a str) -> Self::RemoveFuture<'a>;

    /// Clear all values with prefix
    fn clear_prefix<'a>(&'a self, prefix: &'a str) -> Self::ClearPrefixFuture<'a>;

    /// Get cache statistics
    fn get_stats(&self) -> Self::GetStatsFuture<'_>;
}

// ============================================================================
// Writer Ports
// ============================================================================

gat_port! {
    /// Zero-cost frame writer with GAT futures
    ///
    /// Advanced frame writing with priority handling.
    pub trait FrameWriterGat {
        /// Write frame with priority handling
        async fn write_prioritized_frame(&mut self, frame: Frame) -> ();

        /// Write frames in priority order
        async fn write_frames_by_priority(&mut self, frames: Vec<Frame>) -> ();

        /// Set backpressure threshold
        async fn set_backpressure_threshold(&mut self, threshold: usize) -> ();

        /// Get writer metrics
        async fn get_metrics(&self) -> WriterMetrics;
    }
}

/// Zero-cost writer factory with GAT futures
///
/// Factory for creating frame sink and writer instances.
/// Note: Returns associated types instead of Box<dyn Trait> for zero-cost.
pub trait WriterFactoryGat: Send + Sync {
    /// Associated type for stream writer implementation
    type StreamWriter: FrameSinkGat + Send;

    /// Associated type for frame writer implementation
    type FrameWriter: FrameWriterGat + Send;

    type CreateStreamWriterFuture<'a>: Future<Output = DomainResult<Self::StreamWriter>> + Send + 'a
    where
        Self: 'a;

    type CreateFrameWriterFuture<'a>: Future<Output = DomainResult<Self::FrameWriter>> + Send + 'a
    where
        Self: 'a;

    /// Create stream writer (FrameSinkGat implementation)
    fn create_stream_writer<'a>(
        &'a self,
        connection_id: &'a str,
        config: WriterConfig,
    ) -> Self::CreateStreamWriterFuture<'a>;

    /// Create frame writer with advanced features
    fn create_frame_writer<'a>(
        &'a self,
        connection_id: &'a str,
        config: WriterConfig,
    ) -> Self::CreateFrameWriterFuture<'a>;
}

/// Zero-cost connection monitor with GAT futures
///
/// Monitors connection health and state.
pub trait ConnectionMonitorGat: Send + Sync {
    type GetConnectionStateFuture<'a>: Future<Output = DomainResult<ConnectionState>> + Send + 'a
    where
        Self: 'a;

    type IsConnectionHealthyFuture<'a>: Future<Output = DomainResult<bool>> + Send + 'a
    where
        Self: 'a;

    type GetConnectionMetricsFuture<'a>: Future<Output = DomainResult<ConnectionMetrics>> + Send + 'a
    where
        Self: 'a;

    /// Get connection state
    fn get_connection_state<'a>(&'a self, connection_id: &'a str) -> Self::GetConnectionStateFuture<'a>;

    /// Check if connection is healthy
    fn is_connection_healthy<'a>(&'a self, connection_id: &'a str) -> Self::IsConnectionHealthyFuture<'a>;

    /// Get connection metrics
    fn get_connection_metrics<'a>(&'a self, connection_id: &'a str) -> Self::GetConnectionMetricsFuture<'a>;
}

// ============================================================================
// Extension Traits
// ============================================================================

/// Helper trait for implementing common frame sink operations
pub trait FrameSinkGatExt: FrameSinkGat + Sized {
    /// Send multiple frames with default implementation using send_frame
    fn send_frames_default(
        &mut self,
        frames: Vec<Frame>,
    ) -> impl Future<Output = DomainResult<()>> + Send + '_
    where
        Self: 'static,
    {
        async move {
            for frame in frames {
                self.send_frame(frame).await?;
            }
            Ok(())
        }
    }
}

/// Blanket implementation for all FrameSinkGat implementations
impl<T: FrameSinkGat> FrameSinkGatExt for T {}

// Legacy async_trait adapters removed - use native GAT implementations instead

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entities::Frame;
    use crate::domain::value_objects::{JsonData, Priority, StreamId};
    use std::sync::Arc;
    use tokio::sync::Mutex;

    /// Mock GAT-based frame sink for testing
    pub struct MockFrameSinkGat {
        frames: Arc<Mutex<Vec<Frame>>>,
        closed: Arc<Mutex<bool>>,
    }

    impl MockFrameSinkGat {
        pub fn new() -> Self {
            Self {
                frames: Arc::new(Mutex::new(Vec::new())),
                closed: Arc::new(Mutex::new(false)),
            }
        }

        pub async fn get_frames(&self) -> Vec<Frame> {
            self.frames.lock().await.clone()
        }

        pub async fn is_closed(&self) -> bool {
            *self.closed.lock().await
        }
    }

    impl FrameSinkGat for MockFrameSinkGat {
        type SendFrameFuture<'a>
            = impl Future<Output = DomainResult<()>> + Send + 'a
        where
            Self: 'a;

        type SendFramesFuture<'a>
            = impl Future<Output = DomainResult<()>> + Send + 'a
        where
            Self: 'a;

        type FlushFuture<'a>
            = impl Future<Output = DomainResult<()>> + Send + 'a
        where
            Self: 'a;

        type CloseFuture<'a>
            = impl Future<Output = DomainResult<()>> + Send + 'a
        where
            Self: 'a;

        fn send_frame(&mut self, frame: Frame) -> Self::SendFrameFuture<'_> {
            async move {
                self.frames.lock().await.push(frame);
                Ok(())
            }
        }

        fn send_frames(&mut self, frames: Vec<Frame>) -> Self::SendFramesFuture<'_> {
            async move {
                self.frames.lock().await.extend(frames);
                Ok(())
            }
        }

        fn flush(&mut self) -> Self::FlushFuture<'_> {
            async move { Ok(()) }
        }

        fn close(&mut self) -> Self::CloseFuture<'_> {
            async move {
                *self.closed.lock().await = true;
                Ok(())
            }
        }
    }

    #[tokio::test]
    async fn test_gat_frame_sink() {
        let mut sink = MockFrameSinkGat::new();

        let test_frame = Frame::skeleton(
            StreamId::new(),
            1,
            JsonData::String("test data".to_string()),
        );

        // Test sending single frame
        sink.send_frame(test_frame.clone()).await.unwrap();

        let frames = sink.get_frames().await;
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].priority(), Priority::CRITICAL); // skeleton frames are critical

        // Test sending multiple frames
        let more_frames = vec![test_frame.clone(), test_frame.clone()];
        sink.send_frames(more_frames).await.unwrap();

        let frames = sink.get_frames().await;
        assert_eq!(frames.len(), 3);

        // Test flush and close
        sink.flush().await.unwrap();
        sink.close().await.unwrap();

        assert!(sink.is_closed().await);
    }

    #[tokio::test]
    async fn test_gat_extension_trait() {
        let mut sink = MockFrameSinkGat::new();

        let test_frame = Frame::skeleton(
            StreamId::new(),
            1,
            JsonData::String("test extension".to_string()),
        );

        // Test extension method
        let frames = vec![test_frame.clone(), test_frame.clone()];
        sink.send_frames_default(frames).await.unwrap();

        let stored_frames = sink.get_frames().await;
        assert_eq!(stored_frames.len(), 2);
    }
}
