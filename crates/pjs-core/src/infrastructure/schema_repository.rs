//! Schema repository implementation
//!
//! Provides thread-safe in-memory storage for schema definitions.

use dashmap::DashMap;
use std::sync::Arc;

use crate::{
    ApplicationError, ApplicationResult,
    domain::value_objects::{Schema, SchemaId},
};

/// Thread-safe in-memory schema repository
///
/// Stores schema definitions with concurrent read/write access using `DashMap`.
/// Suitable for production use with high-concurrency workloads.
///
/// # Design Philosophy
/// - Lock-free concurrent access using sharded hash maps
/// - Zero-copy schema retrieval using Arc
/// - Type-safe schema identifiers
/// - Simple CRUD operations
///
/// # Examples
/// ```
/// # use pjson_rs::infrastructure::adapters::SchemaRepository;
/// # use pjson_rs::domain::value_objects::{Schema, SchemaId};
/// let repo = SchemaRepository::new();
/// let schema_id = SchemaId::new("user-v1");
/// let schema = Schema::integer(Some(1), Some(100));
///
/// repo.store(schema_id.clone(), schema).unwrap();
/// let retrieved = repo.get(&schema_id).unwrap();
/// ```
pub struct SchemaRepository {
    schemas: Arc<DashMap<String, Arc<Schema>>>,
}

impl SchemaRepository {
    /// Create a new empty schema repository
    pub fn new() -> Self {
        Self {
            schemas: Arc::new(DashMap::new()),
        }
    }

    /// Store a schema definition
    ///
    /// # Arguments
    /// * `id` - Unique schema identifier
    /// * `schema` - Schema definition to store
    ///
    /// # Returns
    /// `Ok(())` if stored successfully
    ///
    /// # Errors
    /// Returns error if schema with same ID already exists (use `update` instead)
    pub fn store(&self, id: SchemaId, schema: Schema) -> ApplicationResult<()> {
        let key = id.as_str().to_string();

        if self.schemas.contains_key(&key) {
            return Err(ApplicationError::Conflict(format!(
                "Schema with ID '{}' already exists",
                id
            )));
        }

        self.schemas.insert(key, Arc::new(schema));
        Ok(())
    }

    /// Update an existing schema definition
    ///
    /// # Arguments
    /// * `id` - Schema identifier
    /// * `schema` - New schema definition
    ///
    /// # Returns
    /// `Ok(previous_schema)` with the replaced schema if it existed
    ///
    /// # Errors
    /// Returns error if schema does not exist (use `store` instead)
    pub fn update(&self, id: SchemaId, schema: Schema) -> ApplicationResult<Arc<Schema>> {
        let key = id.as_str().to_string();

        match self.schemas.insert(key.clone(), Arc::new(schema)) {
            Some(previous) => Ok(previous),
            None => Err(ApplicationError::NotFound(format!(
                "Schema with ID '{}' not found",
                id
            ))),
        }
    }

    /// Store or update a schema (upsert operation)
    ///
    /// # Arguments
    /// * `id` - Schema identifier
    /// * `schema` - Schema definition
    ///
    /// # Returns
    /// `Ok(Some(previous))` if schema was updated, `Ok(None)` if newly created
    pub fn store_or_update(
        &self,
        id: SchemaId,
        schema: Schema,
    ) -> ApplicationResult<Option<Arc<Schema>>> {
        let key = id.as_str().to_string();
        let previous = self.schemas.insert(key, Arc::new(schema));
        Ok(previous)
    }

    /// Retrieve a schema by ID
    ///
    /// # Arguments
    /// * `id` - Schema identifier
    ///
    /// # Returns
    /// `Ok(Arc<Schema>)` if schema exists
    ///
    /// # Errors
    /// Returns error if schema not found
    pub fn get(&self, id: &SchemaId) -> ApplicationResult<Arc<Schema>> {
        let key = id.as_str();

        self.schemas
            .get(key)
            .map(|entry| Arc::clone(entry.value()))
            .ok_or_else(|| ApplicationError::NotFound(format!("Schema with ID '{}' not found", id)))
    }

    /// Check if a schema exists
    ///
    /// # Arguments
    /// * `id` - Schema identifier
    ///
    /// # Returns
    /// `true` if schema exists, `false` otherwise
    pub fn exists(&self, id: &SchemaId) -> bool {
        self.schemas.contains_key(id.as_str())
    }

    /// Delete a schema by ID
    ///
    /// # Arguments
    /// * `id` - Schema identifier
    ///
    /// # Returns
    /// `Ok(schema)` with the deleted schema if it existed
    ///
    /// # Errors
    /// Returns error if schema not found
    pub fn delete(&self, id: &SchemaId) -> ApplicationResult<Arc<Schema>> {
        let key = id.as_str();

        self.schemas
            .remove(key)
            .map(|(_, schema)| schema)
            .ok_or_else(|| ApplicationError::NotFound(format!("Schema with ID '{}' not found", id)))
    }

