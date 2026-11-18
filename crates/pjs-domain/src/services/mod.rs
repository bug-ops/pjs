//! Domain services
//!
//! Stateless domain logic that does not naturally belong to a single entity
//! or value object. Services here are pure and WASM-compatible.

pub mod priority;
pub mod skeleton;

pub use priority::{PriorityHeuristicConfig, ValueShape, compute_priority};
pub use skeleton::skeleton;
