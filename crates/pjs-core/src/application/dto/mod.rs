//! Data Transfer Objects (DTOs) for serialization
//!
//! This module contains serializable representations of domain objects.
//! DTOs preserve the Clean Architecture principle by keeping serialization
//! concerns out of the domain layer.

pub mod event_dto;
pub mod json_data_dto;
pub mod json_path_dto;
pub mod priority_dto;
pub mod schema_dto;
pub mod session_id_dto;
pub mod stream_id_dto;

pub use event_dto::{DomainEventDto, EventIdDto, PerformanceMetricsDto, PriorityDistributionDto};
pub use json_data_dto::JsonDataDto;
pub use json_path_dto::JsonPathDto;
pub use priority_dto::{FromDto, PriorityDto, ToDto};
pub use schema_dto::{
    SchemaDefinitionDto, SchemaMetadataDto, SchemaRegistrationDto, ValidationErrorDto,
    ValidationRequestDto, ValidationResultDto,
};
pub use session_id_dto::SessionIdDto;
pub use stream_id_dto::StreamIdDto;
