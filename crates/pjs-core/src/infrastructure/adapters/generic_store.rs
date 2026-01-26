//! Generic in-memory store for thread-safe key-value storage
//!
//! Provides a reusable foundation for GAT repository implementations.
//! Uses lock-free DashMap for concurrent access per infrastructure guidelines.
//!
//! # Concurrency Model
//!
//! This store uses [`DashMap`] which provides lock-free concurrent access through
//! sharded hash maps. Each shard has its own lock, enabling high concurrency for
//! operations on different keys.
//!
//! # Iteration Consistency Guarantees
//!
//! **DashMap provides weakly consistent iteration:**
//!
//! - Individual items are always consistent (no torn reads)
//! - Items added during iteration may or may not be included
//! - Items removed during iteration may or may not be included
//! - Overall result represents a "fuzzy" snapshot of the store
//!
//! This is a fundamental trade-off for lock-free performance. For operations
//! requiring strong consistency:
//!
//! - Use single-key lookups (`get`, `contains_key`) for authoritative checks
//! - Accept eventual consistency for bulk queries (filter, iteration)
//!
//! The weakly consistent iteration model enables lock-free concurrent access
//! without the overhead of MVCC or snapshot isolation, which is appropriate
//! for in-memory session management where eventual consistency is acceptable.

use dashmap::DashMap;
use std::{hash::Hash, sync::Arc};

/// Maximum pre-allocation size for filter result vectors.
///
/// Prevents excessive memory allocation when result_limit is very large.
/// Actual allocation is min(result_limit, MAX_PREALLOC_SIZE).
const MAX_PREALLOC_SIZE: usize = 1024;

