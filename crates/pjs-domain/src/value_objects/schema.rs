//! Schema value object for JSON validation
//!
//! Represents a JSON schema definition following a subset of JSON Schema specification.
//! This is a domain value object with no identity, defined purely by its attributes.

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use std::collections::HashMap;

use crate::DomainError;

/// Schema identifier for tracking and referencing schemas
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SchemaId(String);

impl SchemaId {
    /// Create a new schema identifier
    ///
    /// # Arguments
    /// * `id` - Unique schema identifier string
    ///
    /// # Returns
    /// New schema ID instance
    ///
    /// # Examples
    /// ```
    /// # use pjson_rs_domain::value_objects::SchemaId;
    /// let schema_id = SchemaId::new("user-profile-v1");
    /// ```
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Get schema ID as string slice
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SchemaId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// JSON Schema representation for validation
///
/// Supports a practical subset of JSON Schema Draft 2020-12 focused on
/// validation needs for streaming JSON data.
///
/// # Design Philosophy
/// - Focused on validation, not full JSON Schema specification
/// - Performance-oriented with pre-compiled validation rules
/// - Zero-copy where possible using Arc for shared data
/// - Type-safe with strongly-typed enum variants
///
/// # Examples
/// ```
/// # use pjson_rs_domain::value_objects::{Schema, SchemaType};
/// let schema = Schema::Object {
///     properties: vec![
///         ("id".to_string(), Schema::Integer { minimum: Some(1), maximum: None }),
///         ("name".to_string(), Schema::String {
///             min_length: Some(1),
///             max_length: Some(100),
///             pattern: None,
///             allowed_values: None,
///         }),
///     ].into_iter().collect(),
///     required: vec!["id".to_string(), "name".to_string()],
///     additional_properties: false,
/// };
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Schema {
    /// String type with optional constraints
    String {
        /// Minimum string length (inclusive)
        min_length: Option<usize>,
        /// Maximum string length (inclusive)
        max_length: Option<usize>,
        /// Pattern to match (regex)
        pattern: Option<String>,
        /// Enumeration of allowed values
        allowed_values: Option<SmallVec<[String; 8]>>,
    },

    /// Integer type with optional range constraints
    Integer {
        /// Minimum value (inclusive)
        minimum: Option<i64>,
        /// Maximum value (inclusive)
        maximum: Option<i64>,
    },

    /// Number type (float/double) with optional range constraints
    Number {
        /// Minimum value (inclusive)
        minimum: Option<f64>,
        /// Maximum value (inclusive)
        maximum: Option<f64>,
    },

    /// Boolean type (no constraints)
    Boolean,

    /// Null type (no constraints)
    Null,

    /// Array type with element schema and size constraints
    Array {
        /// Schema for array elements (None = any type)
        items: Option<Box<Schema>>,
        /// Minimum array length (inclusive)
        min_items: Option<usize>,
        /// Maximum array length (inclusive)
        max_items: Option<usize>,
        /// Whether all items must be unique
        unique_items: bool,
    },

    /// Object type with property schemas
    Object {
        /// Property name to schema mapping
        properties: HashMap<String, Schema>,
        /// List of required property names
        required: Vec<String>,
        /// Whether additional properties are allowed
        additional_properties: bool,
    },

    /// Union type (one of multiple schemas)
    OneOf {
        /// List of possible schemas
        schemas: SmallVec<[Box<Schema>; 4]>,
    },

    /// Intersection type (all of multiple schemas)
    AllOf {
        /// List of schemas that must all match
        schemas: SmallVec<[Box<Schema>; 4]>,
    },

    /// Any type (no validation)
    Any,
}

/// Schema validation result
pub type SchemaValidationResult<T> = Result<T, SchemaValidationError>;

/// Schema validation error with detailed context
///
/// Provides rich error information including the JSON path where validation failed,
/// expected vs actual values, and human-readable error messages.
///
/// # Design
/// - Includes full path context for nested validation failures
/// - Provides actionable error messages for debugging
/// - Zero-allocation for common error cases using `String`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, thiserror::Error)]
pub enum SchemaValidationError {
    /// Type mismatch error
    #[error("Type mismatch at '{path}': expected {expected}, got {actual}")]
    TypeMismatch {
        /// JSON path where error occurred
        path: String,
        /// Expected type
        expected: String,
        /// Actual type
        actual: String,
    },

