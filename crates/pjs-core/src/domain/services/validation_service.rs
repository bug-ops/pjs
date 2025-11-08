//! Schema validation domain service
//!
//! Provides core validation logic for JSON data against schemas.
//! This is a domain service as it contains business logic that doesn't
//! naturally fit into a value object or entity.

use std::collections::HashSet;

use crate::domain::value_objects::{
    JsonData, Schema, SchemaValidationError, SchemaValidationResult,
};

/// Schema validation service
///
/// Validates JSON data against schema definitions following a subset of
/// JSON Schema specification. Designed for high-performance validation
/// in streaming scenarios.
///
/// # Design Philosophy
/// - Zero allocation validation where possible
/// - Early exit on validation failures for performance
/// - Detailed error messages with full path context
/// - Composable validators for complex schemas
///
/// # Examples
/// ```
/// # use pjson_rs::domain::services::ValidationService;
/// # use pjson_rs::domain::value_objects::{Schema, JsonData};
/// let validator = ValidationService::new();
/// let schema = Schema::integer(Some(0), Some(100));
/// let data = JsonData::Integer(50);
///
/// assert!(validator.validate(&data, &schema, "/value").is_ok());
/// ```
pub struct ValidationService {
    /// Maximum validation depth to prevent stack overflow
    max_depth: usize,
}

impl ValidationService {
    /// Maximum default validation depth
    const DEFAULT_MAX_DEPTH: usize = 32;

    /// Create a new validation service with default configuration
    pub fn new() -> Self {
        Self {
            max_depth: Self::DEFAULT_MAX_DEPTH,
        }
    }

    /// Create validation service with custom maximum depth
    ///
    /// # Arguments
    /// * `max_depth` - Maximum nested validation depth
    pub fn with_max_depth(max_depth: usize) -> Self {
        Self { max_depth }
    }

    /// Validate JSON data against a schema
    ///
    /// Performs comprehensive validation of JSON data against the provided schema,
    /// including type checking, constraint validation, and nested structure validation.
    ///
    /// # Arguments
    /// * `data` - JSON data to validate
    /// * `schema` - Schema to validate against
    /// * `path` - Current JSON path for error reporting
    ///
    /// # Returns
    /// `Ok(())` if validation succeeds, error with details if validation fails
    ///
    /// # Errors
    /// Returns `SchemaValidationError` with context when validation fails
    pub fn validate(
        &self,
        data: &JsonData,
        schema: &Schema,
        path: &str,
    ) -> SchemaValidationResult<()> {
        self.validate_with_depth(data, schema, path, 0)
    }

    /// Internal validation with depth tracking
    fn validate_with_depth(
        &self,
        data: &JsonData,
        schema: &Schema,
        path: &str,
        depth: usize,
    ) -> SchemaValidationResult<()> {
        // Prevent stack overflow from deeply nested structures
        if depth > self.max_depth {
            return Err(SchemaValidationError::TypeMismatch {
                path: path.to_string(),
                expected: "maximum depth not exceeded".to_string(),
                actual: format!("depth {depth} exceeds maximum {}", self.max_depth),
            });
        }

        match schema {
            Schema::Any => Ok(()),
            Schema::Null => self.validate_null(data, path),
            Schema::Boolean => self.validate_boolean(data, path),
            Schema::Integer { minimum, maximum } => {
                self.validate_integer(data, path, *minimum, *maximum)
            }
            Schema::Number { minimum, maximum } => {
                self.validate_number(data, path, *minimum, *maximum)
            }
            Schema::String {
                min_length,
                max_length,
                pattern,
                allowed_values,
            } => self.validate_string(
                data,
                path,
                *min_length,
                *max_length,
                pattern,
                allowed_values,
            ),
            Schema::Array {
                items,
                min_items,
                max_items,
                unique_items,
            } => self.validate_array(
                data,
                path,
                items,
                *min_items,
                *max_items,
                *unique_items,
                depth,
            ),
            Schema::Object {
                properties,
                required,
                additional_properties,
            } => self.validate_object(
                data,
                path,
                properties,
                required,
                *additional_properties,
                depth,
            ),
            Schema::OneOf { schemas } => self.validate_one_of(data, path, schemas, depth),
            Schema::AllOf { schemas } => self.validate_all_of(data, path, schemas, depth),
        }
    }

    fn validate_null(&self, data: &JsonData, path: &str) -> SchemaValidationResult<()> {
        match data {
            JsonData::Null => Ok(()),
            _ => Err(SchemaValidationError::TypeMismatch {
                path: path.to_string(),
                expected: "null".to_string(),
                actual: Self::get_type_name(data).to_string(),
            }),
        }
    }

