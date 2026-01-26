//! Writer ports for streaming I/O operations
//!
//! Supporting types for writer operations. GAT trait definitions are in `super::gat`.
//!
//! # Migration Note
//!
//! async_trait-based writer traits have been removed. Use GAT equivalents from `super::gat`:
//! - `FrameSinkGat` - Frame writing (replaces StreamWriter)
//! - `FrameWriterGat` - Priority-based frame writing
//! - `WriterFactoryGat` - Writer factory
//! - `ConnectionMonitorGat` - Connection monitoring

use std::time::Duration;

// ============================================================================
// Supporting Types
// ============================================================================

// async_trait traits removed - use GAT traits from super::gat instead

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
