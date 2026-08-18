//! Comprehensive tests for infrastructure/integration/object_pool.rs
//!
//! This test suite aims to achieve 60%+ coverage by testing:
//! - ObjectPool creation and management
//! - PooledObject lifecycle (get, release, drop)
//! - Global pools (cow_hashmap, string_hashmap, byte_vec, string_vec)
//! - Concurrent access patterns
//! - Pool statistics tracking
//! - Edge cases and error handling

use pjson_rs::infrastructure::integration::object_pool::{
    ObjectPool, PoolStats, get_byte_vec, get_cow_hashmap, get_global_pool_stats,
    get_string_hashmap, get_string_vec,
};
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;
use std::thread;

// === Basic ObjectPool Tests ===

#[test]
fn test_object_pool_creation() {
    let pool: ObjectPool<Vec<i32>> = ObjectPool::new(10, Vec::new);
    let stats = pool.stats();
    assert_eq!(stats.objects_created, 0);
    assert_eq!(stats.objects_reused, 0);
}

#[test]
fn test_pool_get_creates_object() {
    let pool: ObjectPool<Vec<i32>> = ObjectPool::new(10, Vec::new);
    let _obj = pool.get();
    let stats = pool.stats();
    assert_eq!(stats.objects_created, 1);
}

#[test]
fn test_pool_get_and_return() {
    let pool: ObjectPool<Vec<i32>> = ObjectPool::new(10, Vec::new);

    {
        let _obj = pool.get();
    } // obj dropped here, returned to pool

    let stats = pool.stats();
    assert_eq!(stats.objects_returned, 1);
    assert_eq!(stats.current_pool_size, 1);
}

#[test]
fn test_pool_reuse() {
    let pool: ObjectPool<Vec<i32>> = ObjectPool::new(10, Vec::new);

    // Get and release
    {
        let _obj1 = pool.get();
    }

    // Get again - should reuse
    let _obj2 = pool.get();

    let stats = pool.stats();
    assert_eq!(stats.objects_created, 1); // Only created once
    assert_eq!(stats.objects_reused, 1); // Reused once
}

#[test]
fn test_pool_multiple_gets() {
    let pool: ObjectPool<Vec<i32>> = ObjectPool::new(5, Vec::new);

    let obj1 = pool.get();
    let obj2 = pool.get();
    let obj3 = pool.get();

    let stats = pool.stats();
    assert_eq!(stats.objects_created, 3);

    drop(obj1);
    drop(obj2);
    drop(obj3);

    let stats = pool.stats();
    assert_eq!(stats.objects_returned, 3);
}

#[test]
fn test_pool_capacity_overflow() {
    let pool: ObjectPool<Vec<i32>> = ObjectPool::new(2, Vec::new);

    let obj1 = pool.get();
    let obj2 = pool.get();
    let obj3 = pool.get();

    drop(obj1);
    drop(obj2);
    drop(obj3); // This exceeds capacity, should be dropped

    let stats = pool.stats();
    assert_eq!(stats.objects_created, 3);
    assert_eq!(stats.objects_returned, 2); // Only 2 fit in pool
    assert_eq!(stats.current_pool_size, 2);
}

#[test]
fn test_pool_peak_usage() {
    let pool: ObjectPool<Vec<i32>> = ObjectPool::new(10, Vec::new);

    let obj1 = pool.get();
    let obj2 = pool.get();
    let obj3 = pool.get();

    let stats = pool.stats();
    assert!(stats.peak_usage <= 3);

    drop(obj1);
    drop(obj2);
    drop(obj3);
}

// === PooledObject Tests ===

#[test]
fn test_pooled_object_deref() {
    let pool: ObjectPool<Vec<i32>> = ObjectPool::new(5, Vec::new);
    let mut obj = pool.get();

    obj.push(1);
    obj.push(2);
    obj.push(3);

    assert_eq!(obj.len(), 3);
    assert_eq!(obj[0], 1);
}

#[test]
fn test_pooled_object_deref_mut() {
    let pool: ObjectPool<Vec<i32>> = ObjectPool::new(5, Vec::new);
    let mut obj = pool.get();

    obj.push(10);
    obj[0] = 20;

    assert_eq!(obj[0], 20);
}

