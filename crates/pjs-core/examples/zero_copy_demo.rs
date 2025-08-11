//! Demonstration of zero-copy JSON parsing capabilities
//!
//! This example shows how the zero-copy parser can parse JSON with minimal
//! memory allocations, providing better performance for large documents.

use pjson_rs::parser::{
    ZeroCopyParser, LazyParser, LazyJsonValue,
    SimdZeroCopyParser, SimdZeroCopyConfig,
    BufferSize, global_buffer_pool,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 PJS Zero-Copy JSON Parser Demo");
    println!("=====================================\n");

    // Demo 1: Basic zero-copy string parsing
    demo_basic_zero_copy()?;
    
    // Demo 2: Memory efficiency comparison
    demo_memory_efficiency()?;
    
    // Demo 3: SIMD accelerated parsing
    demo_simd_parsing()?;
    
    // Demo 4: Buffer pool usage
    demo_buffer_pool()?;
    
    // Demo 5: Performance with large JSON
    demo_large_json_performance()?;

    println!("\n✅ All demos completed successfully!");
    Ok(())
}

fn demo_basic_zero_copy() -> Result<(), Box<dyn std::error::Error>> {
    println!("📝 Demo 1: Basic Zero-Copy Parsing");
    println!("----------------------------------");
    
    let mut parser = ZeroCopyParser::new();
    
    // Parse a simple string - should be zero-copy
    let json_string = br#""Hello, zero-copy world!""#;
    let result = parser.parse_lazy(json_string)?;
    
    match result {
        LazyJsonValue::StringBorrowed(bytes) => {
            let text = std::str::from_utf8(bytes)?;
            println!("✓ Parsed string: {}", text);
            
            let memory = result.memory_usage();
            println!("  Memory efficiency: {:.1}% (allocated: {}, referenced: {})",
                memory.efficiency() * 100.0,
                memory.allocated_bytes,
                memory.referenced_bytes
            );
        }
        _ => println!("❌ Expected borrowed string"),
    }
    
    parser.reset();
    
    // Parse a number - also zero-copy
    let json_number = b"123.456";
    let result = parser.parse_lazy(json_number)?;
    
    match result {
        LazyJsonValue::NumberSlice(bytes) => {
            let text = std::str::from_utf8(bytes)?;
            println!("✓ Parsed number: {}", text);
            
            let memory = result.memory_usage();
            println!("  Memory efficiency: {:.1}%", memory.efficiency() * 100.0);
        }
        _ => println!("❌ Expected number slice"),
    }
    
    println!();
    Ok(())
}

