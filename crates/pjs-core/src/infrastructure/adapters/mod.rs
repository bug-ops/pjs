//! Infrastructure adapters implementing domain ports
//!
//! These adapters bridge the gap between domain abstractions and
//! concrete infrastructure implementations, following the Ports & Adapters pattern.

pub mod event_publisher;
pub mod gat_memory_repository;
pub mod generic_store;
pub mod json_adapter;
pub mod limits;
pub mod metrics_collector;

// Re-export commonly used adapters
pub use event_publisher::*;
pub use gat_memory_repository::*;
pub use generic_store::{InMemoryStore, SessionStore, StreamStore};
pub use json_adapter::*;
pub use limits::*;
pub use metrics_collector::*;
