//! Ports - Domain interfaces for external dependencies
//!
//! Defines contracts that infrastructure adapters must implement.
//! These are the domain's view of what it needs from the outside world.
//!
//! This module implements the Ports and Adapters pattern (Hexagonal Architecture)
//! by defining abstract interfaces that decouple the domain from infrastructure concerns.

pub mod gat;
pub mod repositories;
pub mod writer;

// GAT traits (canonical interfaces)
pub use gat::*;

// Supporting types only (no async_trait traits)
pub use repositories::{
    CacheExtensions, CacheStatistics, FrameQueryResult, Pagination, PriorityDistribution,
    SessionHealthSnapshot, SessionQueryCriteria, SessionQueryResult, SortOrder, StreamFilter,
    StreamMetadata, StreamStatistics, StreamStatus,
};

pub use writer::{
    BackpressureStrategy, ConnectionMetrics, ConnectionState, WriterConfig, WriterMetrics,
};

// Legacy async_trait implementations removed - use GAT versions instead

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