fn demo_memory_efficiency() -> Result<(), Box<dyn std::error::Error>> {
    println!("📊 Demo 2: Memory Efficiency Analysis");
    println!("-------------------------------------");
    
    let test_cases = vec![
        ("Simple string", br#""test string""# as &[u8]),
        ("Number", b"42"),
        ("Boolean", b"true"),
        ("Null", b"null"),
        ("Small object", br#"{"key": "value"}"#),
        ("Small array", b"[1, 2, 3]"),
        ("Escaped string", br#""with \"quotes\" and \\backslashes""#),
    ];
    
    let mut parser = ZeroCopyParser::new();
    let mut total_efficiency = 0.0;
    
    for (name, input) in test_cases {
        let result = parser.parse_lazy(input)?;
        let memory = result.memory_usage();
        let efficiency = memory.efficiency();
        
        println!("  {:<15}: {:.1}% efficient (alloc: {:3}, ref: {:3})",
            name,
            efficiency * 100.0,
            memory.allocated_bytes,
            memory.referenced_bytes
        );
        
        total_efficiency += efficiency;
        parser.reset();
    }
    
    println!("  Average efficiency: {:.1}%", (total_efficiency / 7.0) * 100.0);
    println!();
    Ok(())
}

fn demo_simd_parsing() -> Result<(), Box<dyn std::error::Error>> {
    println!("⚡ Demo 3: SIMD Accelerated Parsing");
    println!("----------------------------------");
    
    let json_data = br#"{
        "users": [
            {"id": 1, "name": "Alice", "active": true},
            {"id": 2, "name": "Bob", "active": false},
            {"id": 3, "name": "Charlie", "active": true}
        ],
        "metadata": {
            "count": 3,
            "generated": "2025-01-01T00:00:00Z"
        }
    }"#;
    
    // Test different SIMD configurations
    let configs = vec![
        ("Default", SimdZeroCopyConfig::default()),
        ("High Performance", SimdZeroCopyConfig::high_performance()),
        ("Low Memory", SimdZeroCopyConfig::low_memory()),
    ];
    
    for (name, config) in configs {
        let mut parser = SimdZeroCopyParser::with_config(config);
        let start = std::time::Instant::now();
        let result = parser.parse_simd(json_data)?;
        let _duration = start.elapsed();
        
        println!("  {:<15}: {:>6.0}ns, {:.1}% efficient, SIMD: {}",
            name,
            result.processing_time_ns,
            result.memory_usage.efficiency() * 100.0,
            if result.simd_used { "✓" } else { "✗" }
        );
    }
    
    println!();
    Ok(())
}

fn demo_buffer_pool() -> Result<(), Box<dyn std::error::Error>> {
    println!("🏊 Demo 4: Buffer Pool Management");
    println!("---------------------------------");
    
    let pool = global_buffer_pool();
    
    // Get buffers of different sizes
    let buffer_sizes = vec![
        BufferSize::Small,
        BufferSize::Medium,
        BufferSize::Large,
    ];
    
    for size in buffer_sizes {
        let buffer = pool.get_buffer(size)?;
        println!("  {:?}: {} bytes capacity", size, buffer.capacity());
        // Buffer is automatically returned to pool when dropped
    }
    
    // Check pool statistics
    let stats = pool.stats()?;
    println!("  Pool stats: {} allocations, {:.1}% hit rate",
        stats.total_allocations,
        stats.hit_ratio() * 100.0
    );
    
    println!();
    Ok(())
}

fn demo_large_json_performance() -> Result<(), Box<dyn std::error::Error>> {
    println!("🏃 Demo 5: Large JSON Performance");
    println!("---------------------------------");
    
    // Generate a moderately large JSON document
    let large_json = generate_test_json(1000);
    let json_bytes = large_json.as_bytes();
    
    println!("  JSON size: {:.1} KB", json_bytes.len() as f64 / 1024.0);
    
    // Test zero-copy parser
    let mut zero_copy = ZeroCopyParser::new();
    let start = std::time::Instant::now();
    let result = zero_copy.parse_lazy(json_bytes)?;
    let zero_copy_time = start.elapsed();
    
    let memory = result.memory_usage();
    println!("  Zero-copy: {:>6.2}ms, {:.1}% efficient",
        zero_copy_time.as_secs_f64() * 1000.0,
        memory.efficiency() * 100.0
    );
    
    // Test SIMD parser
    let mut simd_parser = SimdZeroCopyParser::with_config(
        SimdZeroCopyConfig::high_performance()
    );
    let start = std::time::Instant::now();
    let simd_result = simd_parser.parse_simd(json_bytes)?;
    let simd_time = start.elapsed();
    
    println!("  SIMD:      {:>6.2}ms, {:.1}% efficient, SIMD used: {}",
        simd_time.as_secs_f64() * 1000.0,
        simd_result.memory_usage.efficiency() * 100.0,
        if simd_result.simd_used { "✓" } else { "✗" }
    );
    
    // Calculate throughput
    let throughput_mb_s = (json_bytes.len() as f64) / (1024.0 * 1024.0) / 
        zero_copy_time.as_secs_f64();
    println!("  Throughput: {:.1} MB/s", throughput_mb_s);
    
    println!();
    Ok(())
}

fn generate_test_json(items: usize) -> String {
    let mut json = String::from(r#"{"data": {"items": ["#);
    
    for i in 0..items {
        if i > 0 {
            json.push(',');
        }
        json.push_str(&format!(
            r#"{{"id": {}, "name": "item_{}", "value": {}, "active": {}, "metadata": {{"created": "2025-01-01", "priority": {}}}}}"#,
            i,
            i,
            i * 10,
            i % 2 == 0,
            i % 5
        ));
    }
    
    json.push_str(r#"], "summary": {"count": "#);
    json.push_str(&items.to_string());
    json.push_str(r#", "version": "1.0"}}}"#);
    
    json
}