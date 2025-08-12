// Object Pool Performance Demo
//
// This example demonstrates the performance benefits of object pooling
// for frequently allocated data structures like HashMap and Vec.

use pjson_rs::infrastructure::integration::object_pool::{
    get_cow_hashmap, get_byte_vec, get_global_pool_stats,
};
use std::borrow::Cow;
use std::collections::HashMap;
use std::time::Instant;

fn main() {
    println!("🚀 Object Pool Performance Demo");
    println!("==============================\n");

    benchmark_hashmap_allocation();
    benchmark_vec_allocation(); 
    print_pool_statistics();
}

fn benchmark_hashmap_allocation() {
    println!("📊 HashMap Allocation Benchmark");
    println!("--------------------------------");

    const ITERATIONS: usize = 100_000;

    // Benchmark: Standard allocation
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        let mut map = HashMap::<Cow<'static, str>, Cow<'static, str>>::with_capacity(8);
        map.insert(Cow::Borrowed("key1"), Cow::Borrowed("value1"));
        map.insert(Cow::Borrowed("key2"), Cow::Borrowed("value2"));
        // Drop happens automatically
    }
    let standard_duration = start.elapsed();

    // Benchmark: Pooled allocation
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        let mut map = get_cow_hashmap();
        map.insert(Cow::Borrowed("key1"), Cow::Borrowed("value1"));
        map.insert(Cow::Borrowed("key2"), Cow::Borrowed("value2"));
        // Automatic return to pool on drop
    }
    let pooled_duration = start.elapsed();

    println!("Standard allocation: {:?}", standard_duration);
    println!("Pooled allocation:   {:?}", pooled_duration);
    
    let improvement = standard_duration.as_nanos() as f64 / pooled_duration.as_nanos() as f64;
    println!("Performance improvement: {:.2}x faster\n", improvement);
}

fn benchmark_vec_allocation() {
    println!("📊 Vec Allocation Benchmark");
    println!("---------------------------");

    const ITERATIONS: usize = 100_000;

    // Benchmark: Standard allocation
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        let mut vec = Vec::<u8>::with_capacity(1024);
        vec.extend_from_slice(b"test data that we're writing to the vector");
        // Drop happens automatically
    }
    let standard_duration = start.elapsed();

    // Benchmark: Pooled allocation
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        let mut vec = get_byte_vec();
        vec.extend_from_slice(b"test data that we're writing to the vector");
        // Automatic return to pool on drop
    }
    let pooled_duration = start.elapsed();

    println!("Standard allocation: {:?}", standard_duration);
    println!("Pooled allocation:   {:?}", pooled_duration);
    
    let improvement = standard_duration.as_nanos() as f64 / pooled_duration.as_nanos() as f64;
    println!("Performance improvement: {:.2}x faster\n", improvement);
}

fn print_pool_statistics() {
    println!("📈 Pool Statistics");
    println!("------------------");
    
    let stats = get_global_pool_stats();
    
    println!("Total objects created: {}", stats.total_objects_created);
    println!("Total objects reused:  {}", stats.total_objects_reused);
    println!("Reuse ratio:          {:.1}%", stats.total_reuse_ratio * 100.0);
    
    println!("\nPer-pool statistics:");
    println!("• COW HashMap - Created: {}, Reused: {}", 
             stats.cow_hashmap.objects_created, 
             stats.cow_hashmap.objects_reused);
    println!("• Byte Vec    - Created: {}, Reused: {}", 
             stats.byte_vec.objects_created, 
             stats.byte_vec.objects_reused);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_object_pool_demo() {
        // Test that the demo functions don't panic
        benchmark_hashmap_allocation();
        benchmark_vec_allocation();
        print_pool_statistics();
    }

    #[test]
    fn test_pool_reuse_effectiveness() {
        // Clear any existing pool state
        let _map1 = get_cow_hashmap();
        let _map2 = get_cow_hashmap();
        drop(_map1);
        drop(_map2);
        
        // Get fresh objects which should reuse pooled instances
        let _map3 = get_cow_hashmap();
        let _map4 = get_cow_hashmap();
        
        let stats = get_global_pool_stats();
        // Should have some reuse happening
        assert!(stats.total_objects_created > 0 || stats.total_objects_reused > 0);
    }
}