//! Domain Value Objects
//! 
//! Immutable objects that represent concepts in the domain
//! with no conceptual identity, only defined by their attributes.

mod session_id;
mod stream_id;
mod priority;
mod json_path;

pub use session_id::SessionId;
pub use stream_id::StreamId;
pub use priority::Priority;
pub use json_path::{JsonPath, PathSegment};

use uuid::Uuid;
use std::num::NonZeroU8;