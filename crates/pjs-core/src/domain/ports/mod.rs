//! Ports - Domain interfaces for external dependencies
//!
//! Defines contracts that infrastructure adapters must implement.
//! These are the domain's view of what it needs from the outside world.

use crate::domain::aggregates::StreamSession;
use crate::domain::entities::{Frame, Stream};
use crate::domain::{DomainResult, SessionId, StreamId};
use async_trait::async_trait;
use std::{collections::HashMap, time::Duration};

/// Source of streaming frames
#[async_trait]
pub trait FrameSource: Send + Sync {
    /// Receive the next frame from the source
    async fn next_frame(&mut self) -> DomainResult<Option<Frame>>;

    /// Check if source has more frames available
    async fn has_frames(&self) -> DomainResult<bool>;

    /// Close the frame source
    async fn close(&mut self) -> DomainResult<()>;
}

/// Destination for streaming frames
#[async_trait]
pub trait FrameSink: Send + Sync {
    /// Send a frame to the destination
    async fn send_frame(&mut self, frame: Frame) -> DomainResult<()>;

    /// Send multiple frames efficiently
    async fn send_frames(&mut self, frames: Vec<Frame>) -> DomainResult<()> {
        for frame in frames {
            self.send_frame(frame).await?;
        }
        Ok(())
    }

    /// Flush any buffered frames
    async fn flush(&mut self) -> DomainResult<()>;

    /// Close the frame sink
    async fn close(&mut self) -> DomainResult<()>;
}

/// Repository for stream sessions
#[async_trait]
pub trait StreamRepository: Send + Sync {
    /// Find session by ID
    async fn find_session(&self, session_id: SessionId) -> DomainResult<Option<StreamSession>>;

    /// Save session (insert or update)
    async fn save_session(&self, session: StreamSession) -> DomainResult<()>;

    /// Remove session
    async fn remove_session(&self, session_id: SessionId) -> DomainResult<()>;

    /// Find all active sessions
    async fn find_active_sessions(&self) -> DomainResult<Vec<StreamSession>>;
}

/// Repository for streams
#[async_trait]
pub trait StreamStore: Send + Sync {
    /// Store a stream
    async fn store_stream(&self, stream: Stream) -> DomainResult<()>;

    /// Retrieve stream by ID
    async fn get_stream(&self, stream_id: StreamId) -> DomainResult<Option<Stream>>;

    /// Delete stream
    async fn delete_stream(&self, stream_id: StreamId) -> DomainResult<()>;

    /// List streams for session
    async fn list_streams_for_session(&self, session_id: SessionId) -> DomainResult<Vec<Stream>>;
}

/// Event publishing port
#[async_trait]
pub trait EventPublisher: Send + Sync {
    /// Publish a single domain event
    async fn publish(&self, event: crate::domain::events::DomainEvent) -> DomainResult<()>;

    /// Publish multiple domain events
    async fn publish_batch(
        &self,
        events: Vec<crate::domain::events::DomainEvent>,
    ) -> DomainResult<()>;
}

/// Metrics collection port
#[async_trait]
pub trait MetricsCollector: Send + Sync {
    /// Increment a counter metric
    async fn increment_counter(&self, name: &str, value: u64, tags: HashMap<String, String>) -> DomainResult<()>;

    /// Set a gauge metric value
    async fn set_gauge(&self, name: &str, value: f64, tags: HashMap<String, String>) -> DomainResult<()>;

    /// Record timing information
    async fn record_timing(&self, name: &str, duration: Duration, tags: HashMap<String, String>) -> DomainResult<()>;

    /// Record session creation event
    async fn record_session_created(&self, session_id: SessionId, metadata: HashMap<String, String>) -> DomainResult<()>;

    /// Record session ending event
    async fn record_session_ended(&self, session_id: SessionId) -> DomainResult<()>;

    /// Record stream creation event
    async fn record_stream_created(&self, stream_id: StreamId, session_id: SessionId) -> DomainResult<()>;

    /// Record stream completion event
    async fn record_stream_completed(&self, stream_id: StreamId) -> DomainResult<()>;
}

/// Time provider port (for testability)
pub trait TimeProvider: Send + Sync {
    /// Get current timestamp
    fn now(&self) -> chrono::DateTime<chrono::Utc>;

    /// Get current unix timestamp in milliseconds
    fn now_millis(&self) -> u64 {
        self.now().timestamp_millis() as u64
    }
}

/// Default implementation using system time
#[derive(Debug, Clone)]
pub struct SystemTimeProvider;

impl TimeProvider for SystemTimeProvider {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::Utc::now()
    }
}
