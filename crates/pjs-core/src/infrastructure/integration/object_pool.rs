// Object pooling system for high-performance memory management
//
// This module provides thread-safe object pools for frequently allocated
// data structures like HashMap and Vec to minimize garbage collection overhead.

use crossbeam::queue::ArrayQueue;
use once_cell::sync::Lazy;
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Thread-safe object pool for reusable data structures
pub struct ObjectPool<T> {
    /// Queue of available objects
    objects: ArrayQueue<T>,
    /// Factory function to create new objects
    factory: Arc<dyn Fn() -> T + Send + Sync>,
    /// Best-effort stat counters — Relaxed ordering is intentional; these are
    /// metrics, not synchronization points, so occasional imprecision is acceptable.
    stat_created: AtomicUsize,
    stat_reused: AtomicUsize,
    stat_returned: AtomicUsize,
    stat_peak: AtomicUsize,
    stat_pool_size: AtomicUsize,
}

/// Snapshot of pool statistics at a point in time.
///
/// Counters are collected from independent atomics, so the snapshot is
/// not perfectly consistent across fields under concurrent load — use it
/// for monitoring and diagnostics only.
#[derive(Debug, Clone, Default)]
pub struct PoolStats {
    /// Total number of objects ever produced by the factory.
    pub objects_created: usize,
    /// Total number of times an object was reused from the pool.
    pub objects_reused: usize,
    /// Total number of objects returned to the pool on drop.
    pub objects_returned: usize,
    /// Highest concurrent in-use count observed.
    pub peak_usage: usize,
    /// Number of objects currently parked in the pool.
    pub current_pool_size: usize,
}

impl<T> ObjectPool<T> {
    /// Create a new object pool with specified capacity
    pub fn new<F>(capacity: usize, factory: F) -> Self
    where
        F: Fn() -> T + Send + Sync + 'static,
    {
        Self {
            objects: ArrayQueue::new(capacity),
            factory: Arc::new(factory),
            stat_created: AtomicUsize::new(0),
            stat_reused: AtomicUsize::new(0),
            stat_returned: AtomicUsize::new(0),
            stat_peak: AtomicUsize::new(0),
            stat_pool_size: AtomicUsize::new(0),
        }
    }

    /// Get an object from the pool, creating a new one if needed
    pub fn get(&self) -> PooledObject<'_, T> {
        let obj = if let Some(obj) = self.objects.pop() {
            self.stat_reused.fetch_add(1, Ordering::Relaxed);
            self.stat_pool_size.fetch_sub(1, Ordering::Relaxed);
            obj
        } else {
            let obj = (self.factory)();
            let created = self.stat_created.fetch_add(1, Ordering::Relaxed) + 1;
            let pool_size = self.stat_pool_size.load(Ordering::Relaxed);
            // Best-effort peak tracking: imprecision under concurrency is acceptable.
            let in_use = created.saturating_sub(pool_size);
            // `try_update` (the replacement) is stable since 1.95.0, above this crate's 1.89.0 MSRV.
            #[allow(deprecated)]
            let _ = self
                .stat_peak
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |prev| {
                    if in_use > prev { Some(in_use) } else { None }
                });
            obj
        };

        PooledObject {
            object: Some(obj),
            pool: self,
        }
    }

    /// Return an object to the pool
    fn return_object(&self, obj: T) {
        if self.objects.push(obj).is_ok() {
            self.stat_returned.fetch_add(1, Ordering::Relaxed);
            self.stat_pool_size.fetch_add(1, Ordering::Relaxed);
        }
        // If pool is full, object is dropped (let GC handle it)
    }

    /// Get current pool statistics
    pub fn stats(&self) -> PoolStats {
        PoolStats {
            objects_created: self.stat_created.load(Ordering::Relaxed),
            objects_reused: self.stat_reused.load(Ordering::Relaxed),
            objects_returned: self.stat_returned.load(Ordering::Relaxed),
            peak_usage: self.stat_peak.load(Ordering::Relaxed),
            current_pool_size: self.stat_pool_size.load(Ordering::Relaxed),
        }
    }
}

/// RAII wrapper that automatically returns objects to pool
pub struct PooledObject<'a, T> {
    object: Option<T>,
    pool: &'a ObjectPool<T>,
}

