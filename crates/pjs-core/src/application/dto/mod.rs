//! Data Transfer Objects (DTOs) for serialization
//!
//! This module contains serializable representations of domain objects.
//! DTOs preserve the Clean Architecture principle by keeping serialization
//! concerns out of the domain layer.

pub mod json_path_dto;
pub mod priority_dto;
pub mod session_id_dto;
pub mod stream_id_dto;

pub use json_path_dto::JsonPathDto;
pub use priority_dto::{PriorityDto, ToDto, FromDto};
pub use session_id_dto::SessionIdDto;
pub use stream_id_dto::StreamIdDto;