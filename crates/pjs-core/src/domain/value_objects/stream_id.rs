//! Stream ID Value Object

use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Unique identifier for streams within a session
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StreamId(Uuid);

impl StreamId {
    /// Create new random stream ID
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create stream ID from UUID
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Create stream ID from string
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

impl Default for StreamId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for StreamId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<Uuid> for StreamId {
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl From<StreamId> for Uuid {
    fn from(id: StreamId) -> Self {
        id.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_id_creation() {
        let id1 = StreamId::new();
        let id2 = StreamId::new();

        assert_ne!(id1, id2);
    }

    #[test]
    fn test_stream_id_from_string() {
        let uuid_str = "550e8400-e29b-41d4-a716-446655440000";
        let id = StreamId::from_string(uuid_str).unwrap();
        assert_eq!(id.as_str(), uuid_str);
    }
}
