//! Domain layer - Pure business logic
//!
//! Contains entities, value objects, aggregates, domain services
//! and domain events. No dependencies on infrastructure concerns.

// Re-export domain types from pjs-domain crate (WASM-compatible)
pub use pjs_domain::{
    entities, events, value_objects, DomainError, DomainResult,
};

// pjs-core specific domain modules
pub mod aggregates;
pub mod ports;
pub mod services;

// Re-export all core domain types for convenience
pub use aggregates::StreamSession;
pub use entities::{Frame, Stream};
pub use events::{DomainEvent, SessionState};
pub use ports::{FrameSinkGat, FrameSourceGat, StreamRepositoryGat};
pub use services::PriorityService;
pub use value_objects::{JsonData, JsonPath, Priority, Schema, SchemaId, SessionId, StreamId};
