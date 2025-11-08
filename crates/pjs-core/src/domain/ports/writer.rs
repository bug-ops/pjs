//! Writer ports for streaming I/O operations
//!
//! These ports abstract away the underlying I/O mechanisms,
//! allowing the domain to work with any transport (HTTP/2, WebSocket, TCP).

use crate::domain::{DomainResult, entities::Frame};
use async_trait::async_trait;
use std::time::Duration;

/// Abstract interface for writing streaming data
///
/// This port represents the domain's contract for how frames should be
/// written to clients. Infrastructure adapters implement this for specific
/// transports (HTTP/2, WebSocket, etc.).
#[async_trait]
pub trait StreamWriter: Send + Sync {
    /// Write a single frame to the stream
    ///
    /// Should handle backpressure and flow control automatically
    async fn write_frame(&mut self, frame: Frame) -> DomainResult<()>;

    /// Write multiple frames efficiently in a batch
    ///
    /// Implementations should optimize for bulk operations
    async fn write_frames(&mut self, frames: Vec<Frame>) -> DomainResult<()>;

    /// Flush any buffered data to ensure delivery
    async fn flush(&mut self) -> DomainResult<()>;

    /// Get the write capacity (0 = no limit, >0 = max frames that can be buffered)
    fn capacity(&self) -> Option<usize>;

    /// Check if the writer is ready to accept more data
    fn is_ready(&self) -> bool;

    /// Close the writer gracefully
    async fn close(&mut self) -> DomainResult<()>;
}

/// Higher-level interface for frame-oriented writing
///
/// This port provides additional functionality for frame management,
/// including prioritization and adaptive behavior.
#[async_trait]
pub trait FrameWriter: Send + Sync {
    /// Write a frame with priority handling
    ///
    /// High-priority frames may be sent immediately while low-priority
    /// frames may be buffered or dropped under backpressure.
    async fn write_prioritized_frame(&mut self, frame: Frame) -> DomainResult<()>;

    /// Write frames in priority order
    ///
    /// Frames are automatically sorted and written in priority order
    async fn write_frames_by_priority(&mut self, frames: Vec<Frame>) -> DomainResult<()>;

    /// Set backpressure threshold
    ///
    /// When buffer exceeds this threshold, low-priority frames may be dropped
    async fn set_backpressure_threshold(&mut self, threshold: usize) -> DomainResult<()>;

    /// Get current write metrics
    async fn get_metrics(&self) -> DomainResult<WriterMetrics>;
}

/// Metrics about writer performance
#[derive(Debug, Clone, PartialEq)]
pub struct WriterMetrics {
    /// Number of frames successfully written
    pub frames_written: u64,

    /// Total bytes written
    pub bytes_written: u64,

    /// Number of frames dropped due to backpressure
    pub frames_dropped: u64,

    /// Current buffer size
    pub buffer_size: usize,

    /// Average write latency
    pub avg_write_latency: Duration,

    /// Number of write errors encountered
    pub error_count: u64,
}

impl Default for WriterMetrics {
    fn default() -> Self {
        Self {
            frames_written: 0,
            bytes_written: 0,
            frames_dropped: 0,
            buffer_size: 0,
            avg_write_latency: Duration::ZERO,
            error_count: 0,
        }
    }
}

/// Factory for creating writers
///
/// This allows the domain to request writers without knowing
/// about the specific transport implementation.
#[async_trait]
pub trait WriterFactory: Send + Sync {
    /// Create a new stream writer for the given connection
    async fn create_stream_writer(
        &self,
        connection_id: &str,
        config: WriterConfig,
    ) -> DomainResult<Box<dyn StreamWriter>>;

    /// Create a new frame writer with advanced features
    async fn create_frame_writer(
        &self,
        connection_id: &str,
        config: WriterConfig,
    ) -> DomainResult<Box<dyn FrameWriter>>;
}

/// Configuration for writer creation
#[derive(Debug, Clone)]
pub struct WriterConfig {
    /// Buffer size for batching frames
    pub buffer_size: usize,

    /// Maximum write timeout
    pub write_timeout: Duration,

    /// Enable compression if transport supports it
    pub enable_compression: bool,

    /// Maximum frame size (bytes)
    pub max_frame_size: usize,

    /// Backpressure handling strategy
    pub backpressure_strategy: BackpressureStrategy,
}

impl Default for WriterConfig {
    fn default() -> Self {
        Self {
            buffer_size: 1024,
            write_timeout: Duration::from_secs(30),
            enable_compression: true,
            max_frame_size: 1024 * 1024, // 1MB
            backpressure_strategy: BackpressureStrategy::DropLowPriority,
        }
    }
}

/// Strategy for handling backpressure situations
#[derive(Debug, Clone, PartialEq)]
pub enum BackpressureStrategy {
    /// Block writes until buffer has space
    Block,

    /// Drop low-priority frames to make space
    DropLowPriority,

    /// Drop oldest frames first (FIFO)
    DropOldest,

    /// Return error immediately when buffer is full
    Error,
}

/// Connection state information
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    /// Connection is active and ready
    Active,

    /// Connection is temporarily unavailable
    Unavailable,

    /// Connection is closing gracefully
    Closing,

    /// Connection is closed
    Closed,

    /// Connection encountered an error
    Error(String),
}

/// Port for monitoring connection health
#[async_trait]
pub trait ConnectionMonitor: Send + Sync {
    /// Get current connection state
    async fn get_connection_state(&self, connection_id: &str) -> DomainResult<ConnectionState>;

    /// Check if connection is healthy
    async fn is_connection_healthy(&self, connection_id: &str) -> DomainResult<bool>;

    /// Get connection metrics
    async fn get_connection_metrics(&self, connection_id: &str) -> DomainResult<ConnectionMetrics>;

    /// Register for connection state change notifications
    async fn subscribe_to_state_changes(
        &self,
        connection_id: &str,
        callback: Box<dyn Fn(ConnectionState) + Send + Sync>,
    ) -> DomainResult<()>;
}

/// Metrics about connection performance
#[derive(Debug, Clone, PartialEq)]
pub struct ConnectionMetrics {
    /// Round-trip time
    pub rtt: Duration,

    /// Available bandwidth (bytes/sec)
    pub bandwidth: u64,

    /// Connection uptime
    pub uptime: Duration,

    /// Number of reconnections
    pub reconnect_count: u32,

    /// Last error (if any)
    pub last_error: Option<String>,
}

impl Default for ConnectionMetrics {
    fn default() -> Self {
        Self {
            rtt: Duration::from_millis(50),
            bandwidth: 1_000_000, // 1MB/s default
            uptime: Duration::ZERO,
            reconnect_count: 0,
            last_error: None,
        }
    }
}
