//! Generic UUID-based Identifier Value Object
//!
//! Type-safe identifier using phantom types for compile-time differentiation.
//! Uses sealed trait pattern to prevent external marker implementations.

use std::fmt;
use std::marker::PhantomData;
use uuid::Uuid;

/// Sealed trait module preventing external implementations
mod private {
    pub trait Sealed {}
}

/// Marker trait for type-safe ID differentiation.
///
/// This trait is sealed - external crates cannot implement it.
/// Only marker types defined in this module are valid.
pub trait IdMarker: private::Sealed + Send + Sync + 'static {}

/// Marker type for session identifiers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionMarker;

/// Marker type for stream identifiers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StreamMarker;

impl private::Sealed for SessionMarker {}
impl private::Sealed for StreamMarker {}

impl IdMarker for SessionMarker {}
impl IdMarker for StreamMarker {}

/// Generic UUID-based identifier with phantom type safety.
///
/// Provides compile-time type differentiation between different ID types
/// (e.g., SessionId vs StreamId) while sharing a single implementation.
///
/// # Type Safety
///
/// The phantom type parameter `T` ensures that different ID types cannot
/// be accidentally mixed:
///
/// ```compile_fail
/// # use pjson_rs_domain::value_objects::{SessionId, StreamId};
/// let session_id: SessionId = SessionId::new();
/// let stream_id: StreamId = session_id;  // Compile error!
/// ```
///
/// # Zero-Cost Abstraction
///
/// `PhantomData<T>` is a zero-sized type, so `Id<T>` has the same memory
/// layout as a plain `Uuid`.
///
/// # Examples
///
/// ```
/// # use pjson_rs_domain::value_objects::{SessionId, StreamId};
/// let session_id = SessionId::new();
/// let stream_id = StreamId::new();
///
/// // Type-safe: cannot compare different ID types
/// // session_id == stream_id  // Would not compile
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Id<T: IdMarker> {
    value: Uuid,
    _marker: PhantomData<T>,
}

impl<T: IdMarker> Id<T> {
    /// Create new random identifier
    #[must_use]
    pub fn new() -> Self {
        Self {
            value: Uuid::new_v4(),
            _marker: PhantomData,
        }
    }

    /// Create identifier from existing UUID
    #[must_use]
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self {
            value: uuid,
            _marker: PhantomData,
        }
    }

    /// Create identifier from string representation
    ///
    /// # Errors
    ///
    /// Returns `uuid::Error` if the string is not a valid UUID.
    pub fn from_string(s: &str) -> Result<Self, uuid::Error> {
        Uuid::parse_str(s).map(Self::from_uuid)
    }

    /// Get underlying UUID value
    #[must_use]
    pub fn as_uuid(&self) -> Uuid {
        self.value
    }

    /// Get string representation
    #[must_use]
    pub fn as_str(&self) -> String {
        self.value.to_string()
    }
}

impl<T: IdMarker> Default for Id<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: IdMarker> fmt::Debug for Id<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple(std::any::type_name::<Self>())
            .field(&self.value)
            .finish()
    }
}

impl<T: IdMarker> fmt::Display for Id<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.value)
    }
}

impl<T: IdMarker> From<Uuid> for Id<T> {
    fn from(uuid: Uuid) -> Self {
        Self::from_uuid(uuid)
    }
}

impl<T: IdMarker> From<Id<T>> for Uuid {
    fn from(id: Id<T>) -> Self {
        id.value
    }
}

/// Type alias for session identifier
pub type SessionId = Id<SessionMarker>;

/// Type alias for stream identifier
pub type StreamId = Id<StreamMarker>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_id_creation() {
        let id1 = SessionId::new();
        let id2 = SessionId::new();

        assert_ne!(id1, id2);
        assert_eq!(id1.as_uuid().get_version_num(), 4);
    }

    #[test]
    fn test_id_from_string() {
        let uuid_str = "550e8400-e29b-41d4-a716-446655440000";
        let id = SessionId::from_string(uuid_str).unwrap();
        assert_eq!(id.as_str(), uuid_str);
    }

    #[test]
    fn test_id_from_invalid_string() {
        let result = SessionId::from_string("invalid-uuid");
        assert!(result.is_err());
    }

    #[test]
    fn test_different_id_types_are_distinct() {
        let session_uuid = Uuid::new_v4();
        let session_id = SessionId::from_uuid(session_uuid);
        let stream_id = StreamId::from_uuid(session_uuid);

        // Same underlying UUID, but different types
        assert_eq!(session_id.as_uuid(), stream_id.as_uuid());

        // Type system prevents: session_id == stream_id (won't compile)
    }

    #[test]
    fn test_id_default() {
        let id = SessionId::default();
        assert_eq!(id.as_uuid().get_version_num(), 4);
    }

    #[test]
    fn test_id_debug_display() {
        let id = SessionId::new();
        let debug_str = format!("{:?}", id);
        assert!(debug_str.contains("Id<"));

        let display_str = format!("{}", id);
        assert!(Uuid::parse_str(&display_str).is_ok());
    }

    #[test]
    fn test_id_from_uuid_conversion() {
        let uuid = Uuid::new_v4();
        let id: SessionId = uuid.into();
        let back: Uuid = id.into();
        assert_eq!(uuid, back);
    }

    #[test]
    fn test_stream_id() {
        let id1 = StreamId::new();
        let id2 = StreamId::new();
        assert_ne!(id1, id2);
    }
}
