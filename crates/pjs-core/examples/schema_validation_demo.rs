//! Schema Validation Demo
//!
//! Demonstrates the schema validation engine for PJS.
//! Shows how to define schemas, validate JSON data, and handle validation errors.

use pjson_rs::prelude::*;
use std::collections::HashMap;

fn main() -> ApplicationResult<()> {
    println!("=== PJS Schema Validation Engine Demo ===\n");

    // Create validation service and schema repository
    let validator = ValidationService::new();
    let schema_repo = SchemaRepository::new();

    // Demo 1: Simple type validation
    println!("Demo 1: Simple Type Validation");
    demo_simple_types(&validator)?;
    println!();

    // Demo 2: Complex object validation
    println!("Demo 2: Complex Object Validation");
    demo_object_validation(&validator)?;
    println!();

    // Demo 3: Array validation
    println!("Demo 3: Array Validation");
    demo_array_validation(&validator)?;
    println!();

    // Demo 4: Schema repository usage
    println!("Demo 4: Schema Repository");
    demo_schema_repository(&schema_repo, &validator)?;
    println!();

    // Demo 5: OneOf and AllOf validation
    println!("Demo 5: Union and Intersection Types");
    demo_union_types(&validator)?;
    println!();

    // Demo 6: Real-world use case - User registration
    println!("Demo 6: Real-World Use Case - User Registration");
    demo_user_registration(&validator)?;

    println!("\n=== Demo Complete ===");
    Ok(())
}

fn demo_simple_types(validator: &ValidationService) -> ApplicationResult<()> {
    println!("  Testing integer range validation...");

    let age_schema = Schema::integer(Some(0), Some(150));

    let valid_age = JsonData::Integer(25);
    match validator.validate(&valid_age, &age_schema, "/age") {
        Ok(()) => println!("    ✓ Age 25 is valid"),
        Err(e) => println!("    ✗ Error: {e}"),
    }

    let invalid_age = JsonData::Integer(200);
    match validator.validate(&invalid_age, &age_schema, "/age") {
        Ok(()) => println!("    ✗ Should have failed!"),
        Err(e) => println!("    ✓ Correctly rejected: {e}"),
    }

    println!("  Testing string length validation...");

    let name_schema = Schema::string(Some(2), Some(50));

    let valid_name = JsonData::String("John Doe".to_string());
    match validator.validate(&valid_name, &name_schema, "/name") {
        Ok(()) => println!("    ✓ Name 'John Doe' is valid"),
        Err(e) => println!("    ✗ Error: {e}"),
    }

    let too_short = JsonData::String("A".to_string());
    match validator.validate(&too_short, &name_schema, "/name") {
        Ok(()) => println!("    ✗ Should have failed!"),
        Err(e) => println!("    ✓ Correctly rejected: {e}"),
    }

    Ok(())
}

fn demo_object_validation(validator: &ValidationService) -> ApplicationResult<()> {
    println!("  Creating user profile schema...");

    let mut properties = HashMap::new();
    properties.insert("id".to_string(), Schema::integer(Some(1), None));
    properties.insert("email".to_string(), Schema::string(Some(5), Some(100)));
    properties.insert("age".to_string(), Schema::integer(Some(0), Some(150)));

    let user_schema = Schema::Object {
        properties,
        required: vec!["id".to_string(), "email".to_string()],
        additional_properties: true,
    };

    println!("  Validating correct user object...");
    let mut valid_user = HashMap::new();
    valid_user.insert("id".to_string(), JsonData::Integer(1001));
    valid_user.insert(
        "email".to_string(),
        JsonData::String("user@example.com".to_string()),
    );
    valid_user.insert("age".to_string(), JsonData::Integer(30));

    let valid_data = JsonData::Object(valid_user);
    match validator.validate(&valid_data, &user_schema, "/user") {
        Ok(()) => println!("    ✓ Valid user object accepted"),
        Err(e) => println!("    ✗ Error: {e}"),
    }

    println!("  Validating user object with missing required field...");
    let mut invalid_user = HashMap::new();
    invalid_user.insert("id".to_string(), JsonData::Integer(1002));
    // Missing required "email" field

    let invalid_data = JsonData::Object(invalid_user);
    match validator.validate(&invalid_data, &user_schema, "/user") {
        Ok(()) => println!("    ✗ Should have failed!"),
        Err(e) => println!("    ✓ Correctly rejected: {e}"),
    }

    Ok(())
}

