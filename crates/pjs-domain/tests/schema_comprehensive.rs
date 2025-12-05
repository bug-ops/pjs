//! Comprehensive tests for Schema value object
//!
//! Tests cover schema creation, validation rules, type checking,
//! cost estimation, and error handling.

use pjson_rs_domain::value_objects::{Schema, SchemaId, SchemaType, SchemaValidationError};
use std::collections::HashMap;

// ============================================================================
// SchemaId Tests
// ============================================================================

#[test]
fn test_schema_id_creation() {
    let id = SchemaId::new("user-schema-v1");
    assert_eq!(id.as_str(), "user-schema-v1");
}

#[test]
fn test_schema_id_from_string() {
    let id = SchemaId::new("test".to_string());
    assert_eq!(id.as_str(), "test");
}

#[test]
fn test_schema_id_display() {
    let id = SchemaId::new("my-schema");
    assert_eq!(id.to_string(), "my-schema");
    assert_eq!(format!("{id}"), "my-schema");
}

#[test]
fn test_schema_id_equality() {
    let id1 = SchemaId::new("schema-1");
    let id2 = SchemaId::new("schema-1");
    let id3 = SchemaId::new("schema-2");

    assert_eq!(id1, id2);
    assert_ne!(id1, id3);
}

#[test]
fn test_schema_id_clone() {
    let id1 = SchemaId::new("test");
    let id2 = id1.clone();
    assert_eq!(id1, id2);
}

#[test]
fn test_schema_id_hash() {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let id = SchemaId::new("test");
    let mut hasher = DefaultHasher::new();
    id.hash(&mut hasher);
    let _ = hasher.finish();
}

// ============================================================================
// Schema Builder Tests
// ============================================================================

#[test]
fn test_schema_string_builder() {
    let schema = Schema::string(Some(1), Some(100));

    if let Schema::String {
        min_length,
        max_length,
        pattern,
        allowed_values,
    } = schema
    {
        assert_eq!(min_length, Some(1));
        assert_eq!(max_length, Some(100));
        assert!(pattern.is_none());
        assert!(allowed_values.is_none());
    } else {
        panic!("Expected String schema");
    }
}

#[test]
fn test_schema_string_no_constraints() {
    let schema = Schema::string(None, None);
    assert!(matches!(schema, Schema::String { .. }));
}

#[test]
fn test_schema_integer_builder() {
    let schema = Schema::integer(Some(0), Some(100));

    if let Schema::Integer { minimum, maximum } = schema {
        assert_eq!(minimum, Some(0));
        assert_eq!(maximum, Some(100));
    } else {
        panic!("Expected Integer schema");
    }
}

#[test]
fn test_schema_integer_unbounded() {
    let schema = Schema::integer(None, None);
    if let Schema::Integer { minimum, maximum } = schema {
        assert!(minimum.is_none());
        assert!(maximum.is_none());
    } else {
        panic!("Expected Integer schema");
    }
}

#[test]
fn test_schema_number_builder() {
    let schema = Schema::number(Some(0.0), Some(100.5));

    if let Schema::Number { minimum, maximum } = schema {
        assert_eq!(minimum, Some(0.0));
        assert_eq!(maximum, Some(100.5));
    } else {
        panic!("Expected Number schema");
    }
}

#[test]
fn test_schema_array_builder() {
    let schema = Schema::array(Some(Schema::integer(None, None)));

    if let Schema::Array {
        items,
        min_items,
        max_items,
        unique_items,
    } = schema
    {
        assert!(items.is_some());
        assert!(min_items.is_none());
        assert!(max_items.is_none());
        assert!(!unique_items);
    } else {
        panic!("Expected Array schema");
    }
}

#[test]
fn test_schema_array_no_item_type() {
    let schema = Schema::array(None);
    if let Schema::Array { items, .. } = schema {
        assert!(items.is_none());
    } else {
        panic!("Expected Array schema");
    }
}

