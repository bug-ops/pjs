//! SessionId Data Transfer Object for serialization
//!
//! Handles serialization/deserialization of SessionId domain objects
//! while keeping domain layer clean of serialization concerns.

use crate::{
    application::dto::priority_dto::{FromDto, ToDto},
    domain::{DomainError, value_objects::SessionId},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Serializable representation of SessionId domain object
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionIdDto {
    uuid: Uuid,
}

impl SessionIdDto {
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

impl From<SessionId> for SessionIdDto {
    fn from(session_id: SessionId) -> Self {
        Self {
            uuid: session_id.as_uuid(),
        }
    }
}

impl From<SessionIdDto> for SessionId {
    fn from(dto: SessionIdDto) -> Self {
        SessionId::from_uuid(dto.uuid)
    }
}

/// Utility trait implementation for SessionId
impl ToDto<SessionIdDto> for SessionId {
    fn to_dto(self) -> SessionIdDto {
        SessionIdDto::from(self)
    }
}

/// Utility trait implementation for SessionId
impl FromDto<SessionIdDto> for SessionId {
    type Error = DomainError;

    fn from_dto(dto: SessionIdDto) -> Result<Self, Self::Error> {
        Ok(SessionId::from(dto))
    }
}

impl std::fmt::Display for SessionIdDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.uuid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    #[test]
    fn test_session_id_dto_serialization() {
        let session_id = SessionId::new();
        let dto = SessionIdDto::from(session_id);

        // Test JSON serialization
        let json = serde_json::to_string(&dto).unwrap();

        // Test JSON deserialization
        let deserialized: SessionIdDto = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.uuid(), dto.uuid());

        // Test conversion back to domain
        let domain_session_id = SessionId::from_dto(deserialized).unwrap();
        assert_eq!(domain_session_id.as_uuid(), session_id.as_uuid());
    }

    #[test]
    fn test_session_id_dto_from_string() {
        let uuid_str = "550e8400-e29b-41d4-a716-446655440000";
        let dto = SessionIdDto::from_string(uuid_str).unwrap();
        assert_eq!(dto.as_string(), uuid_str);

        // Test invalid UUID
        assert!(SessionIdDto::from_string("invalid-uuid").is_err());
    }

    #[test]
    fn test_conversion_traits() {
        let session_id = SessionId::new();

        // Test ToDto trait
        let dto = session_id.to_dto();
        assert_eq!(dto.uuid(), session_id.as_uuid());

        // Test FromDto trait
        let converted = SessionId::from_dto(dto).unwrap();
        assert_eq!(converted.as_uuid(), session_id.as_uuid());
    }
}
