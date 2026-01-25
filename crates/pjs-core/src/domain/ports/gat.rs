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
    value_objects::{SessionId, StreamId},
};
use crate::gat_port;
use std::future::Future;

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
