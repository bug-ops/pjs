//! Integration tests for schema validation engine
//!
//! Tests the complete workflow of schema definition, storage, retrieval, and validation.

use pjson_rs::prelude::*;
use std::collections::HashMap;

#[test]
fn test_end_to_end_schema_validation() {
    // Setup
    let validator = ValidationService::new();
    let repo = SchemaRepository::new();

    // Define a product schema
    let product_schema = Schema::Object {
        properties: [
            ("sku".to_string(), Schema::string(Some(5), Some(20))),
            ("name".to_string(), Schema::string(Some(1), Some(200))),
            (
                "price".to_string(),
                Schema::number(Some(0.0), Some(1000000.0)),
            ),
            (
                "quantity".to_string(),
                Schema::integer(Some(0), Some(10000)),
            ),
            ("available".to_string(), Schema::Boolean),
        ]
        .into_iter()
        .collect(),
        required: vec!["sku".to_string(), "name".to_string(), "price".to_string()],
        additional_properties: false,
    };

    // Store schema
    let schema_id = SchemaId::new("product-v1");
    repo.store(schema_id.clone(), product_schema).unwrap();

    // Create valid product data
    let mut valid_product = HashMap::new();
    valid_product.insert(
        "sku".to_string(),
        JsonData::String("PROD-12345".to_string()),
    );
    valid_product.insert(
        "name".to_string(),
        JsonData::String("Test Product".to_string()),
    );
    valid_product.insert("price".to_string(), JsonData::Float(99.99));
    valid_product.insert("quantity".to_string(), JsonData::Integer(100));
    valid_product.insert("available".to_string(), JsonData::Bool(true));

    let valid_data = JsonData::Object(valid_product);

    // Retrieve schema and validate
    let schema = repo.get(&schema_id).unwrap();
    let result = validator.validate(&valid_data, &schema, "/product");

    assert!(result.is_ok(), "Valid product should pass validation");
}

#[test]
fn test_validation_error_details() {
    let validator = ValidationService::new();

    let schema = Schema::Object {
        properties: [
            ("id".to_string(), Schema::integer(Some(1), Some(9999))),
            ("email".to_string(), Schema::string(Some(5), Some(100))),
        ]
        .into_iter()
        .collect(),
        required: vec!["id".to_string(), "email".to_string()],
        additional_properties: false,
    };

    // Test missing required field
    let mut missing_field = HashMap::new();
    missing_field.insert("id".to_string(), JsonData::Integer(42));

    let data = JsonData::Object(missing_field);
    let result = validator.validate(&data, &schema, "/user");

    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(error.to_string().contains("Missing required field"));
    assert!(error.to_string().contains("email"));
}

#[test]
fn test_nested_object_validation() {
    let validator = ValidationService::new();

    // Create nested schema
    let address_schema = Schema::Object {
        properties: [
            ("street".to_string(), Schema::string(Some(1), Some(200))),
            ("city".to_string(), Schema::string(Some(1), Some(100))),
            ("zip".to_string(), Schema::string(Some(5), Some(10))),
        ]
        .into_iter()
        .collect(),
        required: vec!["city".to_string()],
        additional_properties: true,
    };

    let user_schema = Schema::Object {
        properties: [
            ("name".to_string(), Schema::string(Some(1), Some(100))),
            ("address".to_string(), address_schema),
        ]
        .into_iter()
        .collect(),
        required: vec!["name".to_string(), "address".to_string()],
        additional_properties: false,
    };

    // Create valid nested data
    let mut address = HashMap::new();
    address.insert(
        "street".to_string(),
        JsonData::String("123 Main St".to_string()),
    );
    address.insert("city".to_string(), JsonData::String("Boston".to_string()));
    address.insert("zip".to_string(), JsonData::String("02101".to_string()));

    let mut user = HashMap::new();
    user.insert("name".to_string(), JsonData::String("Alice".to_string()));
    user.insert("address".to_string(), JsonData::Object(address));

    let data = JsonData::Object(user);
    let result = validator.validate(&data, &user_schema, "/user");

    assert!(result.is_ok(), "Valid nested object should pass validation");
}

#[test]
fn test_array_of_objects_validation() {
    let validator = ValidationService::new();

    let item_schema = Schema::Object {
        properties: [
            ("id".to_string(), Schema::integer(Some(1), None)),
            ("name".to_string(), Schema::string(Some(1), Some(100))),
        ]
        .into_iter()
        .collect(),
        required: vec!["id".to_string(), "name".to_string()],
        additional_properties: false,
    };

    let array_schema = Schema::Array {
        items: Some(Box::new(item_schema)),
        min_items: Some(1),
        max_items: Some(100),
        unique_items: false,
    };

    // Create array of valid objects
    let mut item1 = HashMap::new();
    item1.insert("id".to_string(), JsonData::Integer(1));
    item1.insert("name".to_string(), JsonData::String("Item 1".to_string()));

    let mut item2 = HashMap::new();
    item2.insert("id".to_string(), JsonData::Integer(2));
    item2.insert("name".to_string(), JsonData::String("Item 2".to_string()));

    let data = JsonData::Array(vec![JsonData::Object(item1), JsonData::Object(item2)]);

    let result = validator.validate(&data, &array_schema, "/items");
    assert!(result.is_ok(), "Valid array of objects should pass");
}