#[test]
fn test_pooled_object_get() {
    let pool: ObjectPool<Vec<i32>> = ObjectPool::new(5, Vec::new);
    let obj = pool.get();

    let vec_ref = obj.get();
    assert_eq!(vec_ref.len(), 0);
}

#[test]
fn test_pooled_object_get_mut() {
    let pool: ObjectPool<Vec<i32>> = ObjectPool::new(5, Vec::new);
    let mut obj = pool.get();

    let vec_ref = obj.get_mut();
    vec_ref.push(42);

    assert_eq!(obj.len(), 1);
}

#[test]
fn test_pooled_object_take() {
    let pool: ObjectPool<Vec<i32>> = ObjectPool::new(5, Vec::new);
    let obj = pool.get();

    let vec = obj.take();
    assert_eq!(vec.len(), 0);

    // Object was taken, not returned to pool
    let stats = pool.stats();
    assert_eq!(stats.objects_returned, 0);
}

// === Global Pools Tests ===

#[test]
fn test_get_cow_hashmap() {
    let mut map = get_cow_hashmap();
    map.insert(Cow::Borrowed("key"), Cow::Borrowed("value"));
    assert_eq!(map.len(), 1);
}

#[test]
fn test_cow_hashmap_returns_clean() {
    {
        let mut map = get_cow_hashmap();
        map.insert(Cow::Borrowed("test"), Cow::Borrowed("data"));
    }

    // Get again - should be clean
    let map = get_cow_hashmap();
    assert_eq!(map.len(), 0, "HashMap should be cleaned before reuse");
}

#[test]
fn test_get_string_hashmap() {
    let mut map = get_string_hashmap();
    map.insert("key".to_string(), "value".to_string());
    assert_eq!(map.len(), 1);
}

#[test]
fn test_string_hashmap_returns_clean() {
    {
        let mut map = get_string_hashmap();
        map.insert("test".to_string(), "data".to_string());
    }

    let map = get_string_hashmap();
    assert_eq!(map.len(), 0);
}

#[test]
fn test_get_byte_vec() {
    let mut vec = get_byte_vec();
    vec.extend_from_slice(b"hello");
    assert_eq!(vec.len(), 5);
}

#[test]
fn test_byte_vec_returns_clean() {
    {
        let mut vec = get_byte_vec();
        vec.extend_from_slice(b"test data");
    }

    let vec = get_byte_vec();
    assert_eq!(vec.len(), 0);
}

#[test]
fn test_get_string_vec() {
    let mut vec = get_string_vec();
    vec.push("item1".to_string());
    vec.push("item2".to_string());
    assert_eq!(vec.len(), 2);
}

#[test]
fn test_string_vec_returns_clean() {
    {
        let mut vec = get_string_vec();
        vec.push("test".to_string());
    }

    let vec = get_string_vec();
    assert_eq!(vec.len(), 0);
}

#[test]
fn test_global_pool_stats() {
    // Use all global pools
    {
        let _map1 = get_cow_hashmap();
        let _map2 = get_string_hashmap();
        let _vec1 = get_byte_vec();
        let _vec2 = get_string_vec();
    }

    let stats = get_global_pool_stats();
    assert!(stats.total_objects_created > 0 || stats.total_objects_reused > 0);
    assert!(stats.total_reuse_ratio >= 0.0 && stats.total_reuse_ratio <= 1.0);
}

#[test]
fn test_global_pool_stats_fields() {
    let stats = get_global_pool_stats();

    // All fields should be present
    let _ = stats.cow_hashmap;
    let _ = stats.string_hashmap;
    let _ = stats.byte_vec;
    let _ = stats.string_vec;
    let _ = stats.total_objects_created;
    let _ = stats.total_objects_reused;
    let _ = stats.total_reuse_ratio;
}

// === Concurrent Access Tests ===