    /// Missing required field
    #[error("Missing required field at '{path}': {field}")]
    MissingRequired {
        /// JSON path to parent object
        path: String,
        /// Name of missing field
        field: String,
    },

    /// Value out of range
    #[error("Value out of range at '{path}': {value} not in [{min}, {max}]")]
    OutOfRange {
        /// JSON path where error occurred
        path: String,
        /// Actual value
        value: String,
        /// Minimum allowed value
        min: String,
        /// Maximum allowed value
        max: String,
    },

    /// String length constraint violation
    #[error("String length constraint at '{path}': length {actual} not in [{min}, {max}]")]
    StringLengthConstraint {
        /// JSON path where error occurred
        path: String,
        /// Actual string length
        actual: usize,
        /// Minimum allowed length
        min: usize,
        /// Maximum allowed length
        max: usize,
    },

    /// Pattern mismatch
    #[error("Pattern mismatch at '{path}': value '{value}' does not match pattern '{pattern}'")]
    PatternMismatch {
        /// JSON path where error occurred
        path: String,
        /// Actual value
        value: String,
        /// Expected pattern
        pattern: String,
    },

    /// Array size constraint violation
    #[error("Array size constraint at '{path}': size {actual} not in [{min}, {max}]")]
    ArraySizeConstraint {
        /// JSON path where error occurred
        path: String,
        /// Actual array size
        actual: usize,
        /// Minimum allowed size
        min: usize,
        /// Maximum allowed size
        max: usize,
    },

    /// Unique items constraint violation
    #[error("Unique items constraint at '{path}': duplicate items found")]
    DuplicateItems {
        /// JSON path where error occurred
        path: String,
    },

    /// Invalid enum value
    #[error("Invalid enum value at '{path}': '{value}' not in allowed values")]
    InvalidEnumValue {
        /// JSON path where error occurred
        path: String,
        /// Actual value
        value: String,
    },

    /// Additional properties not allowed
    #[error("Additional property not allowed at '{path}': '{property}'")]
    AdditionalPropertyNotAllowed {
        /// JSON path where error occurred
        path: String,
        /// Property name
        property: String,
    },

    /// No matching schema in OneOf
    #[error("No matching schema in OneOf at '{path}'")]
    NoMatchingOneOf {
        /// JSON path where error occurred
        path: String,
    },

    /// Not all schemas match in AllOf
    #[error("Not all schemas match in AllOf at '{path}': {failures}")]
    AllOfFailure {
        /// JSON path where error occurred
        path: String,
        /// List of failing schema indices
        failures: String,
    },
}

impl Schema {
    /// Check if schema allows a specific type
    ///
    /// Used for quick type compatibility checks before full validation.
    ///
    /// # Arguments
    /// * `schema_type` - The type to check compatibility for
    ///
    /// # Returns
    /// `true` if the schema allows the type, `false` otherwise
    pub fn allows_type(&self, schema_type: SchemaType) -> bool {
        match (self, schema_type) {
            (Self::String { .. }, SchemaType::String) => true,
            (Self::Integer { .. }, SchemaType::Integer) => true,
            (Self::Number { .. }, SchemaType::Number) => true,
            (Self::Boolean, SchemaType::Boolean) => true,
            (Self::Null, SchemaType::Null) => true,
            (Self::Array { .. }, SchemaType::Array) => true,
            (Self::Object { .. }, SchemaType::Object) => true,
            (Self::Any, _) => true,
            (Self::OneOf { schemas }, schema_type) => {
                schemas.iter().any(|s| s.allows_type(schema_type))
            }
            (Self::AllOf { schemas }, schema_type) => {
                schemas.iter().all(|s| s.allows_type(schema_type))
            }
            _ => false,
        }
    }

    /// Get estimated validation cost for performance optimization
    ///
    /// Higher cost indicates more expensive validation operations.
    /// Used by validation scheduler to optimize validation order.
    ///
    /// # Returns
    /// Validation cost estimate (0-1000 range)
    pub fn validation_cost(&self) -> usize {
        match self {
            Self::Null | Self::Boolean | Self::Any => 1,
            Self::Integer { .. } | Self::Number { .. } => 5,
            Self::String {
                pattern: Some(_), ..
            } => 50, // Regex is expensive
            Self::String { .. } => 10,
            Self::Array { items, .. } => {
                let item_cost = items.as_ref().map_or(1, |s| s.validation_cost());
                10 + item_cost
            }
            Self::Object { properties, .. } => {
                let prop_cost: usize = properties.values().map(|s| s.validation_cost()).sum();
                20 + prop_cost
            }
            Self::OneOf { schemas } => {
                let max_cost = schemas
                    .iter()
                    .map(|s| s.validation_cost())
                    .max()
                    .unwrap_or(0);
                30 + max_cost * schemas.len()
            }
            Self::AllOf { schemas } => {
                let total_cost: usize = schemas.iter().map(|s| s.validation_cost()).sum();
                20 + total_cost
            }
        }
    }