#[test]
fn test_schema_object_builder() {
    let mut properties = HashMap::new();
    properties.insert("id".to_string(), Schema::integer(Some(1), None));
    properties.insert("name".to_string(), Schema::string(Some(1), Some(100)));

    let required = vec!["id".to_string(), "name".to_string()];
    let schema = Schema::object(properties, required);

    if let Schema::Object {
        properties: props,
        required: req,
        additional_properties,
    } = schema
    {
        assert_eq!(props.len(), 2);
        assert_eq!(req.len(), 2);
        assert!(additional_properties);
    } else {
        panic!("Expected Object schema");
    }
}

#[test]
fn test_schema_object_empty() {
    let schema = Schema::object(HashMap::new(), vec![]);
    if let Schema::Object { properties, .. } = schema {
        assert!(properties.is_empty());
    } else {
        panic!("Expected Object schema");
    }
}

// ============================================================================
// Schema Type Checking Tests
// ============================================================================

#[test]
fn test_schema_allows_type_string() {
    let schema = Schema::string(None, None);
    assert!(schema.allows_type(SchemaType::String));
    assert!(!schema.allows_type(SchemaType::Integer));
    assert!(!schema.allows_type(SchemaType::Boolean));
}

#[test]
fn test_schema_allows_type_integer() {
    let schema = Schema::integer(None, None);
    assert!(schema.allows_type(SchemaType::Integer));
    assert!(!schema.allows_type(SchemaType::String));
    assert!(!schema.allows_type(SchemaType::Number));
}

#[test]
fn test_schema_allows_type_number() {
    let schema = Schema::number(None, None);
    assert!(schema.allows_type(SchemaType::Number));
    assert!(!schema.allows_type(SchemaType::Integer));
}

#[test]
fn test_schema_allows_type_boolean() {
    let schema = Schema::Boolean;
    assert!(schema.allows_type(SchemaType::Boolean));
    assert!(!schema.allows_type(SchemaType::String));
}

#[test]
fn test_schema_allows_type_null() {
    let schema = Schema::Null;
    assert!(schema.allows_type(SchemaType::Null));
    assert!(!schema.allows_type(SchemaType::Boolean));
}

#[test]
fn test_schema_allows_type_array() {
    let schema = Schema::array(None);
    assert!(schema.allows_type(SchemaType::Array));
    assert!(!schema.allows_type(SchemaType::Object));
}

#[test]
fn test_schema_allows_type_object() {
    let schema = Schema::object(HashMap::new(), vec![]);
    assert!(schema.allows_type(SchemaType::Object));
    assert!(!schema.allows_type(SchemaType::Array));
}

#[test]
fn test_schema_allows_type_any() {
    let schema = Schema::Any;
    assert!(schema.allows_type(SchemaType::String));
    assert!(schema.allows_type(SchemaType::Integer));
    assert!(schema.allows_type(SchemaType::Boolean));
    assert!(schema.allows_type(SchemaType::Null));
    assert!(schema.allows_type(SchemaType::Array));
    assert!(schema.allows_type(SchemaType::Object));
}

#[test]
fn test_schema_allows_type_oneof() {
    use smallvec::SmallVec;

    let schema = Schema::OneOf {
        schemas: SmallVec::from_vec(vec![
            Box::new(Schema::string(None, None)),
            Box::new(Schema::integer(None, None)),
        ]),
    };

    assert!(schema.allows_type(SchemaType::String));
    assert!(schema.allows_type(SchemaType::Integer));
    assert!(!schema.allows_type(SchemaType::Boolean));
}

#[test]
fn test_schema_allows_type_allof() {
    use smallvec::SmallVec;

    let schema = Schema::AllOf {
        schemas: SmallVec::from_vec(vec![
            Box::new(Schema::Any),
            Box::new(Schema::string(None, None)),
        ]),
    };

    assert!(schema.allows_type(SchemaType::String));
    assert!(!schema.allows_type(SchemaType::Integer));
}

// ============================================================================
// Schema Validation Cost Tests
// ============================================================================

#[test]
fn test_validation_cost_null() {
    assert_eq!(Schema::Null.validation_cost(), 1);
}

#[test]
fn test_validation_cost_boolean() {
    assert_eq!(Schema::Boolean.validation_cost(), 1);
}

#[test]
fn test_validation_cost_any() {
    assert_eq!(Schema::Any.validation_cost(), 1);
}

#[test]
fn test_validation_cost_integer() {
    let schema = Schema::integer(Some(0), Some(100));
    assert_eq!(schema.validation_cost(), 5);
}

