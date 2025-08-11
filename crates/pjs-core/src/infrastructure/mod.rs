//! Infrastructure layer - External concerns and adapters
//!
//! Implements infrastructure adapters for databases, HTTP servers,
//! message queues, and other external systems.

pub mod adapters;
#[cfg(feature = "http-server")]
pub mod http;
pub mod repositories;
pub mod services;

pub use adapters::*;
#[cfg(feature = "http-server")]
pub use http::*;
pub use services::*;