fn demo_array_validation(validator: &ValidationService) -> ApplicationResult<()> {
    println!("  Creating array schema with integer items...");

    let scores_schema = Schema::Array {
        items: Some(Box::new(Schema::integer(Some(0), Some(100)))),
        min_items: Some(1),
        max_items: Some(10),
        unique_items: false,
    };

    println!("  Validating valid scores array...");
    let valid_scores = JsonData::Array(vec![
        JsonData::Integer(85),
        JsonData::Integer(92),
        JsonData::Integer(78),
    ]);

    match validator.validate(&valid_scores, &scores_schema, "/scores") {
        Ok(()) => println!("    ✓ Valid scores array accepted"),
        Err(e) => println!("    ✗ Error: {e}"),
    }

    println!("  Validating array with invalid item...");
    let invalid_scores = JsonData::Array(vec![
        JsonData::Integer(85),
        JsonData::Integer(150), // Out of range!
        JsonData::Integer(78),
    ]);

    match validator.validate(&invalid_scores, &scores_schema, "/scores") {
        Ok(()) => println!("    ✗ Should have failed!"),
        Err(e) => println!("    ✓ Correctly rejected: {e}"),
    }

    Ok(())
}

fn demo_schema_repository(
    repo: &SchemaRepository,
    validator: &ValidationService,
) -> ApplicationResult<()> {
    println!("  Storing schemas in repository...");

    // Store user schema
    let user_id = SchemaId::new("user-v1");
    let user_schema = Schema::Object {
        properties: [
            ("id".to_string(), Schema::integer(Some(1), None)),
            ("name".to_string(), Schema::string(Some(2), Some(100))),
        ]
        .into_iter()
        .collect(),
        required: vec!["id".to_string(), "name".to_string()],
        additional_properties: false,
    };
    repo.store(user_id.clone(), user_schema)?;
    println!("    ✓ Stored 'user-v1' schema");

    // Store product schema
    let product_id = SchemaId::new("product-v1");
    let product_schema = Schema::Object {
        properties: [
            ("sku".to_string(), Schema::string(Some(5), Some(20))),
            (
                "price".to_string(),
                Schema::number(Some(0.0), Some(1000000.0)),
            ),
        ]
        .into_iter()
        .collect(),
        required: vec!["sku".to_string(), "price".to_string()],
        additional_properties: false,
    };
    repo.store(product_id.clone(), product_schema)?;
    println!("    ✓ Stored 'product-v1' schema");

    println!("  Repository statistics:");
    println!("    Total schemas: {}", repo.count());
    println!("    Schema IDs: {:?}", repo.list_ids());

    println!("  Retrieving and validating with stored schema...");
    let schema = repo.get(&user_id)?;
    let mut user_data = HashMap::new();
    user_data.insert("id".to_string(), JsonData::Integer(42));
    user_data.insert("name".to_string(), JsonData::String("Alice".to_string()));

    let data = JsonData::Object(user_data);
    match validator.validate(&data, &schema, "/user") {
        Ok(()) => println!("    ✓ Validation with stored schema succeeded"),
        Err(e) => println!("    ✗ Error: {e}"),
    }

    Ok(())
}

