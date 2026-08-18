//! Priority Data Transfer Object for serialization
//!
//! Handles serialization/deserialization of Priority domain objects
//! while keeping domain layer clean of serialization concerns.

use crate::domain::value_objects::Priority;
use crate::domain::{DomainError, DomainResult};
use serde::{Deserialize, Serialize};

/// Serializable representation of Priority domain object
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PriorityDto {
    value: u8,
}

impl PriorityDto {
    /// Create from raw value with validation
    pub fn new(value: u8) -> DomainResult<Self> {
        // Validate using domain rules
        Priority::new(value)?;
        Ok(Self { value })
    }

    /// Get raw value
    pub fn value(self) -> u8 {
        self.value
    }
}

impl From<Priority> for PriorityDto {
    fn from(priority: Priority) -> Self {
        Self {
            value: priority.value(),
        }
    }
}

impl TryFrom<PriorityDto> for Priority {
    type Error = DomainError;

    fn try_from(dto: PriorityDto) -> Result<Self, Self::Error> {
        Priority::new(dto.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    #[test]
    fn test_priority_dto_serialization() {
        let priority = Priority::CRITICAL;
        let dto = PriorityDto::from(priority);

        // Test JSON serialization
        let json = serde_json::to_string(&dto).unwrap();
        assert_eq!(json, "100");

        // Test JSON deserialization
        let deserialized: PriorityDto = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.value(), 100);

        // Test conversion back to domain
        let domain_priority = Priority::try_from(deserialized).unwrap();
        assert_eq!(domain_priority, Priority::CRITICAL);
    }

    #[test]
    fn test_priority_dto_validation() {
        // Valid priority
        assert!(PriorityDto::new(100).is_ok());

        // Invalid priority (zero)
        assert!(PriorityDto::new(0).is_err());
    }

    #[test]
    fn test_try_from_invalid_priority_dto_fails() {
        // `PriorityDto` is `#[serde(transparent)]` over a bare `u8` and derives
        // `Deserialize` directly, so it can be built from wire data that bypasses
        // `PriorityDto::new`'s validation entirely.
        let invalid_dto: PriorityDto = serde_json::from_str("0").unwrap();

        let result = Priority::try_from(invalid_dto);
        assert!(matches!(result, Err(DomainError::InvalidPriority(_))));
    }

    #[test]
    fn test_conversion_traits() {
        let priority = Priority::HIGH;

        // Test From trait
        let dto: PriorityDto = priority.into();
        assert_eq!(dto.value(), 80);

        // Test TryFrom trait
        let converted = Priority::try_from(dto).unwrap();
        assert_eq!(converted, Priority::HIGH);
    }
}
