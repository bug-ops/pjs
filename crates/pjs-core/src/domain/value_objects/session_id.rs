//! Session ID Value Object
//!
//! Pure domain object for session identification.
//! Serialization is handled in the application layer via DTOs.
//!
//! TODO: Remove serde derives once domain events are refactored to use DTOs

use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Unique identifier for streaming sessions
///
/// This is a pure domain object. Serialization should be handled
/// in the application layer via DTOs, but serde is temporarily kept
/// for compatibility with domain events.
///
/// TODO: Remove Serialize, Deserialize derives once domain events use DTOs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(Uuid);

impl SessionId {
    /// Create new random session ID
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create session ID from UUID
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Create session ID from string
    pub fn from_string(s: &str) -> Result<Self, uuid::Error> {
        Uuid::parse_str(s).map(Self)
    }

    /// Get underlying UUID
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }

    /// Get string representation
    pub fn as_str(&self) -> String {
        self.0.to_string()
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<Uuid> for SessionId {
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl From<SessionId> for Uuid {
    fn from(id: SessionId) -> Self {
        id.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_id_creation() {
        let id1 = SessionId::new();
        let id2 = SessionId::new();

        assert_ne!(id1, id2);
        assert_eq!(id1.as_uuid().get_version_num(), 4);
    }

    #[test]
    fn test_session_id_from_string() {
        let uuid_str = "550e8400-e29b-41d4-a716-446655440000";
        let id = SessionId::from_string(uuid_str).unwrap();
        assert_eq!(id.as_str(), uuid_str);
    }
}
