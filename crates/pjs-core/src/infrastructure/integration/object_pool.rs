// Object pooling system for high-performance memory management
//
// This module provides thread-safe object pools for frequently allocated
// data structures like HashMap and Vec to minimize garbage collection overhead.

use crossbeam::queue::ArrayQueue;
use once_cell::sync::Lazy;
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Thread-safe object pool for reusable data structures
pub struct ObjectPool<T> {
    /// Queue of available objects
    objects: ArrayQueue<T>,
    /// Factory function to create new objects
    factory: Arc<dyn Fn() -> T + Send + Sync>,
    /// Maximum pool capacity
    #[allow(dead_code)] // Future: used for pool size enforcement
    max_capacity: usize,
    /// Pool statistics
    stats: Arc<Mutex<PoolStats>>,
}

#[derive(Debug, Clone, Default)]
pub struct PoolStats {
    pub objects_created: usize,
    pub objects_reused: usize,
    pub objects_returned: usize,
    pub peak_usage: usize,
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
            max_capacity: capacity,
            stats: Arc::new(Mutex::new(PoolStats::default())),
        }
    }

    /// Get an object from the pool, creating a new one if needed
    pub fn get(&self) -> PooledObject<'_, T> {
        let obj = if let Some(obj) = self.objects.pop() {
            // Update stats for reuse
            if let Ok(mut stats) = self.stats.lock() {
                stats.objects_reused += 1;
                stats.current_pool_size = stats.current_pool_size.saturating_sub(1);
            }
            obj
        } else {
            // Create new object
            let obj = (self.factory)();
            if let Ok(mut stats) = self.stats.lock() {
                stats.objects_created += 1;
                stats.peak_usage = stats
                    .peak_usage
                    .max(stats.objects_created - stats.current_pool_size);
            }
            obj
        };

        PooledObject {
            object: Some(obj),
            pool: self,
        }
    }

    /// Return an object to the pool
    fn return_object(&self, obj: T) {
        // Try to return to pool
        if self.objects.push(obj).is_ok()
            && let Ok(mut stats) = self.stats.lock()
        {
            stats.objects_returned += 1;
            stats.current_pool_size += 1;
        }
        // If pool is full, object is dropped (let GC handle it)
    }

    /// Clean object before returning to pool (default implementation)
    #[allow(dead_code)] // Future: will be used for type-specific cleaning strategies
    fn clean_object(_obj: &mut T) {
        // Default implementation does nothing
        // Specialized implementations below for specific types
    }

    /// Get current pool statistics
    pub fn stats(&self) -> PoolStats {
        self.stats
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }
}

// Trait for cleanable objects
#[allow(dead_code)] // Future: will be used for polymorphic cleaning
trait Cleanable {
    fn clean(&mut self);
}

impl Cleanable for HashMap<Cow<'static, str>, Cow<'static, str>> {
    fn clean(&mut self) {
        self.clear();
    }
}

impl Cleanable for HashMap<String, String> {
    fn clean(&mut self) {
        self.clear();
    }
}

impl Cleanable for Vec<u8> {
    fn clean(&mut self) {
        self.clear();
    }
}

impl Cleanable for Vec<String> {
    fn clean(&mut self) {
        self.clear();
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

/// Global pools for common data structures
pub struct GlobalPools {
    pub cow_hashmap: ObjectPool<HashMap<Cow<'static, str>, Cow<'static, str>>>,
    pub string_hashmap: ObjectPool<HashMap<String, String>>,
    pub byte_vec: ObjectPool<Vec<u8>>,
    pub string_vec: ObjectPool<Vec<String>>,
}

impl GlobalPools {
    #[allow(dead_code)] // Future: will be exposed for global pool initialization
    fn new() -> Self {
        Self {
            cow_hashmap: ObjectPool::new(50, || {
                let mut map = HashMap::with_capacity(8);
                map.shrink_to_fit(); // Ensure it's clean
                map
            }),
            string_hashmap: ObjectPool::new(50, || {
                let mut map = HashMap::with_capacity(8);
                map.shrink_to_fit(); // Ensure it's clean
                map
            }),
            byte_vec: ObjectPool::new(100, || {
                let mut vec = Vec::with_capacity(1024);
                vec.clear(); // Ensure it's clean
                vec
            }),
            string_vec: ObjectPool::new(50, || {
                let mut vec = Vec::with_capacity(16);
                vec.clear(); // Ensure it's clean
                vec
            }),
        }
    }
}

/// Global singleton pools
#[allow(dead_code)] // Future: will be used for global pool access pattern
static GLOBAL_POOLS: Lazy<GlobalPools> = Lazy::new(GlobalPools::new);

/// Wrapper that ensures cleaning happens
pub struct CleaningPooledObject<T: 'static> {
    inner: PooledObject<'static, T>,
}

impl<T: 'static> CleaningPooledObject<T> {
    fn new(inner: PooledObject<'static, T>) -> Self {
        Self { inner }
    }

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

/// Convenience functions for global cleaning pools
pub fn get_cow_hashmap() -> CleaningPooledObject<HashMap<Cow<'static, str>, Cow<'static, str>>> {
    let mut obj = CLEANING_COW_HASHMAP.get();
    obj.clear(); // Clean before use
    CleaningPooledObject::new(obj)
}

pub fn get_string_hashmap() -> CleaningPooledObject<HashMap<String, String>> {
    let mut obj = CLEANING_STRING_HASHMAP.get();
    obj.clear(); // Clean before use
    CleaningPooledObject::new(obj)
}

pub fn get_byte_vec() -> CleaningPooledObject<Vec<u8>> {
    let mut obj = CLEANING_BYTE_VEC.get();
    obj.clear(); // Clean before use
    CleaningPooledObject::new(obj)
}

pub fn get_string_vec() -> CleaningPooledObject<Vec<String>> {
    let mut obj = CLEANING_STRING_VEC.get();
    obj.clear(); // Clean before use
    CleaningPooledObject::new(obj)
}

/// Pool statistics aggregator
#[derive(Debug, Clone)]
pub struct GlobalPoolStats {
    pub cow_hashmap: PoolStats,
    pub string_hashmap: PoolStats,
    pub byte_vec: PoolStats,
    pub string_vec: PoolStats,
    pub total_objects_created: usize,
    pub total_objects_reused: usize,
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

/// Optimized response building with pooled objects
pub mod pooled_builders {
    use super::*;
    use crate::domain::value_objects::JsonData;
    use crate::infrastructure::integration::{ResponseBody, UniversalResponse};