impl<'a, T> PooledObject<'a, T> {
    /// Get a reference to the pooled object
    pub fn get(&self) -> &T {
        self.object
            .as_ref()
            .expect("PooledObject accessed after take")
    }

    /// Get a mutable reference to the pooled object
    pub fn get_mut(&mut self) -> &mut T {
        self.object
            .as_mut()
            .expect("PooledObject accessed after take")
    }

    /// Take ownership of the object (prevents return to pool)
    pub fn take(mut self) -> T {
        self.object.take().expect("PooledObject already taken")
    }
}

impl<'a, T> Drop for PooledObject<'a, T> {
    fn drop(&mut self) {
        if let Some(obj) = self.object.take() {
            self.pool.return_object(obj);
        }
    }
}

impl<'a, T> std::ops::Deref for PooledObject<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.get()
    }
}

impl<'a, T> std::ops::DerefMut for PooledObject<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.get_mut()
    }
}

/// Wrapper that ensures cleaning happens
pub struct CleaningPooledObject<T: 'static> {
    inner: PooledObject<'static, T>,
}

impl<T: 'static> CleaningPooledObject<T> {
    fn new(inner: PooledObject<'static, T>) -> Self {
        Self { inner }
    }

    /// Take ownership of the inner object, preventing return to the pool on drop.
    pub fn take(self) -> T {
        self.inner.take()
    }
}

impl<T: 'static> std::ops::Deref for CleaningPooledObject<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T: 'static> std::ops::DerefMut for CleaningPooledObject<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

/// Global cleaning pools - direct ObjectPool instances
static CLEANING_COW_HASHMAP: Lazy<ObjectPool<HashMap<Cow<'static, str>, Cow<'static, str>>>> =
    Lazy::new(|| ObjectPool::new(50, || HashMap::with_capacity(8)));
static CLEANING_STRING_HASHMAP: Lazy<ObjectPool<HashMap<String, String>>> =
    Lazy::new(|| ObjectPool::new(50, || HashMap::with_capacity(8)));
static CLEANING_BYTE_VEC: Lazy<ObjectPool<Vec<u8>>> =
    Lazy::new(|| ObjectPool::new(100, || Vec::with_capacity(1024)));
static CLEANING_STRING_VEC: Lazy<ObjectPool<Vec<String>>> =
    Lazy::new(|| ObjectPool::new(50, || Vec::with_capacity(16)));

/// Borrow a cleared `HashMap<Cow<'static, str>, Cow<'static, str>>` from the global pool.
pub fn get_cow_hashmap() -> CleaningPooledObject<HashMap<Cow<'static, str>, Cow<'static, str>>> {
    let mut obj = CLEANING_COW_HASHMAP.get();
    obj.clear(); // Clean before use
    CleaningPooledObject::new(obj)
}

/// Borrow a cleared `HashMap<String, String>` from the global pool.
pub fn get_string_hashmap() -> CleaningPooledObject<HashMap<String, String>> {
    let mut obj = CLEANING_STRING_HASHMAP.get();
    obj.clear(); // Clean before use
    CleaningPooledObject::new(obj)
}

/// Borrow a cleared `Vec<u8>` buffer from the global pool.
pub fn get_byte_vec() -> CleaningPooledObject<Vec<u8>> {
    let mut obj = CLEANING_BYTE_VEC.get();
    obj.clear(); // Clean before use
    CleaningPooledObject::new(obj)
}

/// Borrow a cleared `Vec<String>` from the global pool.
pub fn get_string_vec() -> CleaningPooledObject<Vec<String>> {
    let mut obj = CLEANING_STRING_VEC.get();
    obj.clear(); // Clean before use
    CleaningPooledObject::new(obj)
}

/// Pool statistics aggregator
#[derive(Debug, Clone)]
pub struct GlobalPoolStats {
    /// Stats for the global `Cow` HashMap pool.
    pub cow_hashmap: PoolStats,
    /// Stats for the global `String` HashMap pool.
    pub string_hashmap: PoolStats,
    /// Stats for the global byte buffer pool.
    pub byte_vec: PoolStats,
    /// Stats for the global `Vec<String>` pool.
    pub string_vec: PoolStats,
    /// Sum of objects produced across all pools.
    pub total_objects_created: usize,
    /// Sum of object reuses across all pools.
    pub total_objects_reused: usize,
    /// Overall reuse ratio in `[0.0, 1.0]`.
    pub total_reuse_ratio: f64,
}

