//! Ports - Domain interfaces for external dependencies
//!
//! Defines contracts that infrastructure adapters must implement.
//! These are the domain's view of what it needs from the outside world.

use async_trait::async_trait;
use crate::domain::{DomainResult, SessionId, StreamId};
use crate::domain::entities::{Frame, Stream};
use crate::domain::aggregates::StreamSession;

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
    async fn publish_batch(&self, events: Vec<crate::domain::events::DomainEvent>) -> DomainResult<()>;
}

/// Metrics collection port
#[async_trait]
pub trait MetricsCollector: Send + Sync {
    /// Record a counter metric
    fn counter(&self, name: &str, value: u64, labels: &[(&str, &str)]);
    
    /// Record a gauge metric
    fn gauge(&self, name: &str, value: f64, labels: &[(&str, &str)]);
    
    /// Record a histogram metric
    fn histogram(&self, name: &str, value: f64, labels: &[(&str, &str)]);
    
    /// Start timing a duration
    fn timer_start(&self, name: &str) -> Box<dyn Timer + Send + Sync>;
}

/// Timer for measuring durations
pub trait Timer {
    /// Stop the timer and record the duration
    fn stop(self: Box<Self>);
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