    fn validate_boolean(&self, data: &JsonData, path: &str) -> SchemaValidationResult<()> {
        match data {
            JsonData::Bool(_) => Ok(()),
            _ => Err(SchemaValidationError::TypeMismatch {
                path: path.to_string(),
                expected: "boolean".to_string(),
                actual: Self::get_type_name(data).to_string(),
            }),
        }
    }

    fn get_type_name(data: &JsonData) -> &'static str {
        match data {
            JsonData::Null => "null",
            JsonData::Bool(_) => "boolean",
            JsonData::Integer(_) => "integer",
            JsonData::Float(_) => "number",
            JsonData::String(_) => "string",
            JsonData::Array(_) => "array",
            JsonData::Object(_) => "object",
        }
    }

    fn validate_integer(
        &self,
        data: &JsonData,
        path: &str,
        minimum: Option<i64>,
        maximum: Option<i64>,
    ) -> SchemaValidationResult<()> {
        let value = match data {
            JsonData::Integer(v) => *v,
            _ => {
                return Err(SchemaValidationError::TypeMismatch {
                    path: path.to_string(),
                    expected: "integer".to_string(),
                    actual: Self::get_type_name(data).to_string(),
                });
            }
        };

        if let Some(min) = minimum {
            if value < min {
                return Err(SchemaValidationError::OutOfRange {
                    path: path.to_string(),
                    value: value.to_string(),
                    min: min.to_string(),
                    max: maximum.map_or("∞".to_string(), |m| m.to_string()),
                });
            }
        }

        if let Some(max) = maximum {
            if value > max {
                return Err(SchemaValidationError::OutOfRange {
                    path: path.to_string(),
                    value: value.to_string(),
                    min: minimum.map_or("-∞".to_string(), |m| m.to_string()),
                    max: max.to_string(),
                });
            }
        }

        Ok(())
    }

    fn validate_number(
        &self,
        data: &JsonData,
        path: &str,
        minimum: Option<f64>,
        maximum: Option<f64>,
    ) -> SchemaValidationResult<()> {
        let value = match data {
            JsonData::Float(v) => *v,
            JsonData::Integer(v) => *v as f64,
            _ => {
                return Err(SchemaValidationError::TypeMismatch {
                    path: path.to_string(),
                    expected: "number".to_string(),
                    actual: Self::get_type_name(data).to_string(),
                });
            }
        };

        // Validate that the number is finite (not NaN or Infinity)
        if value.is_nan() || value.is_infinite() {
            return Err(SchemaValidationError::TypeMismatch {
                path: path.to_string(),
                expected: "finite number".to_string(),
                actual: format!("{}", value),
            });
        }

        if let Some(min) = minimum {
            if value < min {
                return Err(SchemaValidationError::OutOfRange {
                    path: path.to_string(),
                    value: value.to_string(),
                    min: min.to_string(),
                    max: maximum.map_or("∞".to_string(), |m| m.to_string()),
                });
            }
        }

        if let Some(max) = maximum {
            if value > max {
                return Err(SchemaValidationError::OutOfRange {
                    path: path.to_string(),
                    value: value.to_string(),
                    min: minimum.map_or("-∞".to_string(), |m| m.to_string()),
                    max: max.to_string(),
                });
            }
        }

        Ok(())
    }

    fn validate_string(
        &self,
        data: &JsonData,
        path: &str,
        min_length: Option<usize>,
        max_length: Option<usize>,
        _pattern: &Option<std::sync::Arc<str>>,
        allowed_values: &Option<smallvec::SmallVec<[std::sync::Arc<str>; 8]>>,
    ) -> SchemaValidationResult<()> {
        let value = match data {
            JsonData::String(s) => s,
            _ => {
                return Err(SchemaValidationError::TypeMismatch {
                    path: path.to_string(),
                    expected: "string".to_string(),
                    actual: Self::get_type_name(data).to_string(),
                });
            }
        };

        let len = value.chars().count();

        if let Some(min) = min_length {
            if len < min {
                return Err(SchemaValidationError::StringLengthConstraint {
                    path: path.to_string(),
                    actual: len,
                    min,
                    max: max_length.unwrap_or(usize::MAX),
                });
            }
        }

        if let Some(max) = max_length {
            if len > max {
                return Err(SchemaValidationError::StringLengthConstraint {
                    path: path.to_string(),
                    actual: len,
                    min: min_length.unwrap_or(0),
                    max,
                });
            }
        }

        if let Some(allowed) = allowed_values {
            if !allowed.iter().any(|v| v.as_ref() == value) {
                return Err(SchemaValidationError::InvalidEnumValue {
                    path: path.to_string(),
                    value: value.clone(),
                });
            }
        }

        Ok(())
    }

    fn validate_array(
        &self,
        data: &JsonData,
        path: &str,
        items: &Option<Box<Schema>>,
        min_items: Option<usize>,
        max_items: Option<usize>,
        unique_items: bool,
        depth: usize,
    ) -> SchemaValidationResult<()> {
        let arr = match data {
            JsonData::Array(a) => a,
            _ => {
                return Err(SchemaValidationError::TypeMismatch {
                    path: path.to_string(),
                    expected: "array".to_string(),
                    actual: Self::get_type_name(data).to_string(),
                });
            }
        };

        let len = arr.len();

        if let Some(min) = min_items {
            if len < min {
                return Err(SchemaValidationError::ArraySizeConstraint {
                    path: path.to_string(),
                    actual: len,
                    min,
                    max: max_items.unwrap_or(usize::MAX),
                });
            }
        }

        if let Some(max) = max_items {
            if len > max {
                return Err(SchemaValidationError::ArraySizeConstraint {
                    path: path.to_string(),
                    actual: len,
                    min: min_items.unwrap_or(0),
                    max,
                });
            }
        }

        if unique_items {
            let mut seen = HashSet::with_capacity(arr.len());
            for item in arr {
                // Use JsonData's Hash implementation directly for efficient uniqueness check
                if !seen.insert(item) {
                    return Err(SchemaValidationError::DuplicateItems {
                        path: path.to_string(),
                    });
                }
            }
        }

        if let Some(item_schema) = items {
            // Pre-allocate path buffer to avoid repeated allocations
            let mut path_buffer = String::with_capacity(path.len() + 16);
            for (i, item) in arr.iter().enumerate() {
                path_buffer.clear();
                path_buffer.push_str(path);
                path_buffer.push('[');
                use std::fmt::Write;
                write!(&mut path_buffer, "{}", i).unwrap();
                path_buffer.push(']');

                self.validate_with_depth(item, item_schema, &path_buffer, depth + 1)?;
            }
        }

        Ok(())
    }

    fn validate_object(
        &self,
        data: &JsonData,
        path: &str,
        properties: &std::collections::HashMap<String, Schema>,
        required: &[String],
        additional_properties: bool,
        depth: usize,
    ) -> SchemaValidationResult<()> {
        let obj = match data {
            JsonData::Object(o) => o,
            _ => {
                return Err(SchemaValidationError::TypeMismatch {
                    path: path.to_string(),
                    expected: "object".to_string(),
                    actual: Self::get_type_name(data).to_string(),
                });
            }
        };

        // Check required fields
        for field in required {
            if !obj.contains_key(field) {
                return Err(SchemaValidationError::MissingRequired {
                    path: path.to_string(),
                    field: field.clone(),
                });
            }
        }

        // Validate defined properties
        let mut path_buffer = String::with_capacity(path.len() + 32);
        for (key, value) in obj {
            if let Some(prop_schema) = properties.get(key) {
                path_buffer.clear();
                path_buffer.push_str(path);
                path_buffer.push('/');
                path_buffer.push_str(key);
                self.validate_with_depth(value, prop_schema, &path_buffer, depth + 1)?;
            } else if !additional_properties {
                return Err(SchemaValidationError::AdditionalPropertyNotAllowed {
                    path: path.to_string(),
                    property: key.clone(),
                });
            }
        }

        Ok(())
    }

    fn validate_one_of(
        &self,
        data: &JsonData,
        path: &str,
        schemas: &[Box<Schema>],
        depth: usize,
    ) -> SchemaValidationResult<()> {
        let mut match_count = 0;

        for schema in schemas {
            if self
                .validate_with_depth(data, schema, path, depth + 1)
                .is_ok()
            {
                match_count += 1;
                // Early exit: if we found 2 matches, we know it's invalid
                if match_count > 1 {
                    return Err(SchemaValidationError::NoMatchingOneOf {
                        path: path.to_string(),
                    });
                }
            }
        }

        if match_count == 1 {
            Ok(())
        } else {
            Err(SchemaValidationError::NoMatchingOneOf {
                path: path.to_string(),
            })
        }
    }

    fn validate_all_of(
        &self,
        data: &JsonData,
        path: &str,
        schemas: &[Box<Schema>],
        depth: usize,
    ) -> SchemaValidationResult<()> {
        let mut failures = Vec::new();

        for (i, schema) in schemas.iter().enumerate() {
            if self
                .validate_with_depth(data, schema, path, depth + 1)
                .is_err()
            {
                failures.push(i);
            }
        }

        if failures.is_empty() {
            Ok(())
        } else {
            Err(SchemaValidationError::AllOfFailure {
                path: path.to_string(),
                failures: failures
                    .iter()
                    .map(|i| i.to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
            })
        }
    }
}