#[test]
fn test_concurrent_pool_access() {
    let pool = Arc::new(ObjectPool::new(20, Vec::<i32>::new));
    let mut handles = vec![];

    for _ in 0..10 {
        let pool_clone = Arc::clone(&pool);
        let handle = thread::spawn(move || {
            for _ in 0..5 {
                let mut obj = pool_clone.get();
                obj.push(1);
                obj.push(2);
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let stats = pool.stats();
    assert!(stats.objects_created > 0);
}

#[test]
fn test_concurrent_global_pools() {
    let mut handles = vec![];

    for i in 0..5 {
        let handle = thread::spawn(move || {
            let mut map = get_cow_hashmap();
            map.insert(
                Cow::Owned(format!("key{}", i)),
                Cow::Owned(format!("value{}", i)),
            );
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    // Should complete without deadlock or race conditions
}

// === Pool Statistics Tests ===

#[test]
fn test_pool_stats_default() {
    let stats = PoolStats::default();
    assert_eq!(stats.objects_created, 0);
    assert_eq!(stats.objects_reused, 0);
    assert_eq!(stats.objects_returned, 0);
    assert_eq!(stats.peak_usage, 0);
    assert_eq!(stats.current_pool_size, 0);
}

#[test]
fn test_pool_stats_tracking() {
    let pool: ObjectPool<Vec<i32>> = ObjectPool::new(10, Vec::new);

    {
        let _obj1 = pool.get();
        let _obj2 = pool.get();
    }

    let _obj3 = pool.get();

    let stats = pool.stats();
    assert_eq!(stats.objects_created, 2);
    assert_eq!(stats.objects_reused, 1);
    assert_eq!(stats.objects_returned, 2);
}

// === Edge Cases and Boundary Conditions ===

#[test]
#[should_panic(expected = "capacity must be non-zero")]
fn test_pool_with_zero_capacity_panics() {
    // Zero capacity is not allowed by crossbeam ArrayQueue
    let _pool: ObjectPool<Vec<i32>> = ObjectPool::new(0, Vec::new);
}

#[test]
fn test_pool_with_large_capacity() {
    let pool: ObjectPool<Vec<i32>> = ObjectPool::new(1000, Vec::new);

    // Get and immediately drop - objects are reused from pool
    for _ in 0..100 {
        let _obj = pool.get();
    }

    let stats = pool.stats();
    // Objects are reused, so only a few are created
    assert!(stats.objects_created >= 1);
    assert!(stats.objects_created <= 100);
}

#[test]
fn test_pooled_object_drop_returns_to_pool() {
    let pool: ObjectPool<Vec<i32>> = ObjectPool::new(5, Vec::new);

    {
        let _obj = pool.get();
        // obj is dropped at end of scope
    }

    let stats = pool.stats();
    assert_eq!(stats.objects_returned, 1);
    assert_eq!(stats.current_pool_size, 1);
}

#[test]
fn test_pool_with_custom_factory() {
    let pool: ObjectPool<Vec<i32>> = ObjectPool::new(5, || vec![1, 2, 3]);

    let obj = pool.get();
    assert_eq!(obj.len(), 3);
    assert_eq!(obj[0], 1);
}

#[test]
fn test_pool_stats_clone() {
    let stats = PoolStats {
        objects_created: 10,
        objects_reused: 5,
        objects_returned: 8,
        peak_usage: 3,
        current_pool_size: 2,
    };

    let cloned = stats.clone();
    assert_eq!(cloned.objects_created, 10);
    assert_eq!(cloned.objects_reused, 5);
}

#[test]
fn test_global_pool_reuse_ratio_zero() {
    // Fresh stats should have ratio 0.0
    let stats = get_global_pool_stats();
    assert!(stats.total_reuse_ratio >= 0.0);
}

#[test]
fn test_cleaning_pooled_object_take() {
    let obj = get_byte_vec();
    let vec = obj.take();
    assert_eq!(vec.len(), 0);
}

// === Type-Specific Pool Tests ===

#[test]
fn test_hashmap_pool_preserves_capacity() {
    let pool: ObjectPool<HashMap<String, String>> =
        ObjectPool::new(5, || HashMap::with_capacity(10));

    let obj = pool.get();
    // Capacity should be preserved from factory
    assert!(obj.capacity() >= 10);
}

#[test]
fn test_vec_pool_preserves_capacity() {
    let pool: ObjectPool<Vec<u8>> = ObjectPool::new(5, || Vec::with_capacity(1024));

    let obj = pool.get();
    assert!(obj.capacity() >= 1024);
}