#[test]
fn test_validation_cost_number() {
    let schema = Schema::number(Some(0.0), Some(100.0));
    assert_eq!(schema.validation_cost(), 5);
}

#[test]
fn test_validation_cost_string_simple() {
    let schema = Schema::string(Some(1), Some(100));
    assert_eq!(schema.validation_cost(), 10);
}

#[test]
fn test_validation_cost_string_with_pattern() {
    let schema = Schema::String {
        min_length: None,
        max_length: None,
        pattern: Some("[a-z]+".to_string()),
        allowed_values: None,
    };
    assert_eq!(schema.validation_cost(), 50);
}

#[test]
fn test_validation_cost_array_no_items() {
    let schema = Schema::array(None);
    assert_eq!(schema.validation_cost(), 10 + 1);
}

#[test]
fn test_validation_cost_array_with_items() {
    let schema = Schema::array(Some(Schema::integer(None, None)));
    assert_eq!(schema.validation_cost(), 10 + 5);
}

#[test]
fn test_validation_cost_object() {
    let mut properties = HashMap::new();
    properties.insert("id".to_string(), Schema::integer(None, None));
    properties.insert("name".to_string(), Schema::string(None, None));

    let schema = Schema::object(properties, vec![]);
    assert!(schema.validation_cost() > 20);
}

#[test]
fn test_validation_cost_oneof() {
    use smallvec::SmallVec;

    let schema = Schema::OneOf {
        schemas: SmallVec::from_vec(vec![
            Box::new(Schema::string(None, None)),
            Box::new(Schema::integer(None, None)),
        ]),
    };

    assert!(schema.validation_cost() > 30);
}

#[test]
fn test_validation_cost_allof() {
    use smallvec::SmallVec;

    let schema = Schema::AllOf {
        schemas: SmallVec::from_vec(vec![
            Box::new(Schema::string(None, None)),
            Box::new(Schema::integer(None, None)),
        ]),
    };

    assert!(schema.validation_cost() >= 20 + 10 + 5);
}

// ============================================================================
// SchemaType Conversion Tests
// ============================================================================

#[test]
fn test_schema_type_from_schema_string() {
    let schema = Schema::string(None, None);
    let schema_type = SchemaType::from(&schema);
    assert_eq!(schema_type, SchemaType::String);
}

#[test]
fn test_schema_type_from_schema_integer() {
    let schema = Schema::integer(None, None);
    let schema_type = SchemaType::from(&schema);
    assert_eq!(schema_type, SchemaType::Integer);
}

#[test]
fn test_schema_type_from_schema_number() {
    let schema = Schema::number(None, None);
    let schema_type = SchemaType::from(&schema);
    assert_eq!(schema_type, SchemaType::Number);
}

#[test]
fn test_schema_type_from_schema_boolean() {
    let schema = Schema::Boolean;
    let schema_type = SchemaType::from(&schema);
    assert_eq!(schema_type, SchemaType::Boolean);
}

#[test]
fn test_schema_type_from_schema_null() {
    let schema = Schema::Null;
    let schema_type = SchemaType::from(&schema);
    assert_eq!(schema_type, SchemaType::Null);
}

#[test]
fn test_schema_type_from_schema_array() {
    let schema = Schema::array(None);
    let schema_type = SchemaType::from(&schema);
    assert_eq!(schema_type, SchemaType::Array);
}

#[test]
fn test_schema_type_from_schema_object() {
    let schema = Schema::object(HashMap::new(), vec![]);
    let schema_type = SchemaType::from(&schema);
    assert_eq!(schema_type, SchemaType::Object);
}

#[test]
fn test_schema_type_from_schema_any() {
    let schema = Schema::Any;
    let schema_type = SchemaType::from(&schema);
    assert_eq!(schema_type, SchemaType::Object);
}

// ============================================================================
// SchemaValidationError Tests
// ============================================================================

#[test]
fn test_validation_error_type_mismatch() {
    let error = SchemaValidationError::TypeMismatch {
        path: "/field".to_string(),
        expected: "string".to_string(),
        actual: "integer".to_string(),
    };

    let msg = error.to_string();
    assert!(msg.contains("/field"));
    assert!(msg.contains("string"));
    assert!(msg.contains("integer"));
}

