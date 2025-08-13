//! Infrastructure adapters implementing domain ports
//!
//! These adapters bridge the gap between domain abstractions and 
//! concrete infrastructure implementations, following the Ports & Adapters pattern.

pub mod event_publisher;
pub mod gat_memory_repository;
pub mod json_adapter;
// pub mod memory_repository; // TODO: finish GAT migration
// pub mod metrics_collector; // TODO: migrate to GAT
// pub mod repository_adapters; // TODO: migrate to GAT
// pub mod tokio_writer; // TODO: migrate to GAT

// Re-export commonly used adapters
pub use event_publisher::*;
pub use gat_memory_repository::*;
pub use json_adapter::*;
// pub use memory_repository::*; // TODO: finish GAT migration
// pub use metrics_collector::*; // TODO: migrate to GAT
// pub use repository_adapters::*; // TODO: migrate to GAT
// pub use tokio_writer::*; // TODO: migrate to GAT