impl Default for ValidationService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_validate_null() {
        let validator = ValidationService::new();
        let schema = Schema::Null;
        let data = JsonData::Null;

        assert!(validator.validate(&data, &schema, "/").is_ok());

        let invalid = JsonData::Integer(42);
        assert!(validator.validate(&invalid, &schema, "/").is_err());
    }

    #[test]
    fn test_validate_boolean() {
        let validator = ValidationService::new();
        let schema = Schema::Boolean;

        assert!(
            validator
                .validate(&JsonData::Bool(true), &schema, "/flag")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::Bool(false), &schema, "/flag")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::Integer(1), &schema, "/flag")
                .is_err()
        );
    }

    #[test]
    fn test_validate_integer_range() {
        let validator = ValidationService::new();
        let schema = Schema::integer(Some(0), Some(100));

        assert!(
            validator
                .validate(&JsonData::Integer(50), &schema, "/value")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::Integer(0), &schema, "/value")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::Integer(100), &schema, "/value")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::Integer(-1), &schema, "/value")
                .is_err()
        );
        assert!(
            validator
                .validate(&JsonData::Integer(101), &schema, "/value")
                .is_err()
        );
    }

    #[test]
    fn test_validate_string_length() {
        let validator = ValidationService::new();
        let schema = Schema::string(Some(2), Some(10));

        assert!(
            validator
                .validate(&JsonData::String("hello".to_string()), &schema, "/")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::String("hi".to_string()), &schema, "/")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::String("0123456789".to_string()), &schema, "/")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::String("a".to_string()), &schema, "/")
                .is_err()
        );
        assert!(
            validator
                .validate(&JsonData::String("12345678901".to_string()), &schema, "/")
                .is_err()
        );
    }

    #[test]
    fn test_validate_array() {
        let validator = ValidationService::new();
        let schema = Schema::Array {
            items: Some(Box::new(Schema::integer(Some(0), None))),
            min_items: Some(1),
            max_items: Some(5),
            unique_items: false,
        };

        let valid = JsonData::Array(vec![JsonData::Integer(1), JsonData::Integer(2)]);
        assert!(validator.validate(&valid, &schema, "/items").is_ok());

        let empty = JsonData::Array(vec![]);
        assert!(validator.validate(&empty, &schema, "/items").is_err());

        let invalid_item = JsonData::Array(vec![JsonData::Integer(-1)]);
        assert!(
            validator
                .validate(&invalid_item, &schema, "/items")
                .is_err()
        );
    }

    #[test]
    fn test_validate_object() {
        let validator = ValidationService::new();
        let mut properties = HashMap::new();
        properties.insert("id".to_string(), Schema::integer(Some(1), None));
        properties.insert("name".to_string(), Schema::string(Some(1), Some(100)));

        let schema = Schema::object(properties, vec!["id".to_string()]);

        let mut valid_obj = HashMap::new();
        valid_obj.insert("id".to_string(), JsonData::Integer(42));
        valid_obj.insert("name".to_string(), JsonData::String("test".to_string()));

        let valid = JsonData::Object(valid_obj);
        assert!(validator.validate(&valid, &schema, "/user").is_ok());

        let mut missing_required = HashMap::new();
        missing_required.insert("name".to_string(), JsonData::String("test".to_string()));
        let invalid = JsonData::Object(missing_required);
        assert!(validator.validate(&invalid, &schema, "/user").is_err());
    }

    #[test]
    fn test_validate_number() {
        let validator = ValidationService::new();
        let schema = Schema::number(Some(0.0), Some(100.0));

        assert!(
            validator
                .validate(&JsonData::Float(50.0), &schema, "/value")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::Integer(50), &schema, "/value")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::Float(0.0), &schema, "/value")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::Float(100.0), &schema, "/value")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::Float(-0.1), &schema, "/value")
                .is_err()
        );
        assert!(
            validator
                .validate(&JsonData::Float(100.1), &schema, "/value")
                .is_err()
        );
    }
}
