//! Domain Value Objects
//!
//! Immutable objects that represent concepts in the domain
//! with no conceptual identity, only defined by their attributes.

mod backpressure;
mod id;
mod json_data;
mod json_path;
mod priority;
mod schema;

pub use backpressure::{BackpressureSignal, FlowControlCredits};
pub use id::{Id, IdMarker, SessionId, SessionMarker, StreamId, StreamMarker};
pub use json_data::JsonData;
pub use json_path::{JsonPath, PathSegment};
pub use priority::Priority;
pub use schema::{Schema, SchemaId, SchemaType, SchemaValidationError, SchemaValidationResult};