#[test]
fn test_validation_error_missing_required() {
    let error = SchemaValidationError::MissingRequired {
        path: "/object".to_string(),
        field: "required_field".to_string(),
    };

    let msg = error.to_string();
    assert!(msg.contains("Missing required field"));
    assert!(msg.contains("/object"));
    assert!(msg.contains("required_field"));
}

#[test]
fn test_validation_error_out_of_range() {
    let error = SchemaValidationError::OutOfRange {
        path: "/number".to_string(),
        value: "150".to_string(),
        min: "0".to_string(),
        max: "100".to_string(),
    };

    let msg = error.to_string();
    assert!(msg.contains("out of range"));
    assert!(msg.contains("150"));
}

#[test]
fn test_validation_error_string_length_constraint() {
    let error = SchemaValidationError::StringLengthConstraint {
        path: "/name".to_string(),
        actual: 150,
        min: 1,
        max: 100,
    };

    let msg = error.to_string();
    assert!(msg.contains("String length constraint"));
    assert!(msg.contains("150"));
}

#[test]
fn test_validation_error_pattern_mismatch() {
    let error = SchemaValidationError::PatternMismatch {
        path: "/email".to_string(),
        value: "invalid".to_string(),
        pattern: "[a-z]+@[a-z]+".to_string(),
    };

    let msg = error.to_string();
    assert!(msg.contains("Pattern mismatch"));
    assert!(msg.contains("invalid"));
}

#[test]
fn test_validation_error_array_size_constraint() {
    let error = SchemaValidationError::ArraySizeConstraint {
        path: "/items".to_string(),
        actual: 150,
        min: 0,
        max: 100,
    };

    let msg = error.to_string();
    assert!(msg.contains("Array size constraint"));
}

#[test]
fn test_validation_error_duplicate_items() {
    let error = SchemaValidationError::DuplicateItems {
        path: "/array".to_string(),
    };

    let msg = error.to_string();
    assert!(msg.contains("Unique items constraint"));
    assert!(msg.contains("duplicate"));
}

#[test]
fn test_validation_error_invalid_enum_value() {
    let error = SchemaValidationError::InvalidEnumValue {
        path: "/status".to_string(),
        value: "unknown".to_string(),
    };

    let msg = error.to_string();
    assert!(msg.contains("Invalid enum value"));
    assert!(msg.contains("unknown"));
}

#[test]
fn test_validation_error_additional_property_not_allowed() {
    let error = SchemaValidationError::AdditionalPropertyNotAllowed {
        path: "/object".to_string(),
        property: "extra_field".to_string(),
    };

    let msg = error.to_string();
    assert!(msg.contains("Additional property not allowed"));
    assert!(msg.contains("extra_field"));
}

#[test]
fn test_validation_error_no_matching_oneof() {
    let error = SchemaValidationError::NoMatchingOneOf {
        path: "/value".to_string(),
    };

    let msg = error.to_string();
    assert!(msg.contains("No matching schema in OneOf"));
}

#[test]
fn test_validation_error_allof_failure() {
    let error = SchemaValidationError::AllOfFailure {
        path: "/value".to_string(),
        failures: "0, 2".to_string(),
    };

    let msg = error.to_string();
    assert!(msg.contains("Not all schemas match in AllOf"));
    assert!(msg.contains("0, 2"));
}

// ============================================================================
// Schema Variant Tests
// ============================================================================

#[test]
fn test_schema_string_full() {
    use smallvec::SmallVec;

    let schema = Schema::String {
        min_length: Some(5),
        max_length: Some(50),
        pattern: Some("^[a-z]+$".to_string()),
        allowed_values: Some(SmallVec::from_vec(vec![
            "test".to_string(),
            "example".to_string(),
        ])),
    };

    if let Schema::String {
        min_length,
        max_length,
        pattern,
        allowed_values,
    } = schema
    {
        assert_eq!(min_length, Some(5));
        assert_eq!(max_length, Some(50));
        assert!(pattern.is_some());
        assert_eq!(allowed_values.as_ref().unwrap().len(), 2);
    } else {
        panic!("Expected String schema");
    }
}

