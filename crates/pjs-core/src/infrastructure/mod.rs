//! Infrastructure layer - External concerns and adapters
//!
//! Implements infrastructure adapters for databases, HTTP servers,
//! message queues, WebSocket transport, and other external systems.

pub mod adapters;
#[cfg(feature = "http-server")]
pub mod http;
pub mod integration;
pub mod repositories;
pub mod schema_repository;
#[cfg(feature = "http-server")]
pub mod websocket;

pub use adapters::*;
#[cfg(feature = "http-server")]
pub use http::*;
pub use integration::*;
pub use schema_repository::SchemaRepository;
#[cfg(feature = "http-server")]
pub use websocket::*;