/// Generic thread-safe in-memory store
///
/// Uses `DashMap` for lock-free concurrent access with sharded hash maps.
/// Arc wrapper enables cheap cloning for shared ownership across async tasks.
///
/// # Iteration Consistency
///
/// This store uses DashMap which provides **weakly consistent iteration**:
/// - Individual items are always consistent (no torn reads)
/// - Items added during iteration may or may not be included
/// - Items removed during iteration may or may not be included
/// - Overall result represents a "fuzzy" snapshot of the store
///
/// For operations requiring strong consistency:
/// - Use single-key lookups (`get`, `contains_key`)
/// - Accept eventual consistency for bulk queries
///
/// This trade-off enables lock-free concurrent access without the overhead
/// of MVCC or snapshot isolation.
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
    ///
    /// # Consistency
    ///
    /// Returns a weakly consistent snapshot. Keys added or removed during
    /// iteration may or may not be included.
    pub fn all_keys(&self) -> Vec<K> {
        self.data.iter().map(|entry| entry.key().clone()).collect()
    }

    /// Get value by key
    ///
    /// # Consistency
    ///
    /// Single-key lookups are always consistent and provide the most recent
    /// committed value for the key.
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
    ///
    /// # Consistency
    ///
    /// Results are weakly consistent. Items added or removed during iteration
    /// may or may not be included. For authoritative checks, use single-key
    /// lookups (`get`, `contains_key`).
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

    /// Filter with bounded results and scan limit
    ///
    /// Returns at most `result_limit` items matching predicate.
    /// Stops iteration after scanning `scan_limit` items.
    ///
    /// # Consistency
    ///
    /// Results are weakly consistent. Items added or removed during iteration
    /// may or may not be included. For authoritative checks, use single-key
    /// lookups (`get`, `contains_key`).
    ///
    /// # Returns
    ///
    /// A tuple of (results, limit_reached) where:
    /// - `results`: Vec of matching items (at most `result_limit` items)
    /// - `limit_reached`: true if either scan_limit or result_limit was hit,
    ///   meaning the query stopped before examining all items. Results are
    ///   still valid but potentially incomplete.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use super::limits::{MAX_SCAN_LIMIT, MAX_RESULTS_LIMIT};
    ///
    /// let (results, truncated) = store.filter_limited(
    ///     |v| v.is_active(),
    ///     MAX_RESULTS_LIMIT,
    ///     MAX_SCAN_LIMIT,
    /// );
    ///
    /// if truncated {
    ///     // Results may be incomplete
    /// }
    /// ```
    pub fn filter_limited<P>(
        &self,
        predicate: P,
        result_limit: usize,
        scan_limit: usize,
    ) -> (Vec<V>, bool)
    where
        P: Fn(&V) -> bool,
    {
        let mut results = Vec::with_capacity(result_limit.min(MAX_PREALLOC_SIZE));
        let mut scanned = 0usize;
        let mut limit_reached = false;

        for entry in self.data.iter() {
            scanned += 1;
            if scanned > scan_limit {
                limit_reached = true;
                break;
            }

            if predicate(entry.value()) {
                results.push(entry.value().clone());
                if results.len() >= result_limit {
                    limit_reached = true;
                    break;
                }
            }
        }

        (results, limit_reached)
    }

    /// Check if key exists
    ///
    /// # Consistency
    ///
    /// Single-key lookups are always consistent and provide the most recent
    /// committed state.
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
    /// # Consistency
    ///
    /// This operation is atomic with respect to the specific key. The function
    /// is executed while holding the shard lock for that key, ensuring no
    /// concurrent modifications to the same key.
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

    /// Iterate over all entries
    ///
    /// Returns an iterator that yields references to each entry.
    /// Useful for manual iteration with early abort.
    ///
    /// # Consistency
    ///
    /// Iteration is weakly consistent. Items added or removed during iteration
    /// may or may not be included. This is a fundamental property of DashMap
    /// that enables lock-free concurrent access.
    pub fn iter(&self) -> impl Iterator<Item = dashmap::mapref::multiple::RefMulti<'_, K, V>> {
        self.data.iter()
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
    fn test_filter_limited_returns_at_most_limit_items() {
        let store: InMemoryStore<i32, i32> = InMemoryStore::new();

        for i in 0..100 {
            store.insert(i, i);
        }

        let (results, limit_reached) = store.filter_limited(|_| true, 10, 1000);

        assert_eq!(results.len(), 10);
        assert!(limit_reached);
    }

    #[test]
    fn test_filter_limited_sets_limit_reached_when_scan_exceeded() {
        let store: InMemoryStore<i32, i32> = InMemoryStore::new();

        for i in 0..100 {
            store.insert(i, i);
        }

        let (results, limit_reached) = store.filter_limited(|v| *v > 1000, 100, 50);

        assert!(results.is_empty());
        assert!(limit_reached);
    }

    #[test]
    fn test_filter_limited_sets_limit_reached_when_results_exceeded() {
        let store: InMemoryStore<i32, i32> = InMemoryStore::new();

        for i in 0..100 {
            store.insert(i, i);
        }

        let (results, limit_reached) = store.filter_limited(|_| true, 5, 1000);

        assert_eq!(results.len(), 5);
        assert!(limit_reached);
    }

    #[test]
    fn test_filter_limited_empty_store() {
        let store: InMemoryStore<i32, i32> = InMemoryStore::new();

        let (results, limit_reached) = store.filter_limited(|_| true, 10, 100);

        assert!(results.is_empty());
        assert!(!limit_reached);
    }

    #[test]
    fn test_filter_limited_no_matches() {
        let store: InMemoryStore<i32, i32> = InMemoryStore::new();

        for i in 0..10 {
            store.insert(i, i);
        }

        let (results, limit_reached) = store.filter_limited(|v| *v > 100, 10, 100);

        assert!(results.is_empty());
        assert!(!limit_reached);
    }

    #[test]
    fn test_filter_limited_partial_match_within_limits() {
        let store: InMemoryStore<i32, i32> = InMemoryStore::new();

        for i in 0..10 {
            store.insert(i, i);
        }

        let (results, limit_reached) = store.filter_limited(|v| v % 2 == 0, 100, 100);

        assert_eq!(results.len(), 5);
        assert!(!limit_reached);
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

    #[test]
    fn test_iter() {
        let store: InMemoryStore<i32, i32> = InMemoryStore::new();

        store.insert(1, 10);
        store.insert(2, 20);
        store.insert(3, 30);

        let mut count = 0;
        for entry in store.iter() {
            assert!(entry.value() == &10 || entry.value() == &20 || entry.value() == &30);
            count += 1;
        }

        assert_eq!(count, 3);
    }

    #[test]
    fn test_max_prealloc_size_limits_allocation() {
        // Verify that preallocation is bounded even with very large result_limit
        let store: InMemoryStore<i32, i32> = InMemoryStore::new();
        store.insert(1, 1);

        // Even with huge result_limit, we only preallocate MAX_PREALLOC_SIZE
        let (results, _) = store.filter_limited(|_| true, 1_000_000, 1_000_000);

        // Should still work correctly
        assert_eq!(results.len(), 1);
    }
}
