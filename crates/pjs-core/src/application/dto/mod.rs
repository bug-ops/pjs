//! Data Transfer Objects (DTOs) for serialization
//!
//! This module contains serializable representations of domain objects.
//! DTOs preserve the Clean Architecture principle by keeping serialization
//! concerns out of the domain layer.

pub mod id_dto;
pub mod priority_dto;
pub mod schema_dto;

pub use id_dto::{IdDto, SessionIdDto, StreamIdDto};
pub use priority_dto::PriorityDto;
pub use schema_dto::{
    SchemaDefinitionDto, SchemaMetadataDto, SchemaRegistrationDto, ValidationErrorDto,
    ValidationRequestDto, ValidationResultDto,
};
