//! Domain services
//!
//! Stateless domain logic that does not naturally belong to a single entity
//! or value object. Services here are pure and WASM-compatible.

pub mod priority;

pub use priority::{PriorityHeuristicConfig, compute_priority};
