//! Data Transfer Objects (DTOs) for serialization
//!
//! This module contains serializable representations of domain objects.
//! DTOs preserve the Clean Architecture principle by keeping serialization
//! concerns out of the domain layer.

pub mod event_dto;
pub mod id_dto;
pub mod json_data_dto;
pub mod json_path_dto;
pub mod priority_dto;
pub mod schema_dto;

pub use event_dto::{DomainEventDto, EventIdDto, PerformanceMetricsDto, PriorityDistributionDto};
pub use id_dto::{IdDto, SessionIdDto, StreamIdDto};
pub use json_data_dto::JsonDataDto;
pub use json_path_dto::JsonPathDto;
pub use priority_dto::{FromDto, PriorityDto, ToDto};
pub use schema_dto::{
    SchemaDefinitionDto, SchemaMetadataDto, SchemaRegistrationDto, ValidationErrorDto,
    ValidationRequestDto, ValidationResultDto,
};