fn demo_union_types(validator: &ValidationService) -> ApplicationResult<()> {
    println!("  Creating OneOf schema (string or integer)...");

    let string_or_int = Schema::OneOf {
        schemas: vec![
            Box::new(Schema::string(Some(1), Some(100))),
            Box::new(Schema::integer(None, None)),
        ]
        .into_iter()
        .collect(),
    };

    println!("  Testing string value...");
    let string_val = JsonData::String("hello".to_string());
    match validator.validate(&string_val, &string_or_int, "/value") {
        Ok(()) => println!("    ✓ String value accepted"),
        Err(e) => println!("    ✗ Error: {e}"),
    }

    println!("  Testing integer value...");
    let int_val = JsonData::Integer(42);
    match validator.validate(&int_val, &string_or_int, "/value") {
        Ok(()) => println!("    ✓ Integer value accepted"),
        Err(e) => println!("    ✗ Error: {e}"),
    }

    println!("  Testing boolean value (should fail)...");
    let bool_val = JsonData::Bool(true);
    match validator.validate(&bool_val, &string_or_int, "/value") {
        Ok(()) => println!("    ✗ Should have failed!"),
        Err(e) => println!("    ✓ Correctly rejected: {e}"),
    }

    Ok(())
}

fn demo_user_registration(validator: &ValidationService) -> ApplicationResult<()> {
    println!("  Creating comprehensive user registration schema...");

    let registration_schema = Schema::Object {
        properties: [
            // Username: 3-20 characters
            ("username".to_string(), Schema::string(Some(3), Some(20))),
            // Email: 5-100 characters
            ("email".to_string(), Schema::string(Some(5), Some(100))),
            // Age: 13-120 years
            ("age".to_string(), Schema::integer(Some(13), Some(120))),
            // Password: 8-128 characters
            ("password".to_string(), Schema::string(Some(8), Some(128))),
            // Terms accepted (boolean)
            ("terms_accepted".to_string(), Schema::Boolean),
            // Optional phone number
            ("phone".to_string(), Schema::string(Some(10), Some(15))),
        ]
        .into_iter()
        .collect(),
        required: vec![
            "username".to_string(),
            "email".to_string(),
            "age".to_string(),
            "password".to_string(),
            "terms_accepted".to_string(),
        ],
        additional_properties: false,
    };

    println!("  Validating valid registration...");
    let mut valid_registration = HashMap::new();
    valid_registration.insert(
        "username".to_string(),
        JsonData::String("john_doe".to_string()),
    );
    valid_registration.insert(
        "email".to_string(),
        JsonData::String("john@example.com".to_string()),
    );
    valid_registration.insert("age".to_string(), JsonData::Integer(25));
    valid_registration.insert(
        "password".to_string(),
        JsonData::String("secureP@ssw0rd".to_string()),
    );
    valid_registration.insert("terms_accepted".to_string(), JsonData::Bool(true));

    let valid_data = JsonData::Object(valid_registration);
    match validator.validate(&valid_data, &registration_schema, "/registration") {
        Ok(()) => println!("    ✓ Valid registration accepted"),
        Err(e) => println!("    ✗ Error: {e}"),
    }

    println!("  Validating registration with short password...");
    let mut invalid_registration = HashMap::new();
    invalid_registration.insert(
        "username".to_string(),
        JsonData::String("jane_doe".to_string()),
    );
    invalid_registration.insert(
        "email".to_string(),
        JsonData::String("jane@example.com".to_string()),
    );
    invalid_registration.insert("age".to_string(), JsonData::Integer(30));
    invalid_registration.insert(
        "password".to_string(),
        JsonData::String("short".to_string()), // Too short!
    );
    invalid_registration.insert("terms_accepted".to_string(), JsonData::Bool(true));

    let invalid_data = JsonData::Object(invalid_registration);
    match validator.validate(&invalid_data, &registration_schema, "/registration") {
        Ok(()) => println!("    ✗ Should have failed!"),
        Err(e) => println!("    ✓ Correctly rejected: {e}"),
    }

    Ok(())
}
