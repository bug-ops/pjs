//! Infrastructure adapters implementing domain ports
//!
//! These adapters bridge the gap between domain abstractions and 
//! concrete infrastructure implementations, following the Ports & Adapters pattern.

pub mod event_publisher;
pub mod memory_repository;
pub mod metrics_collector;
pub mod repository_adapters;
pub mod tokio_writer;

// Re-export commonly used adapters
pub use event_publisher::*;
pub use memory_repository::*;
pub use metrics_collector::*;
pub use repository_adapters::*;
pub use tokio_writer::*;