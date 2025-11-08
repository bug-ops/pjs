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

        if let Some(min) = minimum
            && value < min
        {
            return Err(SchemaValidationError::OutOfRange {
                path: path.to_string(),
                value: value.to_string(),
                min: min.to_string(),
                max: maximum.map_or("∞".to_string(), |m| m.to_string()),
            });
        }

        if let Some(max) = maximum
            && value > max
        {
            return Err(SchemaValidationError::OutOfRange {
                path: path.to_string(),
                value: value.to_string(),
                min: minimum.map_or("-∞".to_string(), |m| m.to_string()),
                max: max.to_string(),
            });
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

        if let Some(min) = minimum
            && value < min
        {
            return Err(SchemaValidationError::OutOfRange {
                path: path.to_string(),
                value: value.to_string(),
                min: min.to_string(),
                max: maximum.map_or("∞".to_string(), |m| m.to_string()),
            });
        }

        if let Some(max) = maximum
            && value > max
        {
            return Err(SchemaValidationError::OutOfRange {
                path: path.to_string(),
                value: value.to_string(),
                min: minimum.map_or("-∞".to_string(), |m| m.to_string()),
                max: max.to_string(),
            });
        }

        Ok(())
    }

    fn validate_string(
        &self,
        data: &JsonData,
        path: &str,
        min_length: Option<usize>,
        max_length: Option<usize>,
        _pattern: &Option<String>,
        allowed_values: &Option<smallvec::SmallVec<[String; 8]>>,
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

        if let Some(min) = min_length
            && len < min
        {
            return Err(SchemaValidationError::StringLengthConstraint {
                path: path.to_string(),
                actual: len,
                min,
                max: max_length.unwrap_or(usize::MAX),
            });
        }

        if let Some(max) = max_length
            && len > max
        {
            return Err(SchemaValidationError::StringLengthConstraint {
                path: path.to_string(),
                actual: len,
                min: min_length.unwrap_or(0),
                max,
            });
        }

        if let Some(allowed) = allowed_values
            && !allowed.iter().any(|v| v.as_str() == value)
        {
            return Err(SchemaValidationError::InvalidEnumValue {
                path: path.to_string(),
                value: value.clone(),
            });
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
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

        if let Some(min) = min_items
            && len < min
        {
            return Err(SchemaValidationError::ArraySizeConstraint {
                path: path.to_string(),
                actual: len,
                min,
                max: max_items.unwrap_or(usize::MAX),
            });
        }

        if let Some(max) = max_items
            && len > max
        {
            return Err(SchemaValidationError::ArraySizeConstraint {
                path: path.to_string(),
                actual: len,
                min: min_items.unwrap_or(0),
                max,
            });
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

    #[test]
    fn test_validate_number_nan_infinity() {
        let validator = ValidationService::new();
        let schema = Schema::number(Some(0.0), Some(100.0));

        // NaN should be rejected
        let result = validator.validate(&JsonData::Float(f64::NAN), &schema, "/value");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, SchemaValidationError::TypeMismatch { .. }));

        // Infinity should be rejected
        let result = validator.validate(&JsonData::Float(f64::INFINITY), &schema, "/value");
        assert!(result.is_err());

        // Negative infinity should be rejected
        let result = validator.validate(&JsonData::Float(f64::NEG_INFINITY), &schema, "/value");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_string_enum_values() {
        let validator = ValidationService::new();
        use smallvec::SmallVec;

        let allowed_values = Some(SmallVec::from_vec(vec![
            String::from("red"),
            String::from("green"),
            String::from("blue"),
        ]));

        let schema = Schema::String {
            min_length: None,
            max_length: None,
            pattern: None,
            allowed_values,
        };

        // Valid enum values
        assert!(
            validator
                .validate(&JsonData::String("red".to_string()), &schema, "/color")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::String("green".to_string()), &schema, "/color")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::String("blue".to_string()), &schema, "/color")
                .is_ok()
        );

        // Invalid enum value
        let result = validator.validate(&JsonData::String("yellow".to_string()), &schema, "/color");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(
            err,
            SchemaValidationError::InvalidEnumValue { .. }
        ));
    }

    #[test]
    fn test_validate_array_unique_items() {
        let validator = ValidationService::new();
        let schema = Schema::Array {
            items: Some(Box::new(Schema::integer(None, None))),
            min_items: None,
            max_items: None,
            unique_items: true,
        };

        // Valid: all unique items
        let unique = JsonData::Array(vec![
            JsonData::Integer(1),
            JsonData::Integer(2),
            JsonData::Integer(3),
        ]);
        assert!(validator.validate(&unique, &schema, "/items").is_ok());

        // Invalid: duplicate items
        let duplicates = JsonData::Array(vec![
            JsonData::Integer(1),
            JsonData::Integer(2),
            JsonData::Integer(1),
        ]);
        let result = validator.validate(&duplicates, &schema, "/items");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, SchemaValidationError::DuplicateItems { .. }));
    }

    #[test]
    fn test_validate_array_min_max_items() {
        let validator = ValidationService::new();
        let schema = Schema::Array {
            items: None,
            min_items: Some(2),
            max_items: Some(4),
            unique_items: false,
        };

        // Valid: within range
        let valid = JsonData::Array(vec![JsonData::Integer(1), JsonData::Integer(2)]);
        assert!(validator.validate(&valid, &schema, "/items").is_ok());

        // Invalid: too few items
        let too_few = JsonData::Array(vec![JsonData::Integer(1)]);
        let result = validator.validate(&too_few, &schema, "/items");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(
            err,
            SchemaValidationError::ArraySizeConstraint { .. }
        ));

        // Invalid: too many items
        let too_many = JsonData::Array(vec![
            JsonData::Integer(1),
            JsonData::Integer(2),
            JsonData::Integer(3),
            JsonData::Integer(4),
            JsonData::Integer(5),
        ]);
        let result = validator.validate(&too_many, &schema, "/items");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SchemaValidationError::ArraySizeConstraint { .. }
        ));
    }

    #[test]
    fn test_validate_object_additional_properties() {
        let validator = ValidationService::new();
        let mut properties = HashMap::new();
        properties.insert("name".to_string(), Schema::string(Some(1), Some(100)));

        // Schema disallows additional properties
        let schema = Schema::Object {
            properties: properties.clone(),
            required: vec![],
            additional_properties: false,
        };

        let mut valid_obj = HashMap::new();
        valid_obj.insert("name".to_string(), JsonData::String("test".to_string()));

        // Valid: no additional properties
        let valid = JsonData::Object(valid_obj.clone());
        assert!(validator.validate(&valid, &schema, "/obj").is_ok());

        // Invalid: has additional property
        let mut invalid_obj = valid_obj;
        invalid_obj.insert("extra".to_string(), JsonData::Integer(42));
        let invalid = JsonData::Object(invalid_obj);
        let result = validator.validate(&invalid, &schema, "/obj");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(
            err,
            SchemaValidationError::AdditionalPropertyNotAllowed { .. }
        ));

        // Schema allows additional properties
        let schema_allow = Schema::Object {
            properties,
            required: vec![],
            additional_properties: true,
        };

        let mut obj_with_extra = HashMap::new();
        obj_with_extra.insert("name".to_string(), JsonData::String("test".to_string()));
        obj_with_extra.insert("extra".to_string(), JsonData::Integer(42));
        let with_extra = JsonData::Object(obj_with_extra);
        assert!(
            validator
                .validate(&with_extra, &schema_allow, "/obj")
                .is_ok()
        );
    }

    #[test]
    fn test_validate_one_of_single_match() {
        let validator = ValidationService::new();
        use smallvec::SmallVec;

        let schema = Schema::OneOf {
            schemas: SmallVec::from_vec(vec![
                Box::new(Schema::string(Some(1), None)),
                Box::new(Schema::integer(Some(0), None)),
            ]),
        };

        // Valid: matches exactly one schema (string)
        assert!(
            validator
                .validate(&JsonData::String("test".to_string()), &schema, "/value")
                .is_ok()
        );

        // Valid: matches exactly one schema (integer)
        assert!(
            validator
                .validate(&JsonData::Integer(42), &schema, "/value")
                .is_ok()
        );
    }

    #[test]
    fn test_validate_one_of_no_match() {
        let validator = ValidationService::new();
        use smallvec::SmallVec;

        let schema = Schema::OneOf {
            schemas: SmallVec::from_vec(vec![
                Box::new(Schema::string(Some(5), None)),    // min length 5
                Box::new(Schema::integer(Some(100), None)), // min 100
            ]),
        };

        // Invalid: matches no schemas (string too short, not an integer)
        let result = validator.validate(&JsonData::String("hi".to_string()), &schema, "/value");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SchemaValidationError::NoMatchingOneOf { .. }
        ));

        // Invalid: matches no schemas (integer too small, not a string)
        let result = validator.validate(&JsonData::Integer(50), &schema, "/value");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_one_of_multiple_matches() {
        let validator = ValidationService::new();
        use smallvec::SmallVec;

        let schema = Schema::OneOf {
            schemas: SmallVec::from_vec(vec![
                Box::new(Schema::integer(None, None)), // matches any integer
                Box::new(Schema::integer(Some(0), Some(100))), // matches integers 0-100
            ]),
        };

        // Invalid: matches both schemas (ambiguous)
        let result = validator.validate(&JsonData::Integer(50), &schema, "/value");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SchemaValidationError::NoMatchingOneOf { .. }
        ));
    }

    #[test]
    fn test_validate_all_of_success() {
        let validator = ValidationService::new();
        use smallvec::SmallVec;

        let schema = Schema::AllOf {
            schemas: SmallVec::from_vec(vec![
                Box::new(Schema::integer(Some(0), None)),   // >= 0
                Box::new(Schema::integer(None, Some(100))), // <= 100
            ]),
        };

        // Valid: matches all schemas
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
    }

    #[test]
    fn test_validate_all_of_failure() {
        let validator = ValidationService::new();
        use smallvec::SmallVec;

        let schema = Schema::AllOf {
            schemas: SmallVec::from_vec(vec![
                Box::new(Schema::integer(Some(0), None)),   // >= 0
                Box::new(Schema::integer(None, Some(100))), // <= 100
            ]),
        };

        // Invalid: fails first constraint
        let result = validator.validate(&JsonData::Integer(-1), &schema, "/value");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, SchemaValidationError::AllOfFailure { .. }));

        // Invalid: fails second constraint
        let result = validator.validate(&JsonData::Integer(101), &schema, "/value");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SchemaValidationError::AllOfFailure { .. }
        ));
    }

    #[test]
    fn test_validate_max_depth_exceeded() {
        let validator = ValidationService::with_max_depth(5);

        // Create nested structure that exceeds max depth
        fn create_nested(depth: usize) -> JsonData {
            if depth == 0 {
                JsonData::Integer(42)
            } else {
                let mut obj = HashMap::new();
                obj.insert("nested".to_string(), create_nested(depth - 1));
                JsonData::Object(obj)
            }
        }

        fn create_nested_schema(depth: usize) -> Schema {
            if depth == 0 {
                Schema::integer(None, None)
            } else {
                Schema::Object {
                    properties: [("nested".to_string(), create_nested_schema(depth - 1))]
                        .into_iter()
                        .collect(),
                    required: vec![],
                    additional_properties: false,
                }
            }
        }

        let data = create_nested(10);
        let schema = create_nested_schema(10);

        // Should fail due to depth limit
        let result = validator.validate(&data, &schema, "/deep");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, SchemaValidationError::TypeMismatch { .. }));
    }

    #[test]
    fn test_validate_any_schema() {
        let validator = ValidationService::new();
        let schema = Schema::Any;

        // Any schema accepts all types
        assert!(validator.validate(&JsonData::Null, &schema, "/").is_ok());
        assert!(
            validator
                .validate(&JsonData::Bool(true), &schema, "/")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::Integer(42), &schema, "/")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::Float(std::f64::consts::PI), &schema, "/")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::String("test".to_string()), &schema, "/")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::Array(vec![]), &schema, "/")
                .is_ok()
        );
        assert!(
            validator
                .validate(&JsonData::Object(HashMap::new()), &schema, "/")
                .is_ok()
        );
    }

    #[test]
    fn test_validate_type_mismatches() {
        let validator = ValidationService::new();

        // Test all type mismatches
        let test_cases = vec![
            (Schema::Null, JsonData::Integer(42), "null"),
            (
                Schema::Boolean,
                JsonData::String("true".to_string()),
                "boolean",
            ),
            (
                Schema::integer(None, None),
                JsonData::String("42".to_string()),
                "integer",
            ),
            (
                Schema::number(None, None),
                JsonData::String("3.14".to_string()),
                "number",
            ),
            (Schema::string(None, None), JsonData::Integer(42), "string"),
            (
                Schema::Array {
                    items: None,
                    min_items: None,
                    max_items: None,
                    unique_items: false,
                },
                JsonData::Integer(42),
                "array",
            ),
            (
                Schema::Object {
                    properties: HashMap::new(),
                    required: vec![],
                    additional_properties: true,
                },
                JsonData::Integer(42),
                "object",
            ),
        ];

        for (schema, data, expected_type) in test_cases {
            let result = validator.validate(&data, &schema, "/test");
            assert!(result.is_err(), "Expected error for {expected_type}");
            let err = result.unwrap_err();
            assert!(
                matches!(err, SchemaValidationError::TypeMismatch { .. }),
                "Expected TypeMismatch for {expected_type}"
            );
        }
    }

    #[test]
    fn test_default_validation_service() {
        let default = ValidationService::default();
        let created = ValidationService::new();

        // Both should have same max_depth
        let schema = Schema::integer(None, None);
        let data = JsonData::Integer(42);

        assert!(default.validate(&data, &schema, "/").is_ok());
        assert!(created.validate(&data, &schema, "/").is_ok());
    }
}
