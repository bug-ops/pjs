//! Schema Data Transfer Objects
//!
//! DTOs for transferring schema data across application boundaries.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::domain::value_objects::Schema;

/// Schema registration DTO
///
/// Used when registering a new schema in the system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaRegistrationDto {
    /// Unique schema identifier
    pub id: String,
    /// Schema definition
    pub schema: SchemaDefinitionDto,
    /// Optional schema metadata
    pub metadata: Option<SchemaMetadataDto>,
}

/// Schema metadata DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaMetadataDto {
    /// Schema version
    pub version: String,
    /// Schema description
    pub description: Option<String>,
    /// Schema author
    pub author: Option<String>,
    /// Creation timestamp
    pub created_at: Option<i64>,
}

/// Schema definition DTO
///
/// Simplified JSON-serializable representation of schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SchemaDefinitionDto {
    String {
        #[serde(skip_serializing_if = "Option::is_none")]
        min_length: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        max_length: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pattern: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        enum_values: Option<Vec<String>>,
    },
    Integer {
        #[serde(skip_serializing_if = "Option::is_none")]
        minimum: Option<i64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        maximum: Option<i64>,
    },
    Number {
        #[serde(skip_serializing_if = "Option::is_none")]
        minimum: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        maximum: Option<f64>,
    },
    Boolean,
    Null,
    Array {
        #[serde(skip_serializing_if = "Option::is_none")]
        items: Option<Box<SchemaDefinitionDto>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        min_items: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        max_items: Option<usize>,
        #[serde(default)]
        unique_items: bool,
    },
    Object {
        properties: HashMap<String, SchemaDefinitionDto>,
        #[serde(default)]
        required: Vec<String>,
        #[serde(default = "default_true")]
        additional_properties: bool,
    },
    OneOf {
        schemas: Vec<SchemaDefinitionDto>,
    },
    AllOf {
        schemas: Vec<SchemaDefinitionDto>,
    },
    Any,
}

fn default_true() -> bool {
    true
}

impl From<SchemaDefinitionDto> for Schema {
    fn from(dto: SchemaDefinitionDto) -> Self {
        match dto {
            SchemaDefinitionDto::String {
                min_length,
                max_length,
                pattern,
                enum_values,
            } => Self::String {
                min_length,
                max_length,
                pattern: pattern.map(|p| p.into()),
                allowed_values: enum_values.map(|values| {
                    values
                        .into_iter()
                        .map(|v| v.into())
                        .collect::<smallvec::SmallVec<[_; 8]>>()
                }),
            },
            SchemaDefinitionDto::Integer { minimum, maximum } => Self::Integer { minimum, maximum },
            SchemaDefinitionDto::Number { minimum, maximum } => Self::Number { minimum, maximum },
            SchemaDefinitionDto::Boolean => Self::Boolean,
            SchemaDefinitionDto::Null => Self::Null,
            SchemaDefinitionDto::Array {
                items,
                min_items,
                max_items,
                unique_items,
            } => Self::Array {
                items: items.map(|i| Box::new((*i).into())),
                min_items,
                max_items,
                unique_items,
            },
            SchemaDefinitionDto::Object {
                properties,
                required,
                additional_properties,
            } => Self::Object {
                properties: properties.into_iter().map(|(k, v)| (k, v.into())).collect(),
                required,
                additional_properties,
            },
            SchemaDefinitionDto::OneOf { schemas } => Self::OneOf {
                schemas: schemas
                    .into_iter()
                    .map(|s| Box::new(s.into()))
                    .collect::<smallvec::SmallVec<[_; 4]>>(),
            },
            SchemaDefinitionDto::AllOf { schemas } => Self::AllOf {
                schemas: schemas
                    .into_iter()
                    .map(|s| Box::new(s.into()))
                    .collect::<smallvec::SmallVec<[_; 4]>>(),
            },
            SchemaDefinitionDto::Any => Self::Any,
        }
    }
}

