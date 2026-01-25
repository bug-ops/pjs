//! Generic ID Data Transfer Object for serialization
//!
//! Handles serialization/deserialization of Id<T> domain objects
//! while keeping domain layer clean of serialization concerns.

use crate::application::dto::priority_dto::{FromDto, ToDto};
use crate::domain::{
    DomainError,
    value_objects::{Id, IdMarker, SessionMarker, StreamMarker},
};
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;
use uuid::Uuid;

/// Generic serializable representation of Id<T> domain object.
///
/// The phantom marker is skipped during serialization, resulting in
/// a transparent UUID representation in JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct IdDto<T: IdMarker> {
    uuid: Uuid,
    #[serde(skip)]
    _marker: PhantomData<T>,
}

impl<T: IdMarker> IdDto<T> {
    /// Create from UUID
    #[must_use]
    pub fn new(uuid: Uuid) -> Self {
        Self {
            uuid,
            _marker: PhantomData,
        }
    }

    /// Create from string with validation
    ///
    /// # Errors
    ///
    /// Returns `uuid::Error` if the string is not a valid UUID.
    pub fn from_string(s: &str) -> Result<Self, uuid::Error> {
        let uuid = Uuid::parse_str(s)?;
        Ok(Self::new(uuid))
    }

    /// Get UUID value
    #[must_use]
    pub fn uuid(self) -> Uuid {
        self.uuid
    }

    /// Get string representation
    #[must_use]
    pub fn as_string(self) -> String {
        self.uuid.to_string()
    }
}

impl<T: IdMarker> From<Id<T>> for IdDto<T> {
    fn from(id: Id<T>) -> Self {
        Self::new(id.as_uuid())
    }
}

impl<T: IdMarker> From<IdDto<T>> for Id<T> {
    fn from(dto: IdDto<T>) -> Self {
        Id::from_uuid(dto.uuid)
    }
}

impl<T: IdMarker> ToDto<IdDto<T>> for Id<T> {
    fn to_dto(self) -> IdDto<T> {
        IdDto::from(self)
    }
}

impl<T: IdMarker> FromDto<IdDto<T>> for Id<T> {
    type Error = DomainError;

    fn from_dto(dto: IdDto<T>) -> Result<Self, Self::Error> {
        Ok(Id::from(dto))
    }
}

impl<T: IdMarker> std::fmt::Display for IdDto<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.uuid)
    }
}

/// Type alias for session ID DTO
pub type SessionIdDto = IdDto<SessionMarker>;

/// Type alias for stream ID DTO
pub type StreamIdDto = IdDto<StreamMarker>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::value_objects::{SessionId, StreamId};

    #[test]
    fn test_session_id_dto_serialization() {
        let session_id = SessionId::new();
        let dto = SessionIdDto::from(session_id);

        let json = serde_json::to_string(&dto).unwrap();
        let deserialized: SessionIdDto = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.uuid(), dto.uuid());

        let domain_session_id = Id::from_dto(deserialized).unwrap();
        assert_eq!(domain_session_id.as_uuid(), session_id.as_uuid());
    }

    #[test]
    fn test_stream_id_dto_serialization() {
        let stream_id = StreamId::new();
        let dto = StreamIdDto::from(stream_id);

        let json = serde_json::to_string(&dto).unwrap();
        let deserialized: StreamIdDto = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.uuid(), dto.uuid());
    }

    #[test]
    fn test_id_dto_from_string() {
        let uuid_str = "550e8400-e29b-41d4-a716-446655440000";
        let dto = SessionIdDto::from_string(uuid_str).unwrap();
        assert_eq!(dto.as_string(), uuid_str);

        assert!(SessionIdDto::from_string("invalid-uuid").is_err());
    }

    #[test]
    fn test_conversion_traits() {
        let session_id = SessionId::new();

        let dto = session_id.to_dto();
        assert_eq!(dto.uuid(), session_id.as_uuid());

        let converted = SessionId::from_dto(dto).unwrap();
        assert_eq!(converted.as_uuid(), session_id.as_uuid());
    }

    #[test]
    fn test_display() {
        let dto = SessionIdDto::from_string("550e8400-e29b-41d4-a716-446655440000").unwrap();
        assert_eq!(format!("{}", dto), "550e8400-e29b-41d4-a716-446655440000");
    }
}
