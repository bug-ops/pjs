//! Infrastructure layer - External concerns and adapters
//!
//! Implements infrastructure adapters for databases, HTTP servers,
//! message queues, and other external systems.

pub mod adapters;
pub mod http;
pub mod repositories;

pub use adapters::*;
pub use http::*;