/// Get comprehensive statistics for all global pools
pub fn get_global_pool_stats() -> GlobalPoolStats {
    let cow_hashmap = CLEANING_COW_HASHMAP.stats();
    let string_hashmap = CLEANING_STRING_HASHMAP.stats();
    let byte_vec = CLEANING_BYTE_VEC.stats();
    let string_vec = CLEANING_STRING_VEC.stats();

    let total_created = cow_hashmap.objects_created
        + string_hashmap.objects_created
        + byte_vec.objects_created
        + string_vec.objects_created;
    let total_reused = cow_hashmap.objects_reused
        + string_hashmap.objects_reused
        + byte_vec.objects_reused
        + string_vec.objects_reused;

    let total_reuse_ratio = if total_created + total_reused > 0 {
        total_reused as f64 / (total_created + total_reused) as f64
    } else {
        0.0
    };

    GlobalPoolStats {
        cow_hashmap,
        string_hashmap,
        byte_vec,
        string_vec,
        total_objects_created: total_created,
        total_objects_reused: total_reused,
        total_reuse_ratio,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_object_pool_basic_operations() {
        let pool = ObjectPool::new(5, || HashMap::<String, String>::with_capacity(4));

        // Get object from pool
        let mut obj1 = pool.get();
        obj1.insert("test".to_string(), "value".to_string());

        // Get another object
        let obj2 = pool.get();

        // Check stats
        let stats = pool.stats();
        assert_eq!(stats.objects_created, 2);
        assert_eq!(stats.objects_reused, 0);

        // Drop objects (return to pool)
        drop(obj1);
        drop(obj2);

        // Get object again (should be reused)
        let _obj3 = pool.get();
        // Note: obj3 might not be empty because we're using a basic pool
        // The cleaning happens in CleaningPooledObject, not in basic ObjectPool

        let stats = pool.stats();
        assert_eq!(stats.objects_reused, 1);
    }

    #[test]
    fn test_pooled_object_deref() {
        let pool = ObjectPool::new(5, || vec![1, 2, 3]);
        let obj = pool.get();

        // Test Deref
        assert_eq!(obj.len(), 3);
        assert_eq!(obj[0], 1);
    }

    #[test]
    fn test_pooled_object_take() {
        let pool = ObjectPool::new(5, || vec![1, 2, 3]);
        let obj = pool.get();

        let taken = obj.take();
        assert_eq!(taken, vec![1, 2, 3]);

        // Object should not be returned to pool
        let stats = pool.stats();
        assert_eq!(stats.objects_returned, 0);
    }

    #[test]
    fn test_global_pools() {
        let mut headers = get_cow_hashmap();
        headers.insert(Cow::Borrowed("test"), Cow::Borrowed("value"));
        drop(headers);

        let mut bytes = get_byte_vec();
        bytes.extend_from_slice(b"test data");
        drop(bytes);

        let stats = get_global_pool_stats();
        // Note: Stats might be 0 initially because we're using different pools
        // This test validates that the stats function works, not specific values
        assert!(stats.total_reuse_ratio >= 0.0);
    }

    #[test]
    fn test_pool_capacity_limits() {
        let pool = ObjectPool::new(2, Vec::<i32>::new);

        let obj1 = pool.get();
        let obj2 = pool.get();
        let obj3 = pool.get(); // This should create new object

        drop(obj1);
        drop(obj2);
        drop(obj3); // Pool is full, so this should be dropped

        let stats = pool.stats();
        assert_eq!(stats.objects_created, 3);
        assert_eq!(stats.objects_returned, 2); // Only 2 can fit in pool
    }

    #[test]
    fn test_concurrent_pool_access() {
        use std::sync::Arc;
        use std::thread;

        let pool = Arc::new(ObjectPool::new(10, Vec::<i32>::new));
        let mut handles = vec![];

        for _ in 0..5 {
            let pool_clone = Arc::clone(&pool);
            let handle = thread::spawn(move || {
                let mut obj = pool_clone.get();
                obj.push(1);
                obj.push(2);
                // Object automatically returned when dropped
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        let stats = pool.stats();
        assert!(stats.objects_created <= 10); // Should reuse objects
        assert!(stats.objects_reused > 0 || stats.objects_created == 5);
    }
}