impl From<&Schema> for SchemaDefinitionDto {
    fn from(schema: &Schema) -> Self {
        match schema {
            Schema::String {
                min_length,
                max_length,
                pattern,
                allowed_values,
            } => Self::String {
                min_length: *min_length,
                max_length: *max_length,
                pattern: pattern.as_ref().map(|p| p.to_string()),
                enum_values: allowed_values
                    .as_ref()
                    .map(|v| v.iter().map(|s| s.to_string()).collect()),
            },
            Schema::Integer { minimum, maximum } => Self::Integer {
                minimum: *minimum,
                maximum: *maximum,
            },
            Schema::Number { minimum, maximum } => Self::Number {
                minimum: *minimum,
                maximum: *maximum,
            },
            Schema::Boolean => Self::Boolean,
            Schema::Null => Self::Null,
            Schema::Array {
                items,
                min_items,
                max_items,
                unique_items,
            } => Self::Array {
                items: items.as_ref().map(|i| Box::new(i.as_ref().into())),
                min_items: *min_items,
                max_items: *max_items,
                unique_items: *unique_items,
            },
            Schema::Object {
                properties,
                required,
                additional_properties,
            } => Self::Object {
                properties: properties
                    .iter()
                    .map(|(k, v)| (k.clone(), v.into()))
                    .collect(),
                required: required.clone(),
                additional_properties: *additional_properties,
            },
            Schema::OneOf { schemas } => Self::OneOf {
                schemas: schemas.iter().map(|s| s.as_ref().into()).collect(),
            },
            Schema::AllOf { schemas } => Self::AllOf {
                schemas: schemas.iter().map(|s| s.as_ref().into()).collect(),
            },
            Schema::Any => Self::Any,
        }
    }
}

/// Validation request DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationRequestDto {
    /// Schema ID to validate against
    pub schema_id: String,
    /// JSON data to validate (as string)
    pub data: String,
}

/// Validation result DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResultDto {
    /// Whether validation succeeded
    pub valid: bool,
    /// Validation errors (if any)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<ValidationErrorDto>,
}

/// Validation error DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationErrorDto {
    /// JSON path where error occurred
    pub path: String,
    /// Error message
    pub message: String,
    /// Error type
    pub error_type: String,
}

impl From<&crate::domain::value_objects::SchemaValidationError> for ValidationErrorDto {
    fn from(error: &crate::domain::value_objects::SchemaValidationError) -> Self {
        use crate::domain::value_objects::SchemaValidationError;

        let (error_type, path, message) = match error {
            SchemaValidationError::TypeMismatch {
                path,
                expected,
                actual,
            } => (
                "type_mismatch".to_string(),
                path.clone(),
                format!("Expected {expected}, got {actual}"),
            ),
            SchemaValidationError::MissingRequired { path, field } => (
                "missing_required".to_string(),
                path.clone(),
                format!("Missing required field: {field}"),
            ),
            SchemaValidationError::OutOfRange {
                path,
                value,
                min,
                max,
            } => (
                "out_of_range".to_string(),
                path.clone(),
                format!("Value {value} not in range [{min}, {max}]"),
            ),
            SchemaValidationError::StringLengthConstraint {
                path,
                actual,
                min,
                max,
            } => (
                "string_length".to_string(),
                path.clone(),
                format!("String length {actual} not in range [{min}, {max}]"),
            ),
            SchemaValidationError::PatternMismatch {
                path,
                value,
                pattern,
            } => (
                "pattern_mismatch".to_string(),
                path.clone(),
                format!("Value '{value}' does not match pattern '{pattern}'"),
            ),
            SchemaValidationError::ArraySizeConstraint {
                path,
                actual,
                min,
                max,
            } => (
                "array_size".to_string(),
                path.clone(),
                format!("Array size {actual} not in range [{min}, {max}]"),
            ),
            SchemaValidationError::DuplicateItems { path } => (
                "duplicate_items".to_string(),
                path.clone(),
                "Array contains duplicate items".to_string(),
            ),
            SchemaValidationError::InvalidEnumValue { path, value } => (
                "invalid_enum".to_string(),
                path.clone(),
                format!("Value '{value}' not in allowed values"),
            ),
            SchemaValidationError::AdditionalPropertyNotAllowed { path, property } => (
                "additional_property".to_string(),
                path.clone(),
                format!("Additional property '{property}' not allowed"),
            ),
            SchemaValidationError::NoMatchingOneOf { path } => (
                "no_matching_one_of".to_string(),
                path.clone(),
                "No matching schema in OneOf".to_string(),
            ),
            SchemaValidationError::AllOfFailure { path, failures } => (
                "all_of_failure".to_string(),
                path.clone(),
                format!("AllOf validation failed for schemas: {failures}"),
            ),
        };

        Self {
            path,
            message,
            error_type,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_dto_serialization() {
        let dto = SchemaDefinitionDto::String {
            min_length: Some(1),
            max_length: Some(100),
            pattern: None,
            enum_values: None,
        };

        let json = serde_json::to_string(&dto).unwrap();
        let deserialized: SchemaDefinitionDto = serde_json::from_str(&json).unwrap();

        assert!(matches!(deserialized, SchemaDefinitionDto::String { .. }));
    }

    #[test]
    fn test_schema_dto_conversion() {
        let dto = SchemaDefinitionDto::Integer {
            minimum: Some(0),
            maximum: Some(100),
        };

        let schema: Schema = dto.into();
        assert!(matches!(schema, Schema::Integer { .. }));
    }
}
