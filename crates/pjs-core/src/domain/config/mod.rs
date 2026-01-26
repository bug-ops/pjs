//! Domain configuration module
//!
//! Contains domain-level configuration constants that define business rules
//! and validation constraints. These are independent of infrastructure.

pub mod limits;

pub use limits::{ALLOWED_SORT_FIELDS, MAX_PAGINATION_LIMIT, MAX_PAGINATION_OFFSET};
