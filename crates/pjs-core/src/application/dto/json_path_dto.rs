//! JsonPath Data Transfer Object for serialization
//!
//! Handles serialization/deserialization of JsonPath domain objects
//! while keeping domain layer clean of serialization concerns.

use crate::{
    domain::{value_objects::JsonPath, DomainError, DomainResult},
    application::dto::priority_dto::{ToDto, FromDto},
};
use serde::{Deserialize, Serialize};

/// Serializable representation of JsonPath domain object
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JsonPathDto {
    path: String,
}

impl JsonPathDto {
    /// Create from path string with validation
    pub fn new(path: impl Into<String>) -> DomainResult<Self> {
        let path = path.into();
        // Validate using domain rules
        JsonPath::new(&path)?;
        Ok(Self { path })
    }
    
    /// Create root path DTO
    pub fn root() -> Self {
        Self { 
            path: "$".to_string() 
        }
    }
    
    /// Get path string
    pub fn path(&self) -> &str {
        &self.path
    }
    
    /// Get owned path string
    pub fn into_path(self) -> String {
        self.path
    }
}

impl From<JsonPath> for JsonPathDto {
    fn from(json_path: JsonPath) -> Self {
        Self { 
            path: json_path.as_str().to_string() 
        }
    }
}

impl TryFrom<JsonPathDto> for JsonPath {
    type Error = DomainError;
    
    fn try_from(dto: JsonPathDto) -> Result<Self, Self::Error> {
        JsonPath::new(dto.path)
    }
}

/// Utility trait implementation for JsonPath
impl ToDto<JsonPathDto> for JsonPath {
    fn to_dto(self) -> JsonPathDto {
        JsonPathDto::from(self)
    }
}

/// Utility trait implementation for JsonPath
impl FromDto<JsonPathDto> for JsonPath {
    type Error = DomainError;
    
    fn from_dto(dto: JsonPathDto) -> Result<Self, Self::Error> {
        JsonPath::try_from(dto)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    #[test]
    fn test_json_path_dto_serialization() {
        let json_path = JsonPath::new("$.users[0].name").unwrap();
        let dto = JsonPathDto::from(json_path);
        
        // Test JSON serialization
        let json = serde_json::to_string(&dto).unwrap();
        assert_eq!(json, "\"$.users[0].name\"");
        
        // Test JSON deserialization
        let deserialized: JsonPathDto = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.path(), "$.users[0].name");
        
        // Test conversion back to domain
        let domain_path = JsonPath::from_dto(deserialized).unwrap();
        assert_eq!(domain_path.as_str(), "$.users[0].name");
    }
    
    #[test]
    fn test_json_path_dto_validation() {
        // Valid path
        assert!(JsonPathDto::new("$.valid.path").is_ok());
        
        // Invalid path
        assert!(JsonPathDto::new("invalid.path").is_err());
        assert!(JsonPathDto::new("$.").is_err());
    }
    
    #[test]
    fn test_json_path_dto_root() {
        let root_dto = JsonPathDto::root();
        assert_eq!(root_dto.path(), "$");
        
        let domain_root = JsonPath::from_dto(root_dto).unwrap();
        assert_eq!(domain_root, JsonPath::root());
    }
    
    #[test]
    fn test_conversion_traits() {
        let json_path = JsonPath::new("$.test.path").unwrap();
        
        // Test ToDto trait
        let dto = json_path.to_dto();
        assert_eq!(dto.path(), "$.test.path");
        
        // Test FromDto trait
        let converted = JsonPath::from_dto(dto).unwrap();
        assert_eq!(converted.as_str(), "$.test.path");
    }
    
    #[test]
    fn test_json_path_dto_edge_cases() {
        let paths = vec![
            "$",
            "$.key",
            "$.array[0]",
            "$.nested.deep.path",
            "$.array[123].field"
        ];
        
        for path in paths {
            let original = JsonPath::new(path).unwrap();
            let original_str = original.as_str().to_string(); // Save string before move
            let dto = original.to_dto();
            let serialized = serde_json::to_string(&dto).unwrap();
            let deserialized: JsonPathDto = serde_json::from_str(&serialized).unwrap();
            let restored = JsonPath::from_dto(deserialized).unwrap();
            
            assert_eq!(original_str, restored.as_str());
        }
    }
}