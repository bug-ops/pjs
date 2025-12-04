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
                pattern,
                allowed_values: enum_values
                    .map(|values| values.into_iter().collect::<smallvec::SmallVec<[_; 8]>>()),
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
    use crate::domain::value_objects::{Schema, SchemaValidationError};

    // ===========================================
    // SchemaDefinitionDto Serialization Tests
    // ===========================================

    #[test]
    fn test_schema_dto_string_serialization() {
        let dto = SchemaDefinitionDto::String {
            min_length: Some(1),
            max_length: Some(100),
            pattern: Some("^[a-z]+$".to_string()),
            enum_values: Some(vec!["hello".to_string(), "world".to_string()]),
        };

        let json = serde_json::to_string(&dto).unwrap();
        let deserialized: SchemaDefinitionDto = serde_json::from_str(&json).unwrap();

        assert!(matches!(
            deserialized,
            SchemaDefinitionDto::String {
                min_length: Some(1),
                max_length: Some(100),
                pattern: Some(_),
                enum_values: Some(_)
            }
        ));
    }

    #[test]
    fn test_schema_dto_string_minimal_serialization() {
        let dto = SchemaDefinitionDto::String {
            min_length: None,
            max_length: None,
            pattern: None,
            enum_values: None,
        };

        let json = serde_json::to_string(&dto).unwrap();
        let deserialized: SchemaDefinitionDto = serde_json::from_str(&json).unwrap();

        assert!(matches!(
            deserialized,
            SchemaDefinitionDto::String {
                min_length: None,
                max_length: None,
                pattern: None,
                enum_values: None
            }
        ));
    }

    #[test]
    fn test_schema_dto_integer_serialization() {
        let dto = SchemaDefinitionDto::Integer {
            minimum: Some(-100),
            maximum: Some(100),
        };

        let json = serde_json::to_string(&dto).unwrap();
        let deserialized: SchemaDefinitionDto = serde_json::from_str(&json).unwrap();

        assert!(matches!(
            deserialized,
            SchemaDefinitionDto::Integer {
                minimum: Some(-100),
                maximum: Some(100)
            }
        ));
    }

    #[test]
    fn test_schema_dto_number_serialization() {
        let dto = SchemaDefinitionDto::Number {
            minimum: Some(0.5),
            maximum: Some(99.9),
        };

        let json = serde_json::to_string(&dto).unwrap();
        let deserialized: SchemaDefinitionDto = serde_json::from_str(&json).unwrap();

        assert!(matches!(deserialized, SchemaDefinitionDto::Number { .. }));
    }

    #[test]
    fn test_schema_dto_boolean_serialization() {
        let dto = SchemaDefinitionDto::Boolean;

        let json = serde_json::to_string(&dto).unwrap();
        let deserialized: SchemaDefinitionDto = serde_json::from_str(&json).unwrap();

        assert!(matches!(deserialized, SchemaDefinitionDto::Boolean));
    }

    #[test]
    fn test_schema_dto_null_serialization() {
        let dto = SchemaDefinitionDto::Null;

        let json = serde_json::to_string(&dto).unwrap();
        let deserialized: SchemaDefinitionDto = serde_json::from_str(&json).unwrap();

        assert!(matches!(deserialized, SchemaDefinitionDto::Null));
    }

    #[test]
    fn test_schema_dto_array_serialization() {
        let dto = SchemaDefinitionDto::Array {
            items: Some(Box::new(SchemaDefinitionDto::String {
                min_length: None,
                max_length: None,
                pattern: None,
                enum_values: None,
            })),
            min_items: Some(1),
            max_items: Some(10),
            unique_items: true,
        };

        let json = serde_json::to_string(&dto).unwrap();
        let deserialized: SchemaDefinitionDto = serde_json::from_str(&json).unwrap();

        assert!(matches!(
            deserialized,
            SchemaDefinitionDto::Array {
                items: Some(_),
                min_items: Some(1),
                max_items: Some(10),
                unique_items: true
            }
        ));
    }

    #[test]
    fn test_schema_dto_array_minimal_serialization() {
        let dto = SchemaDefinitionDto::Array {
            items: None,
            min_items: None,
            max_items: None,
            unique_items: false,
        };

        let json = serde_json::to_string(&dto).unwrap();
        let deserialized: SchemaDefinitionDto = serde_json::from_str(&json).unwrap();

        assert!(matches!(
            deserialized,
            SchemaDefinitionDto::Array {
                items: None,
                min_items: None,
                max_items: None,
                unique_items: false
            }
        ));
    }

    #[test]
    fn test_schema_dto_object_serialization() {
        let mut properties = HashMap::new();
        properties.insert(
            "name".to_string(),
            SchemaDefinitionDto::String {
                min_length: Some(1),
                max_length: None,
                pattern: None,
                enum_values: None,
            },
        );
        properties.insert(
            "age".to_string(),
            SchemaDefinitionDto::Integer {
                minimum: Some(0),
                maximum: Some(150),
            },
        );

        let dto = SchemaDefinitionDto::Object {
            properties,
            required: vec!["name".to_string()],
            additional_properties: false,
        };

        let json = serde_json::to_string(&dto).unwrap();
        let deserialized: SchemaDefinitionDto = serde_json::from_str(&json).unwrap();

        assert!(matches!(deserialized, SchemaDefinitionDto::Object {
            properties,
            required,
            additional_properties: false
        } if properties.len() == 2 && required.len() == 1));
    }

    #[test]
    fn test_schema_dto_object_allow_additional_properties() {
        let properties = HashMap::new();
        let dto = SchemaDefinitionDto::Object {
            properties,
            required: vec![],
            additional_properties: true,
        };

        let json = serde_json::to_string(&dto).unwrap();
        let deserialized: SchemaDefinitionDto = serde_json::from_str(&json).unwrap();

        assert!(matches!(
            deserialized,
            SchemaDefinitionDto::Object {
                additional_properties: true,
                ..
            }
        ));
    }

    #[test]
    fn test_schema_dto_oneof_serialization() {
        let dto = SchemaDefinitionDto::OneOf {
            schemas: vec![
                SchemaDefinitionDto::String {
                    min_length: None,
                    max_length: None,
                    pattern: None,
                    enum_values: None,
                },
                SchemaDefinitionDto::Integer {
                    minimum: None,
                    maximum: None,
                },
            ],
        };

        let json = serde_json::to_string(&dto).unwrap();
        let deserialized: SchemaDefinitionDto = serde_json::from_str(&json).unwrap();

        assert!(
            matches!(deserialized, SchemaDefinitionDto::OneOf { schemas } if schemas.len() == 2)
        );
    }

    #[test]
    fn test_schema_dto_allof_serialization() {
        let dto = SchemaDefinitionDto::AllOf {
            schemas: vec![
                SchemaDefinitionDto::String {
                    min_length: Some(1),
                    max_length: None,
                    pattern: None,
                    enum_values: None,
                },
                SchemaDefinitionDto::String {
                    min_length: None,
                    max_length: Some(100),
                    pattern: None,
                    enum_values: None,
                },
            ],
        };

        let json = serde_json::to_string(&dto).unwrap();
        let deserialized: SchemaDefinitionDto = serde_json::from_str(&json).unwrap();

        assert!(
            matches!(deserialized, SchemaDefinitionDto::AllOf { schemas } if schemas.len() == 2)
        );
    }

    #[test]
    fn test_schema_dto_any_serialization() {
        let dto = SchemaDefinitionDto::Any;

        let json = serde_json::to_string(&dto).unwrap();
        let deserialized: SchemaDefinitionDto = serde_json::from_str(&json).unwrap();

        assert!(matches!(deserialized, SchemaDefinitionDto::Any));
    }

    // ===========================================
    // DTO to Domain Schema Conversion Tests
    // ===========================================

    #[test]
    fn test_schema_dto_to_domain_string() {
        let dto = SchemaDefinitionDto::String {
            min_length: Some(5),
            max_length: Some(50),
            pattern: Some("[a-z]+".to_string()),
            enum_values: Some(vec!["foo".to_string(), "bar".to_string()]),
        };

        let schema: Schema = dto.into();

        assert!(matches!(
            schema,
            Schema::String {
                min_length: Some(5),
                max_length: Some(50),
                pattern: Some(_),
                allowed_values: Some(_)
            }
        ));
    }

    #[test]
    fn test_schema_dto_to_domain_integer() {
        let dto = SchemaDefinitionDto::Integer {
            minimum: Some(10),
            maximum: Some(20),
        };

        let schema: Schema = dto.into();

        assert!(matches!(
            schema,
            Schema::Integer {
                minimum: Some(10),
                maximum: Some(20)
            }
        ));
    }

    #[test]
    fn test_schema_dto_to_domain_number() {
        let dto = SchemaDefinitionDto::Number {
            minimum: Some(1.5),
            maximum: Some(9.9),
        };

        let schema: Schema = dto.into();

        assert!(matches!(schema, Schema::Number {
            minimum: Some(min),
            maximum: Some(max)
        } if (min - 1.5).abs() < 0.001 && (max - 9.9).abs() < 0.001));
    }

    #[test]
    fn test_schema_dto_to_domain_boolean() {
        let dto = SchemaDefinitionDto::Boolean;
        let schema: Schema = dto.into();

        assert!(matches!(schema, Schema::Boolean));
    }

    #[test]
    fn test_schema_dto_to_domain_null() {
        let dto = SchemaDefinitionDto::Null;
        let schema: Schema = dto.into();

        assert!(matches!(schema, Schema::Null));
    }

    #[test]
    fn test_schema_dto_to_domain_array_with_items() {
        let dto = SchemaDefinitionDto::Array {
            items: Some(Box::new(SchemaDefinitionDto::Integer {
                minimum: None,
                maximum: None,
            })),
            min_items: Some(0),
            max_items: Some(100),
            unique_items: true,
        };

        let schema: Schema = dto.into();

        assert!(matches!(
            schema,
            Schema::Array {
                items: Some(_),
                min_items: Some(0),
                max_items: Some(100),
                unique_items: true
            }
        ));
    }

    #[test]
    fn test_schema_dto_to_domain_array_without_items() {
        let dto = SchemaDefinitionDto::Array {
            items: None,
            min_items: None,
            max_items: None,
            unique_items: false,
        };

        let schema: Schema = dto.into();

        assert!(matches!(
            schema,
            Schema::Array {
                items: None,
                min_items: None,
                max_items: None,
                unique_items: false
            }
        ));
    }

    #[test]
    fn test_schema_dto_to_domain_object() {
        let mut properties = HashMap::new();
        properties.insert(
            "id".to_string(),
            SchemaDefinitionDto::Integer {
                minimum: Some(1),
                maximum: None,
            },
        );

        let dto = SchemaDefinitionDto::Object {
            properties,
            required: vec!["id".to_string()],
            additional_properties: false,
        };

        let schema: Schema = dto.into();

        assert!(matches!(schema, Schema::Object {
            properties,
            required,
            additional_properties: false
        } if properties.len() == 1 && required.len() == 1));
    }

    #[test]
    fn test_schema_dto_to_domain_oneof() {
        let dto = SchemaDefinitionDto::OneOf {
            schemas: vec![
                SchemaDefinitionDto::String {
                    min_length: None,
                    max_length: None,
                    pattern: None,
                    enum_values: None,
                },
                SchemaDefinitionDto::Integer {
                    minimum: None,
                    maximum: None,
                },
            ],
        };

        let schema: Schema = dto.into();

        assert!(matches!(schema, Schema::OneOf { schemas } if schemas.len() == 2));
    }

    #[test]
    fn test_schema_dto_to_domain_allof() {
        let dto = SchemaDefinitionDto::AllOf {
            schemas: vec![
                SchemaDefinitionDto::String {
                    min_length: Some(1),
                    max_length: None,
                    pattern: None,
                    enum_values: None,
                },
                SchemaDefinitionDto::String {
                    min_length: None,
                    max_length: Some(100),
                    pattern: None,
                    enum_values: None,
                },
            ],
        };

        let schema: Schema = dto.into();

        assert!(matches!(schema, Schema::AllOf { schemas } if schemas.len() == 2));
    }

    #[test]
    fn test_schema_dto_to_domain_any() {
        let dto = SchemaDefinitionDto::Any;
        let schema: Schema = dto.into();

        assert!(matches!(schema, Schema::Any));
    }

    // ===========================================
    // Domain Schema to DTO Conversion Tests
    // ===========================================

    #[test]
    fn test_domain_schema_to_dto_string() {
        let schema = Schema::String {
            min_length: Some(10),
            max_length: Some(100),
            pattern: Some("[0-9]+".into()),
            allowed_values: Some(smallvec::smallvec!["123".into(), "456".into()]),
        };

        let dto: SchemaDefinitionDto = (&schema).into();

        assert!(matches!(
            dto,
            SchemaDefinitionDto::String {
                min_length: Some(10),
                max_length: Some(100),
                pattern: Some(_),
                enum_values: Some(_)
            }
        ));
    }

    #[test]
    fn test_domain_schema_to_dto_integer() {
        let schema = Schema::Integer {
            minimum: Some(0),
            maximum: Some(1000),
        };

        let dto: SchemaDefinitionDto = (&schema).into();

        assert!(matches!(
            dto,
            SchemaDefinitionDto::Integer {
                minimum: Some(0),
                maximum: Some(1000)
            }
        ));
    }

    #[test]
    fn test_domain_schema_to_dto_number() {
        let schema = Schema::Number {
            minimum: Some(0.0),
            maximum: Some(99.99),
        };

        let dto: SchemaDefinitionDto = (&schema).into();

        assert!(matches!(dto, SchemaDefinitionDto::Number { .. }));
    }

    #[test]
    fn test_domain_schema_to_dto_boolean() {
        let schema = Schema::Boolean;
        let dto: SchemaDefinitionDto = (&schema).into();

        assert!(matches!(dto, SchemaDefinitionDto::Boolean));
    }

    #[test]
    fn test_domain_schema_to_dto_null() {
        let schema = Schema::Null;
        let dto: SchemaDefinitionDto = (&schema).into();

        assert!(matches!(dto, SchemaDefinitionDto::Null));
    }

    #[test]
    fn test_domain_schema_to_dto_array() {
        let schema = Schema::Array {
            items: Some(Box::new(Schema::String {
                min_length: None,
                max_length: None,
                pattern: None,
                allowed_values: None,
            })),
            min_items: Some(1),
            max_items: Some(50),
            unique_items: true,
        };

        let dto: SchemaDefinitionDto = (&schema).into();

        assert!(matches!(
            dto,
            SchemaDefinitionDto::Array {
                items: Some(_),
                min_items: Some(1),
                max_items: Some(50),
                unique_items: true
            }
        ));
    }

    #[test]
    fn test_domain_schema_to_dto_object() {
        let mut properties = HashMap::new();
        properties.insert(
            "email".to_string(),
            Schema::String {
                min_length: Some(5),
                max_length: Some(200),
                pattern: None,
                allowed_values: None,
            },
        );

        let schema = Schema::Object {
            properties,
            required: vec!["email".to_string()],
            additional_properties: true,
        };

        let dto: SchemaDefinitionDto = (&schema).into();

        assert!(matches!(dto, SchemaDefinitionDto::Object {
            properties,
            required,
            additional_properties: true
        } if properties.len() == 1 && required.len() == 1));
    }

    #[test]
    fn test_domain_schema_to_dto_oneof() {
        let schema = Schema::OneOf {
            schemas: smallvec::smallvec![
                Box::new(Schema::String {
                    min_length: None,
                    max_length: None,
                    pattern: None,
                    allowed_values: None,
                }),
                Box::new(Schema::Integer {
                    minimum: None,
                    maximum: None,
                }),
            ],
        };

        let dto: SchemaDefinitionDto = (&schema).into();

        assert!(matches!(dto, SchemaDefinitionDto::OneOf { schemas } if schemas.len() == 2));
    }

    #[test]
    fn test_domain_schema_to_dto_allof() {
        let schema = Schema::AllOf {
            schemas: smallvec::smallvec![
                Box::new(Schema::String {
                    min_length: Some(1),
                    max_length: None,
                    pattern: None,
                    allowed_values: None,
                }),
                Box::new(Schema::String {
                    min_length: None,
                    max_length: Some(50),
                    pattern: None,
                    allowed_values: None,
                }),
            ],
        };

        let dto: SchemaDefinitionDto = (&schema).into();

        assert!(matches!(dto, SchemaDefinitionDto::AllOf { schemas } if schemas.len() == 2));
    }

    #[test]
    fn test_domain_schema_to_dto_any() {
        let schema = Schema::Any;
        let dto: SchemaDefinitionDto = (&schema).into();

        assert!(matches!(dto, SchemaDefinitionDto::Any));
    }

    // ===========================================
    // SchemaRegistrationDto Tests
    // ===========================================

    #[test]
    fn test_schema_registration_dto_serialization() {
        let dto = SchemaRegistrationDto {
            id: "user-schema".to_string(),
            schema: SchemaDefinitionDto::Object {
                properties: HashMap::new(),
                required: vec![],
                additional_properties: true,
            },
            metadata: Some(SchemaMetadataDto {
                version: "1.0".to_string(),
                description: Some("User schema".to_string()),
                author: Some("John Doe".to_string()),
                created_at: Some(1234567890),
            }),
        };

        let json = serde_json::to_string(&dto).unwrap();
        let deserialized: SchemaRegistrationDto = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.id, "user-schema");
        assert!(matches!(
            deserialized.schema,
            SchemaDefinitionDto::Object { .. }
        ));
        assert!(deserialized.metadata.is_some());
    }

    #[test]
    fn test_schema_registration_dto_without_metadata() {
        let dto = SchemaRegistrationDto {
            id: "simple-schema".to_string(),
            schema: SchemaDefinitionDto::String {
                min_length: None,
                max_length: None,
                pattern: None,
                enum_values: None,
            },
            metadata: None,
        };

        let json = serde_json::to_string(&dto).unwrap();
        let deserialized: SchemaRegistrationDto = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.id, "simple-schema");
        assert!(deserialized.metadata.is_none());
    }

    // ===========================================
    // SchemaMetadataDto Tests
    // ===========================================

    #[test]
    fn test_schema_metadata_dto_full() {
        let dto = SchemaMetadataDto {
            version: "2.5".to_string(),
            description: Some("Complete metadata".to_string()),
            author: Some("Jane Smith".to_string()),
            created_at: Some(9876543210),
        };

        let json = serde_json::to_string(&dto).unwrap();
        let deserialized: SchemaMetadataDto = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.version, "2.5");
        assert_eq!(
            deserialized.description,
            Some("Complete metadata".to_string())
        );
        assert_eq!(deserialized.author, Some("Jane Smith".to_string()));
        assert_eq!(deserialized.created_at, Some(9876543210));
    }

    #[test]
    fn test_schema_metadata_dto_minimal() {
        let dto = SchemaMetadataDto {
            version: "1.0".to_string(),
            description: None,
            author: None,
            created_at: None,
        };

        let json = serde_json::to_string(&dto).unwrap();
        let deserialized: SchemaMetadataDto = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.version, "1.0");
        assert!(deserialized.description.is_none());
        assert!(deserialized.author.is_none());
        assert!(deserialized.created_at.is_none());
    }

    // ===========================================
    // ValidationRequestDto and ValidationResultDto Tests
    // ===========================================

    #[test]
    fn test_validation_request_dto_serialization() {
        let dto = ValidationRequestDto {
            schema_id: "user-schema".to_string(),
            data: r#"{"name": "John", "age": 30}"#.to_string(),
        };

        let json = serde_json::to_string(&dto).unwrap();
        let deserialized: ValidationRequestDto = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.schema_id, "user-schema");
        assert_eq!(deserialized.data, r#"{"name": "John", "age": 30}"#);
    }

    #[test]
    fn test_validation_result_dto_valid() {
        let dto = ValidationResultDto {
            valid: true,
            errors: vec![],
        };

        let json = serde_json::to_string(&dto).unwrap();
        // When errors is empty, it's not serialized due to skip_serializing_if
        // So we add it back for deserialization
        let json_with_errors = if json.contains("errors") {
            json
        } else {
            json.replace("}", r#","errors":[]}"#)
        };
        let deserialized: ValidationResultDto = serde_json::from_str(&json_with_errors).unwrap();

        assert!(deserialized.valid);
        assert!(deserialized.errors.is_empty());
    }

    #[test]
    fn test_validation_result_dto_with_errors() {
        let dto = ValidationResultDto {
            valid: false,
            errors: vec![
                ValidationErrorDto {
                    path: "$.name".to_string(),
                    message: "Too short".to_string(),
                    error_type: "string_length".to_string(),
                },
                ValidationErrorDto {
                    path: "$.age".to_string(),
                    message: "Out of range".to_string(),
                    error_type: "out_of_range".to_string(),
                },
            ],
        };

        let json = serde_json::to_string(&dto).unwrap();
        let deserialized: ValidationResultDto = serde_json::from_str(&json).unwrap();

        assert!(!deserialized.valid);
        assert_eq!(deserialized.errors.len(), 2);
    }

    // ===========================================
    // ValidationErrorDto Conversion Tests
    // ===========================================

    #[test]
    fn test_validation_error_type_mismatch_conversion() {
        let domain_error = SchemaValidationError::TypeMismatch {
            path: "$.field".to_string(),
            expected: "string".to_string(),
            actual: "number".to_string(),
        };

        let dto: ValidationErrorDto = (&domain_error).into();

        assert_eq!(dto.path, "$.field");
        assert_eq!(dto.error_type, "type_mismatch");
        assert!(dto.message.contains("string"));
        assert!(dto.message.contains("number"));
    }

    #[test]
    fn test_validation_error_missing_required_conversion() {
        let domain_error = SchemaValidationError::MissingRequired {
            path: "$.".to_string(),
            field: "email".to_string(),
        };

        let dto: ValidationErrorDto = (&domain_error).into();

        assert_eq!(dto.path, "$.");
        assert_eq!(dto.error_type, "missing_required");
        assert!(dto.message.contains("email"));
    }

    #[test]
    fn test_validation_error_out_of_range_conversion() {
        let domain_error = SchemaValidationError::OutOfRange {
            path: "$.age".to_string(),
            value: "200".to_string(),
            min: "0".to_string(),
            max: "150".to_string(),
        };

        let dto: ValidationErrorDto = (&domain_error).into();

        assert_eq!(dto.path, "$.age");
        assert_eq!(dto.error_type, "out_of_range");
        assert!(dto.message.contains("200"));
    }

    #[test]
    fn test_validation_error_string_length_conversion() {
        let domain_error = SchemaValidationError::StringLengthConstraint {
            path: "$.name".to_string(),
            actual: 150,
            min: 1,
            max: 100,
        };

        let dto: ValidationErrorDto = (&domain_error).into();

        assert_eq!(dto.path, "$.name");
        assert_eq!(dto.error_type, "string_length");
        assert!(dto.message.contains("150"));
    }

    #[test]
    fn test_validation_error_pattern_mismatch_conversion() {
        let domain_error = SchemaValidationError::PatternMismatch {
            path: "$.email".to_string(),
            value: "invalid".to_string(),
            pattern: "[a-z]+@[a-z]+\\.[a-z]+".to_string(),
        };

        let dto: ValidationErrorDto = (&domain_error).into();

        assert_eq!(dto.path, "$.email");
        assert_eq!(dto.error_type, "pattern_mismatch");
        assert!(dto.message.contains("invalid"));
    }

    #[test]
    fn test_validation_error_array_size_conversion() {
        let domain_error = SchemaValidationError::ArraySizeConstraint {
            path: "$.items".to_string(),
            actual: 20,
            min: 1,
            max: 10,
        };

        let dto: ValidationErrorDto = (&domain_error).into();

        assert_eq!(dto.path, "$.items");
        assert_eq!(dto.error_type, "array_size");
        assert!(dto.message.contains("20"));
    }

    #[test]
    fn test_validation_error_duplicate_items_conversion() {
        let domain_error = SchemaValidationError::DuplicateItems {
            path: "$.values".to_string(),
        };

        let dto: ValidationErrorDto = (&domain_error).into();

        assert_eq!(dto.path, "$.values");
        assert_eq!(dto.error_type, "duplicate_items");
        assert!(dto.message.contains("duplicate"));
    }

    #[test]
    fn test_validation_error_invalid_enum_conversion() {
        let domain_error = SchemaValidationError::InvalidEnumValue {
            path: "$.status".to_string(),
            value: "pending".to_string(),
        };

        let dto: ValidationErrorDto = (&domain_error).into();

        assert_eq!(dto.path, "$.status");
        assert_eq!(dto.error_type, "invalid_enum");
        assert!(dto.message.contains("pending"));
    }

    #[test]
    fn test_validation_error_additional_property_conversion() {
        let domain_error = SchemaValidationError::AdditionalPropertyNotAllowed {
            path: "$.".to_string(),
            property: "extra_field".to_string(),
        };

        let dto: ValidationErrorDto = (&domain_error).into();

        assert_eq!(dto.path, "$.");
        assert_eq!(dto.error_type, "additional_property");
        assert!(dto.message.contains("extra_field"));
    }

    #[test]
    fn test_validation_error_no_matching_oneof_conversion() {
        let domain_error = SchemaValidationError::NoMatchingOneOf {
            path: "$.value".to_string(),
        };

        let dto: ValidationErrorDto = (&domain_error).into();

        assert_eq!(dto.path, "$.value");
        assert_eq!(dto.error_type, "no_matching_one_of");
    }

    #[test]
    fn test_validation_error_allof_failure_conversion() {
        let domain_error = SchemaValidationError::AllOfFailure {
            path: "$.item".to_string(),
            failures: "schema1, schema2".to_string(),
        };

        let dto: ValidationErrorDto = (&domain_error).into();

        assert_eq!(dto.path, "$.item");
        assert_eq!(dto.error_type, "all_of_failure");
        assert!(dto.message.contains("schema1"));
    }

    // ===========================================
    // Complex Nested Schema Tests
    // ===========================================

    #[test]
    fn test_nested_object_with_array_conversion() {
        let mut inner_properties = HashMap::new();
        inner_properties.insert(
            "id".to_string(),
            SchemaDefinitionDto::Integer {
                minimum: Some(1),
                maximum: None,
            },
        );

        let dto = SchemaDefinitionDto::Object {
            properties: {
                let mut props = HashMap::new();
                props.insert(
                    "items".to_string(),
                    SchemaDefinitionDto::Array {
                        items: Some(Box::new(SchemaDefinitionDto::Object {
                            properties: inner_properties,
                            required: vec!["id".to_string()],
                            additional_properties: false,
                        })),
                        min_items: Some(1),
                        max_items: None,
                        unique_items: false,
                    },
                );
                props
            },
            required: vec!["items".to_string()],
            additional_properties: true,
        };

        let schema: Schema = dto.into();
        assert!(matches!(schema, Schema::Object { .. }));
    }

    #[test]
    fn test_nested_object_roundtrip() {
        let mut inner_props = HashMap::new();
        inner_props.insert(
            "name".to_string(),
            Schema::String {
                min_length: Some(1),
                max_length: Some(100),
                pattern: None,
                allowed_values: None,
            },
        );

        let original_schema = Schema::Object {
            properties: {
                let mut props = HashMap::new();
                props.insert(
                    "user".to_string(),
                    Schema::Object {
                        properties: inner_props,
                        required: vec!["name".to_string()],
                        additional_properties: false,
                    },
                );
                props
            },
            required: vec!["user".to_string()],
            additional_properties: true,
        };

        let dto: SchemaDefinitionDto = (&original_schema).into();
        let schema: Schema = dto.into();

        assert!(matches!(schema, Schema::Object { .. }));
    }

    #[test]
    fn test_deeply_nested_array() {
        let innermost_dto = SchemaDefinitionDto::String {
            min_length: None,
            max_length: None,
            pattern: None,
            enum_values: None,
        };

        let level1 = SchemaDefinitionDto::Array {
            items: Some(Box::new(innermost_dto)),
            min_items: None,
            max_items: None,
            unique_items: false,
        };

        let level2 = SchemaDefinitionDto::Array {
            items: Some(Box::new(level1)),
            min_items: None,
            max_items: None,
            unique_items: false,
        };

        let schema: Schema = level2.into();

        assert!(matches!(schema, Schema::Array {
            items: Some(boxed),
            ..
        } if matches!(*boxed, Schema::Array { .. })));
    }
}