    /// List all schema IDs
    ///
    /// # Returns
    /// Vector of all schema IDs in the repository
    pub fn list_ids(&self) -> Vec<SchemaId> {
        self.schemas
            .iter()
            .map(|entry| SchemaId::new(entry.key().clone()))
            .collect()
    }

    /// Get number of schemas stored
    ///
    /// # Returns
    /// Count of schemas in repository
    pub fn count(&self) -> usize {
        self.schemas.len()
    }

    /// Clear all schemas
    ///
    /// Removes all stored schemas from the repository.
    pub fn clear(&self) {
        self.schemas.clear();
    }
}

impl Default for SchemaRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for SchemaRepository {
    fn clone(&self) -> Self {
        Self {
            schemas: Arc::clone(&self.schemas),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_schema() -> Schema {
        Schema::integer(Some(0), Some(100))
    }

    #[test]
    fn test_store_and_retrieve() {
        let repo = SchemaRepository::new();
        let id = SchemaId::new("test-schema");
        let schema = create_test_schema();

        repo.store(id.clone(), schema).unwrap();
        assert!(repo.exists(&id));

        let retrieved = repo.get(&id).unwrap();
        assert!(matches!(*retrieved, Schema::Integer { .. }));
    }

    #[test]
    fn test_store_duplicate_fails() {
        let repo = SchemaRepository::new();
        let id = SchemaId::new("test-schema");
        let schema = create_test_schema();

        repo.store(id.clone(), schema.clone()).unwrap();
        let result = repo.store(id, schema);

        assert!(result.is_err());
    }

    #[test]
    fn test_update() {
        let repo = SchemaRepository::new();
        let id = SchemaId::new("test-schema");
        let schema1 = create_test_schema();
        let schema2 = Schema::string(Some(1), Some(100));

        repo.store(id.clone(), schema1).unwrap();
        let previous = repo.update(id.clone(), schema2).unwrap();

        assert!(matches!(*previous, Schema::Integer { .. }));

        let current = repo.get(&id).unwrap();
        assert!(matches!(*current, Schema::String { .. }));
    }

    #[test]
    fn test_store_or_update() {
        let repo = SchemaRepository::new();
        let id = SchemaId::new("test-schema");
        let schema1 = create_test_schema();
        let schema2 = Schema::string(Some(1), Some(100));

        // First insert
        let result = repo.store_or_update(id.clone(), schema1).unwrap();
        assert!(result.is_none());

        // Update
        let result = repo.store_or_update(id.clone(), schema2).unwrap();
        assert!(result.is_some());
        assert!(matches!(*result.unwrap(), Schema::Integer { .. }));
    }

    #[test]
    fn test_delete() {
        let repo = SchemaRepository::new();
        let id = SchemaId::new("test-schema");
        let schema = create_test_schema();

        repo.store(id.clone(), schema).unwrap();
        assert!(repo.exists(&id));

        let deleted = repo.delete(&id).unwrap();
        assert!(matches!(*deleted, Schema::Integer { .. }));
        assert!(!repo.exists(&id));
    }

    #[test]
    fn test_list_ids() {
        let repo = SchemaRepository::new();

        repo.store(SchemaId::new("schema1"), create_test_schema())
            .unwrap();
        repo.store(SchemaId::new("schema2"), create_test_schema())
            .unwrap();

        let ids = repo.list_ids();
        assert_eq!(ids.len(), 2);
        assert!(ids.iter().any(|id| id.as_str() == "schema1"));
        assert!(ids.iter().any(|id| id.as_str() == "schema2"));
    }

    #[test]
    fn test_count_and_clear() {
        let repo = SchemaRepository::new();

        repo.store(SchemaId::new("schema1"), create_test_schema())
            .unwrap();
        repo.store(SchemaId::new("schema2"), create_test_schema())
            .unwrap();

        assert_eq!(repo.count(), 2);

        repo.clear();
        assert_eq!(repo.count(), 0);
    }

    #[test]
    fn test_concurrent_access() {
        use std::thread;

        let repo = SchemaRepository::new();
        let repo_clone = repo.clone();

        let handle = thread::spawn(move || {
            for i in 0..100 {
                let id = SchemaId::new(format!("schema-{i}"));
                repo_clone.store(id, create_test_schema()).ok(); // Some may fail due to duplicates
            }
        });

        for i in 0..100 {
            let id = SchemaId::new(format!("schema-{i}"));
            repo.store(id, create_test_schema()).ok();
        }

        handle.join().unwrap();

        // Should have stored all unique schemas
        assert!(repo.count() <= 100);
        assert!(repo.count() > 0);
    }
}
