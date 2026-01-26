//! Generic in-memory store for thread-safe key-value storage
//!
//! Provides a reusable foundation for GAT repository implementations.
//! Uses lock-free DashMap for concurrent access per infrastructure guidelines.

use dashmap::DashMap;
use std::{hash::Hash, sync::Arc};

/// Generic thread-safe in-memory store
///
/// Uses `DashMap` for lock-free concurrent access with sharded hash maps.
/// Arc wrapper enables cheap cloning for shared ownership across async tasks.
#[derive(Debug)]
pub struct InMemoryStore<K, V>
where
    K: Eq + Hash + Clone + Send + Sync,
    V: Clone + Send + Sync,
{
    data: Arc<DashMap<K, V>>,
}

impl<K, V> InMemoryStore<K, V>
where
    K: Eq + Hash + Clone + Send + Sync,
    V: Clone + Send + Sync,
{
    /// Create empty store
    pub fn new() -> Self {
        Self {
            data: Arc::new(DashMap::new()),
        }
    }

    /// Get number of entries
    pub fn count(&self) -> usize {
        self.data.len()
    }

    /// Remove all entries
    pub fn clear(&self) {
        self.data.clear();
    }

    /// Get all keys
    pub fn all_keys(&self) -> Vec<K> {
        self.data.iter().map(|entry| entry.key().clone()).collect()
    }

    /// Get value by key
    pub fn get(&self, key: &K) -> Option<V> {
        self.data.get(key).map(|entry| entry.value().clone())
    }

    /// Insert or update value
    pub fn insert(&self, key: K, value: V) -> Option<V> {
        self.data.insert(key, value)
    }

    /// Remove entry by key
    pub fn remove(&self, key: &K) -> Option<V> {
        self.data.remove(key).map(|(_k, v)| v)
    }

    /// Filter values by predicate
    pub fn filter<P>(&self, predicate: P) -> Vec<V>
    where
        P: Fn(&V) -> bool,
    {
        self.data
            .iter()
            .filter(|entry| predicate(entry.value()))
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Check if key exists
    pub fn contains_key(&self, key: &K) -> bool {
        self.data.contains_key(key)
    }

    /// Check if store is empty
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Atomic read-modify-write operation
    ///
    /// Applies function to mutable value reference if key exists.
    /// Returns the result of the function or None if key not found.
    ///
    /// # Example
    /// ```ignore
    /// store.update_with(&stream_id, |stream| {
    ///     stream.complete()
    /// });
    /// ```
    pub fn update_with<F, R>(&self, key: &K, f: F) -> Option<R>
    where
        F: FnOnce(&mut V) -> R,
    {
        self.data.get_mut(key).map(|mut entry| f(entry.value_mut()))
    }
}

impl<K, V> Default for InMemoryStore<K, V>
where
    K: Eq + Hash + Clone + Send + Sync,
    V: Clone + Send + Sync,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<K, V> Clone for InMemoryStore<K, V>
where
    K: Eq + Hash + Clone + Send + Sync,
    V: Clone + Send + Sync,
{
    fn clone(&self) -> Self {
        Self {
            data: Arc::clone(&self.data),
        }
    }
}

// Type aliases for domain-specific stores
use crate::domain::{
    aggregates::StreamSession,
    entities::Stream,
    value_objects::{SessionId, StreamId},
};

/// Session store type alias
pub type SessionStore = InMemoryStore<SessionId, StreamSession>;

/// Stream store type alias
pub type StreamStore = InMemoryStore<StreamId, Stream>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_operations() {
        let store: InMemoryStore<String, i32> = InMemoryStore::new();

        assert!(store.is_empty());
        assert_eq!(store.count(), 0);

        store.insert("a".to_string(), 1);
        store.insert("b".to_string(), 2);

        assert_eq!(store.count(), 2);
        assert_eq!(store.get(&"a".to_string()), Some(1));
        assert_eq!(store.get(&"c".to_string()), None);

        let keys = store.all_keys();
        assert_eq!(keys.len(), 2);

        store.remove(&"a".to_string());
        assert_eq!(store.count(), 1);

        store.clear();
        assert!(store.is_empty());
    }

    #[test]
    fn test_filter() {
        let store: InMemoryStore<String, i32> = InMemoryStore::new();

        store.insert("a".to_string(), 1);
        store.insert("b".to_string(), 2);
        store.insert("c".to_string(), 3);

        let evens = store.filter(|v| v % 2 == 0);
        assert_eq!(evens, vec![2]);
    }

    #[test]
    fn test_clone_shares_data() {
        let store1: InMemoryStore<String, i32> = InMemoryStore::new();
        store1.insert("key".to_string(), 42);

        let store2 = store1.clone();
        assert_eq!(store2.get(&"key".to_string()), Some(42));

        store2.insert("another".to_string(), 100);
        assert_eq!(store1.get(&"another".to_string()), Some(100));
    }

    #[test]
    fn test_contains_key() {
        let store: InMemoryStore<String, i32> = InMemoryStore::new();

        assert!(!store.contains_key(&"key".to_string()));
        store.insert("key".to_string(), 42);
        assert!(store.contains_key(&"key".to_string()));
    }

    /// Test concurrent access from multiple threads
    ///
    /// Verifies DashMap's lock-free behavior under contention
    #[test]
    fn test_concurrent_access() {
        use std::thread;

        let store: InMemoryStore<i32, String> = InMemoryStore::new();
        let store_clone = store.clone();

        // Spawn thread to write concurrently
        let write_handle = thread::spawn(move || {
            for i in 0..100 {
                store_clone.insert(i, format!("thread1-{}", i));
            }
        });

        // Write from main thread concurrently
        for i in 100..200 {
            store.insert(i, format!("thread2-{}", i));
        }

        write_handle.join().unwrap();

        // Verify all writes succeeded (DashMap handles concurrent writes)
        assert_eq!(store.count(), 200);
        assert_eq!(store.get(&50), Some("thread1-50".to_string()));
        assert_eq!(store.get(&150), Some("thread2-150".to_string()));

        // Test concurrent reads
        let read_store = store.clone();
        let read_handle = thread::spawn(move || {
            for i in 0..200 {
                read_store.get(&i); // Lock-free reads
            }
        });

        // Read from main thread while other thread reads
        for i in 0..200 {
            store.get(&i);
        }

        read_handle.join().unwrap();
    }
}