    /// Create a simple string schema with length constraints
    pub fn string(min_length: Option<usize>, max_length: Option<usize>) -> Self {
        Self::String {
            min_length,
            max_length,
            pattern: None,
            allowed_values: None,
        }
    }

    /// Create a simple integer schema with range constraints
    pub fn integer(minimum: Option<i64>, maximum: Option<i64>) -> Self {
        Self::Integer { minimum, maximum }
    }

    /// Create a simple number schema with range constraints
    pub fn number(minimum: Option<f64>, maximum: Option<f64>) -> Self {
        Self::Number { minimum, maximum }
    }

    /// Create an array schema with item type
    pub fn array(items: Option<Schema>) -> Self {
        Self::Array {
            items: items.map(Box::new),
            min_items: None,
            max_items: None,
            unique_items: false,
        }
    }

    /// Create an object schema with properties
    pub fn object(properties: HashMap<String, Schema>, required: Vec<String>) -> Self {
        Self::Object {
            properties,
            required,
            additional_properties: true,
        }
    }
}

/// Simplified schema type for quick type checking
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SchemaType {
    /// String type
    String,
    /// Integer type
    Integer,
    /// Floating-point number type
    Number,
    /// Boolean type
    Boolean,
    /// Null type
    Null,
    /// Array type
    Array,
    /// Object type
    Object,
}

impl From<&Schema> for SchemaType {
    fn from(schema: &Schema) -> Self {
        match schema {
            Schema::String { .. } => Self::String,
            Schema::Integer { .. } => Self::Integer,
            Schema::Number { .. } => Self::Number,
            Schema::Boolean => Self::Boolean,
            Schema::Null => Self::Null,
            Schema::Array { .. } => Self::Array,
            Schema::Object { .. } => Self::Object,
            Schema::Any => Self::Object, // Default to most flexible
            Schema::OneOf { .. } | Schema::AllOf { .. } => Self::Object,
        }
    }
}

impl From<DomainError> for SchemaValidationError {
    fn from(error: DomainError) -> Self {
        match error {
            DomainError::ValidationError(msg) => Self::TypeMismatch {
                path: "/".to_string(),
                expected: "valid".to_string(),
                actual: msg,
            },
            _ => Self::TypeMismatch {
                path: "/".to_string(),
                expected: "valid".to_string(),
                actual: error.to_string(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_id_creation() {
        let id = SchemaId::new("test-schema-v1");
        assert_eq!(id.as_str(), "test-schema-v1");
        assert_eq!(id.to_string(), "test-schema-v1");
    }

    #[test]
    fn test_schema_allows_type() {
        let string_schema = Schema::string(Some(1), Some(100));
        assert!(string_schema.allows_type(SchemaType::String));
        assert!(!string_schema.allows_type(SchemaType::Integer));

        let any_schema = Schema::Any;
        assert!(any_schema.allows_type(SchemaType::String));
        assert!(any_schema.allows_type(SchemaType::Integer));
    }

    #[test]
    fn test_validation_cost() {
        let simple = Schema::Boolean;
        assert_eq!(simple.validation_cost(), 1);

        let complex = Schema::Object {
            properties: [
                ("id".to_string(), Schema::integer(None, None)),
                ("name".to_string(), Schema::string(Some(1), Some(100))),
            ]
            .into_iter()
            .collect(),
            required: vec!["id".to_string()],
            additional_properties: false,
        };
        assert!(complex.validation_cost() > 20);
    }

    #[test]
    fn test_schema_builders() {
        let str_schema = Schema::string(Some(1), Some(100));
        assert!(matches!(str_schema, Schema::String { .. }));

        let int_schema = Schema::integer(Some(0), Some(100));
        assert!(matches!(int_schema, Schema::Integer { .. }));

        let arr_schema = Schema::array(Some(Schema::integer(None, None)));
        assert!(matches!(arr_schema, Schema::Array { .. }));
    }
}
