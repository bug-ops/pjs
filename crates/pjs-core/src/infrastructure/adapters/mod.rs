//! Infrastructure adapters implementing domain ports
//!
//! These adapters bridge the gap between domain abstractions and
//! concrete infrastructure implementations, following the Ports & Adapters pattern.

pub mod event_publisher;
pub mod frame_store;
pub mod gat_memory_repository;
pub mod generic_store;
pub mod limits;
pub mod metrics_collector;

// Re-export commonly used adapters
pub use event_publisher::{InMemoryEventPublisher, StoredEvent};
pub use frame_store::InMemoryFrameStore;
pub use gat_memory_repository::{GatInMemoryStreamRepository, GatInMemoryStreamStore};
pub use generic_store::{InMemoryStore, SessionStore, StreamStore};
pub use limits::{
    MAX_HEALTH_METRICS, MAX_PAGINATION_LIMIT, MAX_PAGINATION_OFFSET, MAX_RESULTS_LIMIT,
    MAX_SCAN_LIMIT,
};
pub use metrics_collector::{
    InMemoryMetricsCollector, PerformanceMetrics, SessionMetrics, StreamMetrics, TimestampedMetrics,
};
