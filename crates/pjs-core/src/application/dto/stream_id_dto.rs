//! StreamId Data Transfer Object for serialization
//!
//! Handles serialization/deserialization of StreamId domain objects
//! while keeping domain layer clean of serialization concerns.

use crate::domain::value_objects::StreamId;
use crate::domain::DomainError;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Serializable representation of StreamId domain object
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StreamIdDto {
    uuid: Uuid,
}

impl StreamIdDto {
    /// Create from UUID
    pub fn new(uuid: Uuid) -> Self {
        Self { uuid }
    }
    
    /// Create from string with validation
    pub fn from_string(s: &str) -> Result<Self, uuid::Error> {
        let uuid = Uuid::parse_str(s)?;
        Ok(Self { uuid })
    }
    
    /// Get UUID value
    pub fn uuid(self) -> Uuid {
        self.uuid
    }
    
    /// Get string representation
    pub fn as_string(self) -> String {
        self.uuid.to_string()
    }
}

impl From<StreamId> for StreamIdDto {
    fn from(stream_id: StreamId) -> Self {
        Self { 
            uuid: stream_id.as_uuid() 
        }
    }
}

impl From<StreamIdDto> for StreamId {
    fn from(dto: StreamIdDto) -> Self {
        StreamId::from_uuid(dto.uuid)
    }
}

/// Utility trait implementation for StreamId
impl super::ToDto<StreamIdDto> for StreamId {
    fn to_dto(self) -> StreamIdDto {
        StreamIdDto::from(self)
    }
}

/// Utility trait implementation for StreamId
impl super::FromDto<StreamIdDto> for StreamId {
    type Error = DomainError;
    
    fn from_dto(dto: StreamIdDto) -> Result<Self, Self::Error> {
        Ok(StreamId::from(dto))
    }
}

impl std::fmt::Display for StreamIdDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.uuid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    #[test]
    fn test_stream_id_dto_serialization() {
        let stream_id = StreamId::new();
        let dto = StreamIdDto::from(stream_id);
        
        // Test JSON serialization
        let json = serde_json::to_string(&dto).unwrap();
        
        // Test JSON deserialization
        let deserialized: StreamIdDto = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.uuid(), dto.uuid());
        
        // Test conversion back to domain
        let domain_stream_id = StreamId::from_dto(deserialized).unwrap();
        assert_eq!(domain_stream_id.as_uuid(), stream_id.as_uuid());
    }
    
    #[test]
    fn test_stream_id_dto_from_string() {
        let uuid_str = "550e8400-e29b-41d4-a716-446655440000";
        let dto = StreamIdDto::from_string(uuid_str).unwrap();
        assert_eq!(dto.as_string(), uuid_str);
        
        // Test invalid UUID
        assert!(StreamIdDto::from_string("invalid-uuid").is_err());
    }
    
    #[test]
    fn test_conversion_traits() {
        let stream_id = StreamId::new();
        
        // Test ToDto trait
        let dto = stream_id.to_dto();
        assert_eq!(dto.uuid(), stream_id.as_uuid());
        
        // Test FromDto trait
        let converted = StreamId::from_dto(dto).unwrap();
        assert_eq!(converted.as_uuid(), stream_id.as_uuid());
    }
}