//! GAT-based domain ports with zero-cost async abstractions
//!
//! This module provides Generic Associated Type (GAT) versions of domain ports
//! that eliminate the runtime overhead of async_trait while maintaining the same
//! API contract. These implementations use true zero-cost futures.

use crate::domain::{
    DomainResult,
    aggregates::StreamSession,
    entities::{Frame, Stream},
    events::DomainEvent,
    value_objects::{SessionId, StreamId},
};
use std::future::Future;

/// Zero-cost frame source with GAT futures
pub trait FrameSourceGat: Send + Sync {
    /// Future type for receiving frames
    type NextFrameFuture<'a>: Future<Output = DomainResult<Option<Frame>>> + Send + 'a
    where
        Self: 'a;

    /// Future type for checking frame availability
    type HasFramesFuture<'a>: Future<Output = DomainResult<bool>> + Send + 'a
    where
        Self: 'a;

    /// Future type for closing the source
    type CloseFuture<'a>: Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    /// Receive the next frame from the source
    fn next_frame(&mut self) -> Self::NextFrameFuture<'_>;

    /// Check if source has more frames available
    fn has_frames(&self) -> Self::HasFramesFuture<'_>;

    /// Close the frame source
    fn close(&mut self) -> Self::CloseFuture<'_>;
}

/// Zero-cost frame sink with GAT futures
pub trait FrameSinkGat: Send + Sync {
    /// Future type for sending a single frame
    type SendFrameFuture<'a>: Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    /// Future type for sending multiple frames
    type SendFramesFuture<'a>: Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    /// Future type for flushing
    type FlushFuture<'a>: Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    /// Future type for closing
    type CloseFuture<'a>: Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    /// Send a frame to the destination
    fn send_frame(&mut self, frame: Frame) -> Self::SendFrameFuture<'_>;

    /// Send multiple frames efficiently
    fn send_frames(&mut self, frames: Vec<Frame>) -> Self::SendFramesFuture<'_>;

    /// Flush any buffered frames
    fn flush(&mut self) -> Self::FlushFuture<'_>;

    /// Close the frame sink
    fn close(&mut self) -> Self::CloseFuture<'_>;
}

/// Zero-cost stream repository with GAT futures
pub trait StreamRepositoryGat: Send + Sync {
    /// Future type for finding sessions
    type FindSessionFuture<'a>: Future<Output = DomainResult<Option<StreamSession>>> + Send + 'a
    where
        Self: 'a;

    /// Future type for saving sessions
    type SaveSessionFuture<'a>: Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    /// Future type for removing sessions
    type RemoveSessionFuture<'a>: Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    /// Future type for finding active sessions
    type FindActiveSessionsFuture<'a>: Future<Output = DomainResult<Vec<StreamSession>>> + Send + 'a
    where
        Self: 'a;

    /// Find session by ID
    fn find_session(&self, session_id: SessionId) -> Self::FindSessionFuture<'_>;

    /// Save session (insert or update)
    fn save_session(&self, session: StreamSession) -> Self::SaveSessionFuture<'_>;

    /// Remove session
    fn remove_session(&self, session_id: SessionId) -> Self::RemoveSessionFuture<'_>;

    /// Find all active sessions
    fn find_active_sessions(&self) -> Self::FindActiveSessionsFuture<'_>;
}

/// Zero-cost stream store with GAT futures
pub trait StreamStoreGat: Send + Sync {
    /// Future type for storing streams
    type StoreStreamFuture<'a>: Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    /// Future type for getting streams
    type GetStreamFuture<'a>: Future<Output = DomainResult<Option<Stream>>> + Send + 'a
    where
        Self: 'a;

    /// Future type for deleting streams
    type DeleteStreamFuture<'a>: Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    /// Future type for listing streams
    type ListStreamsFuture<'a>: Future<Output = DomainResult<Vec<Stream>>> + Send + 'a
    where
        Self: 'a;

    /// Store a stream
    fn store_stream(&self, stream: Stream) -> Self::StoreStreamFuture<'_>;

    /// Retrieve stream by ID
    fn get_stream(&self, stream_id: StreamId) -> Self::GetStreamFuture<'_>;

    /// Delete stream
    fn delete_stream(&self, stream_id: StreamId) -> Self::DeleteStreamFuture<'_>;

    /// List streams for session
    fn list_streams_for_session(&self, session_id: SessionId) -> Self::ListStreamsFuture<'_>;
}

/// Zero-cost event publisher with GAT futures
pub trait EventPublisherGat: Send + Sync {
    /// Future type for publishing single event
    type PublishFuture<'a>: Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    /// Future type for publishing multiple events
    type PublishBatchFuture<'a>: Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    /// Publish a single domain event
    fn publish(&self, event: DomainEvent) -> Self::PublishFuture<'_>;

    /// Publish multiple domain events
    fn publish_batch(&self, events: Vec<DomainEvent>) -> Self::PublishBatchFuture<'_>;
}

/// Zero-cost metrics collector with GAT futures
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

/// Zero-cost session/stream metrics collector with GAT futures
///
/// Separate trait for session-level metrics following Interface Segregation Principle
pub trait SessionMetricsGat: Send + Sync {
    /// Future type for recording session creation
    type RecordSessionCreatedFuture<'a>: Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    /// Future type for recording session end
    type RecordSessionEndedFuture<'a>: Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    /// Future type for recording stream creation
    type RecordStreamCreatedFuture<'a>: Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    /// Future type for recording stream completion
    type RecordStreamCompletedFuture<'a>: Future<Output = DomainResult<()>> + Send + 'a
    where
        Self: 'a;

    /// Record session creation
    fn record_session_created(
        &self,
        session_id: SessionId,
        metadata: std::collections::HashMap<String, String>,
    ) -> Self::RecordSessionCreatedFuture<'_>;

    /// Record session end
    fn record_session_ended(&self, session_id: SessionId) -> Self::RecordSessionEndedFuture<'_>;

    /// Record stream creation
    fn record_stream_created(
        &self,
        stream_id: StreamId,
        session_id: SessionId,
    ) -> Self::RecordStreamCreatedFuture<'_>;

    /// Record stream completion
    fn record_stream_completed(&self, stream_id: StreamId)
    -> Self::RecordStreamCompletedFuture<'_>;
}

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