#[test]
fn test_schema_array_full() {
    let schema = Schema::Array {
        items: Some(Box::new(Schema::string(None, None))),
        min_items: Some(1),
        max_items: Some(10),
        unique_items: true,
    };

    if let Schema::Array {
        items,
        min_items,
        max_items,
        unique_items,
    } = schema
    {
        assert!(items.is_some());
        assert_eq!(min_items, Some(1));
        assert_eq!(max_items, Some(10));
        assert!(unique_items);
    } else {
        panic!("Expected Array schema");
    }
}

#[test]
fn test_schema_object_full() {
    let mut properties = HashMap::new();
    properties.insert("id".to_string(), Schema::integer(Some(1), None));
    properties.insert("name".to_string(), Schema::string(Some(1), Some(100)));
    properties.insert("active".to_string(), Schema::Boolean);

    let schema = Schema::Object {
        properties,
        required: vec!["id".to_string(), "name".to_string()],
        additional_properties: false,
    };

    if let Schema::Object {
        properties,
        required,
        additional_properties,
    } = schema
    {
        assert_eq!(properties.len(), 3);
        assert_eq!(required.len(), 2);
        assert!(!additional_properties);
    } else {
        panic!("Expected Object schema");
    }
}

#[test]
fn test_schema_oneof_creation() {
    use smallvec::SmallVec;

    let schema = Schema::OneOf {
        schemas: SmallVec::from_vec(vec![
            Box::new(Schema::string(None, None)),
            Box::new(Schema::integer(None, None)),
            Box::new(Schema::Boolean),
        ]),
    };

    if let Schema::OneOf { schemas } = schema {
        assert_eq!(schemas.len(), 3);
    } else {
        panic!("Expected OneOf schema");
    }
}

#[test]
fn test_schema_allof_creation() {
    use smallvec::SmallVec;

    let schema = Schema::AllOf {
        schemas: SmallVec::from_vec(vec![
            Box::new(Schema::string(Some(1), Some(100))),
            Box::new(Schema::String {
                min_length: None,
                max_length: None,
                pattern: Some("^[A-Z]".to_string()),
                allowed_values: None,
            }),
        ]),
    };

    if let Schema::AllOf { schemas } = schema {
        assert_eq!(schemas.len(), 2);
    } else {
        panic!("Expected AllOf schema");
    }
}

// ============================================================================
// Schema Clone and Equality Tests
// ============================================================================

#[test]
fn test_schema_clone() {
    let schema = Schema::string(Some(1), Some(100));
    let cloned = schema.clone();
    assert_eq!(schema, cloned);
}

#[test]
fn test_schema_equality() {
    let schema1 = Schema::integer(Some(0), Some(100));
    let schema2 = Schema::integer(Some(0), Some(100));
    let schema3 = Schema::integer(Some(0), Some(200));

    assert_eq!(schema1, schema2);
    assert_ne!(schema1, schema3);
}

#[test]
fn test_schema_debug() {
    let schema = Schema::Boolean;
    let debug = format!("{:?}", schema);
    assert!(debug.contains("Boolean"));
}

// ============================================================================
// Serialization Tests
// ============================================================================

#[test]
fn test_schema_serialize_deserialize() {
    let schema = Schema::string(Some(1), Some(100));
    let serialized = serde_json::to_string(&schema).unwrap();
    let deserialized: Schema = serde_json::from_str(&serialized).unwrap();
    assert_eq!(schema, deserialized);
}

#[test]
fn test_schema_id_serialize_deserialize() {
    let id = SchemaId::new("test-schema");
    let serialized = serde_json::to_string(&id).unwrap();
    let deserialized: SchemaId = serde_json::from_str(&serialized).unwrap();
    assert_eq!(id, deserialized);
}

#[test]
fn test_validation_error_serialize_deserialize() {
    let error = SchemaValidationError::TypeMismatch {
        path: "/test".to_string(),
        expected: "string".to_string(),
        actual: "integer".to_string(),
    };

    let serialized = serde_json::to_string(&error).unwrap();
    let deserialized: SchemaValidationError = serde_json::from_str(&serialized).unwrap();
    assert_eq!(error, deserialized);
}