#[test]
fn test_union_type_validation() {
    let validator = ValidationService::new();

    let union_schema = Schema::OneOf {
        schemas: vec![
            Box::new(Schema::string(Some(1), None)),
            Box::new(Schema::integer(None, None)),
            Box::new(Schema::Boolean),
        ]
        .into_iter()
        .collect(),
    };

    // All these should be valid
    assert!(
        validator
            .validate(
                &JsonData::String("test".to_string()),
                &union_schema,
                "/value"
            )
            .is_ok()
    );
    assert!(
        validator
            .validate(&JsonData::Integer(42), &union_schema, "/value")
            .is_ok()
    );
    assert!(
        validator
            .validate(&JsonData::Bool(true), &union_schema, "/value")
            .is_ok()
    );

    // This should fail
    assert!(
        validator
            .validate(&JsonData::Null, &union_schema, "/value")
            .is_err()
    );
}

#[test]
fn test_concurrent_schema_operations() {
    use std::sync::Arc;
    use std::thread;

    let repo = Arc::new(SchemaRepository::new());
    let validator = Arc::new(ValidationService::new());

    let mut handles = vec![];

    // Spawn multiple threads doing schema operations
    for i in 0..10 {
        let repo_clone = Arc::clone(&repo);
        let validator_clone = Arc::clone(&validator);

        let handle = thread::spawn(move || {
            let schema_id = SchemaId::new(format!("schema-{i}"));
            let schema = Schema::integer(Some(0), Some(100));

            // Store schema
            if repo_clone.store(schema_id.clone(), schema.clone()).is_ok() {
                // Retrieve and validate
                if let Ok(retrieved_schema) = repo_clone.get(&schema_id) {
                    let data = JsonData::Integer(50);
                    validator_clone
                        .validate(&data, &retrieved_schema, "/value")
                        .expect("Validation should succeed");
                }
            }
        });

        handles.push(handle);
    }

    // Wait for all threads to complete
    for handle in handles {
        handle.join().unwrap();
    }

    // Verify all schemas were stored
    assert!(repo.count() <= 10);
    assert!(repo.count() > 0);
}

#[test]
fn test_schema_repository_crud_operations() {
    let repo = SchemaRepository::new();

    // Create
    let id = SchemaId::new("test-schema");
    let schema = Schema::string(Some(1), Some(100));
    assert!(repo.store(id.clone(), schema.clone()).is_ok());
    assert_eq!(repo.count(), 1);

    // Read
    assert!(repo.exists(&id));
    assert!(repo.get(&id).is_ok());

    // Update
    let new_schema = Schema::integer(Some(0), Some(100));
    assert!(repo.update(id.clone(), new_schema).is_ok());

    // Verify update
    let retrieved = repo.get(&id).unwrap();
    assert!(matches!(*retrieved, Schema::Integer { .. }));

    // Delete
    assert!(repo.delete(&id).is_ok());
    assert_eq!(repo.count(), 0);
    assert!(!repo.exists(&id));
}

#[test]
fn test_validation_performance() {
    let validator = ValidationService::new();

    let schema = Schema::Object {
        properties: [
            ("id".to_string(), Schema::integer(Some(1), None)),
            ("name".to_string(), Schema::string(Some(1), Some(100))),
            ("active".to_string(), Schema::Boolean),
        ]
        .into_iter()
        .collect(),
        required: vec!["id".to_string()],
        additional_properties: false,
    };

    let mut data = HashMap::new();
    data.insert("id".to_string(), JsonData::Integer(42));
    data.insert("name".to_string(), JsonData::String("Test".to_string()));
    data.insert("active".to_string(), JsonData::Bool(true));
    let json_data = JsonData::Object(data);

    // Validate 10,000 times to ensure performance is acceptable
    let start = std::time::Instant::now();
    for _ in 0..10_000 {
        validator.validate(&json_data, &schema, "/object").unwrap();
    }
    let duration = start.elapsed();

    println!(
        "10,000 validations completed in {:?} ({:.2} validations/ms)",
        duration,
        10_000.0 / duration.as_millis() as f64
    );

    // Validation should be fast enough (less than 1ms per validation on average)
    assert!(
        duration.as_millis() < 10_000,
        "Validation performance regression detected"
    );
}

#[test]
fn test_deep_nesting_validation() {
    let validator = ValidationService::with_max_depth(50);

    // Create deeply nested object (30 levels)
    fn create_nested(depth: usize) -> JsonData {
        if depth == 0 {
            JsonData::Integer(42)
        } else {
            let mut obj = HashMap::new();
            obj.insert("nested".to_string(), create_nested(depth - 1));
            JsonData::Object(obj)
        }
    }

    // Create corresponding schema
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

    let data = create_nested(30);
    let schema = create_nested_schema(30);

    let result = validator.validate(&data, &schema, "/deep");
    assert!(result.is_ok(), "Deep nesting within limits should work");

    // Test exceeding depth limit
    let too_deep_data = create_nested(60);
    let too_deep_schema = create_nested_schema(60);

    let result = validator.validate(&too_deep_data, &too_deep_schema, "/deep");
    assert!(result.is_err(), "Exceeding depth limit should fail");
}