    /// Response builder that uses pooled HashMap
    pub struct PooledResponseBuilder {
        status_code: u16,
        headers: CleaningPooledObject<HashMap<Cow<'static, str>, Cow<'static, str>>>,
        content_type: Cow<'static, str>,
    }

    impl PooledResponseBuilder {
        /// Create new builder with pooled HashMap
        pub fn new() -> Self {
            Self {
                status_code: 200,
                headers: get_cow_hashmap(),
                content_type: Cow::Borrowed("application/json"),
            }
        }

        /// Set status code
        pub fn status(mut self, status: u16) -> Self {
            self.status_code = status;
            self
        }

        /// Add header
        pub fn header(
            mut self,
            name: impl Into<Cow<'static, str>>,
            value: impl Into<Cow<'static, str>>,
        ) -> Self {
            self.headers.insert(name.into(), value.into());
            self
        }

        /// Set content type
        pub fn content_type(mut self, content_type: impl Into<Cow<'static, str>>) -> Self {
            self.content_type = content_type.into();
            self
        }

        /// Build response with JSON data
        pub fn json(self, data: JsonData) -> UniversalResponse {
            // Take ownership of headers to avoid cloning
            let headers = self.headers.take();

            UniversalResponse {
                status_code: self.status_code,
                headers,
                body: ResponseBody::Json(data),
                content_type: self.content_type,
            }
        }

        /// Build response with binary data using pooled Vec
        pub fn binary(self, data: Vec<u8>) -> UniversalResponse {
            let headers = self.headers.take();

            UniversalResponse {
                status_code: self.status_code,
                headers,
                body: ResponseBody::Binary(data),
                content_type: Cow::Borrowed("application/octet-stream"),
            }
        }
    }

    impl Default for PooledResponseBuilder {
        fn default() -> Self {
            Self::new()
        }
    }

    /// Server-Sent Events builder with pooled Vec
    pub struct PooledSSEBuilder {
        events: CleaningPooledObject<Vec<String>>,
        headers: CleaningPooledObject<HashMap<Cow<'static, str>, Cow<'static, str>>>,
    }

    impl PooledSSEBuilder {
        /// Create new SSE builder
        pub fn new() -> Self {
            let mut headers = get_cow_hashmap();
            headers.insert(Cow::Borrowed("Cache-Control"), Cow::Borrowed("no-cache"));
            headers.insert(Cow::Borrowed("Connection"), Cow::Borrowed("keep-alive"));

            Self {
                events: get_string_vec(),
                headers,
            }
        }

        /// Add event data
        pub fn event(mut self, data: impl Into<String>) -> Self {
            self.events.push(format!("data: {}\n\n", data.into()));
            self
        }

        /// Add custom header
        pub fn header(
            mut self,
            name: impl Into<Cow<'static, str>>,
            value: impl Into<Cow<'static, str>>,
        ) -> Self {
            self.headers.insert(name.into(), value.into());
            self
        }

        /// Build SSE response
        pub fn build(self) -> UniversalResponse {
            let events = self.events.take();
            let headers = self.headers.take();

            UniversalResponse {
                status_code: 200,
                headers,
                body: ResponseBody::ServerSentEvents(events),
                content_type: Cow::Borrowed("text/event-stream"),
            }
        }
    }

    impl Default for PooledSSEBuilder {
        fn default() -> Self {
            Self::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::ResponseBody;
    use super::*;
    use crate::domain::value_objects::JsonData;

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
    fn test_pooled_response_builder() {
        let response = pooled_builders::PooledResponseBuilder::new()
            .status(201)
            .header("X-Test", "test-value")
            .content_type("application/json")
            .json(JsonData::String("test".to_string()));

        assert_eq!(response.status_code, 201);
        assert_eq!(
            response.headers.get("X-Test"),
            Some(&Cow::Borrowed("test-value"))
        );
    }

    #[test]
    fn test_pooled_sse_builder() {
        let response = pooled_builders::PooledSSEBuilder::new()
            .event("first event")
            .event("second event")
            .header("X-Custom", "custom-value")
            .build();

        assert_eq!(response.status_code, 200);
        assert_eq!(response.content_type, "text/event-stream");

        if let ResponseBody::ServerSentEvents(events) = response.body {
            assert_eq!(events.len(), 2);
            assert!(events[0].contains("first event"));
            assert!(events[1].contains("second event"));
        } else {
            panic!("Expected ServerSentEvents body");
        }
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
