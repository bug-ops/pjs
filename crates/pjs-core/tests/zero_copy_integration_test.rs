//! Integration tests for zero-copy parser implementation
//!
//! These tests verify that the zero-copy parser works correctly with
//! the rest of the PJS system and provides the expected performance benefits.

use pjson_rs::{
    parser::{
        ZeroCopyParser, LazyParser, LazyJsonValue,
        SimdZeroCopyParser, SimdZeroCopyConfig, BufferSize,
        global_buffer_pool,
    },
};

#[tokio::test]
async fn test_zero_copy_parser_basic() {
    let mut parser = ZeroCopyParser::new();
    let input = br#"{"name": "test", "value": 42, "active": true}"#;
    
    let result = parser.parse_lazy(input).unwrap();
    match result {
        LazyJsonValue::ObjectSlice(bytes) => {
            assert_eq!(bytes.len(), input.len());
            assert_eq!(bytes, input);
        }
        _ => panic!("Expected object slice"),
    }
}

#[tokio::test] 
async fn test_zero_copy_string_parsing() {
    let mut parser = ZeroCopyParser::new();
    let input = br#""hello world""#;
    
    let result = parser.parse_lazy(input).unwrap();
    match result {
        LazyJsonValue::StringBorrowed(bytes) => {
            assert_eq!(bytes, b"hello world");
            let memory_usage = result.memory_usage();
            assert_eq!(memory_usage.allocated_bytes, 0); // Zero-copy!
            assert_eq!(memory_usage.referenced_bytes, 11);
            assert_eq!(memory_usage.efficiency(), 1.0); // Perfect efficiency
        }
        _ => panic!("Expected borrowed string"),
    }
}

#[tokio::test]
async fn test_escaped_string_allocation() {
    let mut parser = ZeroCopyParser::new();
    let input = br#""hello \"world\"!""#;
    
    let result = parser.parse_lazy(input).unwrap();
    match result {
        LazyJsonValue::StringOwned(ref s) => {
            assert_eq!(s, r#"hello "world"!"#);
            let memory_usage = result.memory_usage();
            assert!(memory_usage.allocated_bytes > 0); // Required allocation
            assert_eq!(memory_usage.referenced_bytes, 0);
            assert_eq!(memory_usage.efficiency(), 0.0); // No zero-copy possible
        }
        _ => panic!("Expected owned string due to escapes"),
    }
}

#[tokio::test]
async fn test_simd_zero_copy_parser() {
    let mut parser = SimdZeroCopyParser::new();
    let input = br#"{"numbers": [1, 2, 3, 4, 5], "text": "sample"}"#;
    
    let result = parser.parse_simd(input).unwrap();
    
    // Check that we got a valid result
    match result.value {
        LazyJsonValue::ObjectSlice(bytes) => {
            assert_eq!(bytes.len(), input.len());
        }
        _ => panic!("Expected object slice"),
    }
    
    // Check memory efficiency
    assert!(result.memory_usage.efficiency() > 0.8); // Should be mostly zero-copy
    
    // Check processing time is reasonable
    assert!(result.processing_time_ns > 0);
    assert!(result.processing_time_ns < 1_000_000); // Less than 1ms
}

#[tokio::test]
async fn test_buffer_pool_integration() {
    let pool = global_buffer_pool();
    
    // Get a buffer from the pool
    let buffer = pool.get_buffer(BufferSize::Medium).unwrap();
    assert!(buffer.capacity() >= BufferSize::Medium as usize);
    
    // Check pool statistics
    let stats = pool.stats().unwrap();
    assert!(stats.total_allocations > 0);
}

#[tokio::test]
async fn test_memory_efficiency_comparison() {
    // Test data that should demonstrate zero-copy benefits
    let test_cases = vec![
        br#""simple string""# as &[u8],
        br#"123.456"#,
        br#"true"#,
        br#"null"#,
        br#"{"key": "value"}"#,
        br#"[1, 2, 3, 4, 5]"#,
    ];
    
    let mut total_zero_copy_efficiency = 0.0;
    let mut test_count = 0;
    
    for test_input in test_cases {
        let mut parser = ZeroCopyParser::new();
        let result = parser.parse_lazy(test_input).unwrap();
        
        let memory_usage = result.memory_usage();
        let efficiency = memory_usage.efficiency();
        
        println!(
            "Input: {:?}, Efficiency: {:.2}, Allocated: {}, Referenced: {}",
            std::str::from_utf8(test_input).unwrap_or("<invalid utf8>"),
            efficiency,
            memory_usage.allocated_bytes,
            memory_usage.referenced_bytes
        );
        
        total_zero_copy_efficiency += efficiency;
        test_count += 1;
        
        // Basic sanity checks
        assert!(memory_usage.total() > 0);
        assert!(efficiency >= 0.0 && efficiency <= 1.0);
    }
    
    let average_efficiency = total_zero_copy_efficiency / test_count as f64;
    println!("Average zero-copy efficiency: {:.2}", average_efficiency);
    
    // We expect high efficiency for simple cases
    assert!(average_efficiency > 0.7, "Zero-copy efficiency should be > 70%");
}

#[tokio::test]
async fn test_parser_performance_comparison() {
    let large_json = generate_large_test_json(1000);
    let input_bytes = large_json.as_bytes();
    
    // Test zero-copy parser
    let start = std::time::Instant::now();
    let mut zero_copy_parser = ZeroCopyParser::new();
    let zero_copy_result = zero_copy_parser.parse_lazy(input_bytes).unwrap();
    let zero_copy_time = start.elapsed();
    
    // Test SIMD zero-copy parser
    let start = std::time::Instant::now();
    let mut simd_parser = SimdZeroCopyParser::new();
    let simd_result = simd_parser.parse_simd(input_bytes).unwrap();
    let simd_time = start.elapsed();
    
    println!("Zero-copy parser: {:?}", zero_copy_time);
    println!("SIMD zero-copy parser: {:?}", simd_time);
    println!("SIMD processing time: {}ns", simd_result.processing_time_ns);
    
    // Both should complete in reasonable time
    assert!(zero_copy_time.as_millis() < 100); // Less than 100ms
    assert!(simd_time.as_millis() < 100);
    
    // Check memory efficiency
    let zero_copy_efficiency = zero_copy_result.memory_usage();
    let simd_efficiency = simd_result.memory_usage;
    
    assert!(zero_copy_efficiency.efficiency() > 0.5);
    assert!(simd_efficiency.efficiency() > 0.5);
}

#[tokio::test]
async fn test_parser_reuse() {
    let mut parser = ZeroCopyParser::new();
    
    let inputs = vec![
        br#""first string""# as &[u8],
        br#"42"#,
        br#"{"key": "value"}"#,
        br#"[1, 2, 3]"#,
    ];
    
    for input in inputs {
        let result = parser.parse_lazy(input).unwrap();
        
        // Verify we got a valid result
        match result {
            LazyJsonValue::StringBorrowed(_) | 
            LazyJsonValue::StringOwned(_) |
            LazyJsonValue::NumberSlice(_) |
            LazyJsonValue::ObjectSlice(_) |
            LazyJsonValue::ArraySlice(_) => {
                // All good
            }
            _ => panic!("Unexpected result type"),
        }
        
        // Reset for next use
        parser.reset();
        assert!(parser.is_complete());
    }
}

#[tokio::test]
async fn test_incremental_parsing() {
    // TODO: This would test the IncrementalParser when fully implemented
    // For now, just verify it can be created
    let _parser = pjson_rs::parser::IncrementalParser::new();
}

#[tokio::test]
async fn test_config_variations() {
    // Test different configurations of SIMD parser
    let configs = vec![
        SimdZeroCopyConfig::default(),
        SimdZeroCopyConfig::high_performance(),
        SimdZeroCopyConfig::low_memory(),
    ];
    
    let input = br#"{"test": "configuration", "number": 123}"#;
    
    for config in configs {
        let mut parser = SimdZeroCopyParser::with_config(config);
        let result = parser.parse_simd(input).unwrap();
        
        // All configurations should produce valid results
        match result.value {
            LazyJsonValue::ObjectSlice(_) => {
                // Expected
            }
            _ => panic!("Expected object slice"),
        }
        
        assert!(result.processing_time_ns > 0);
    }
}

// Helper function to generate test data
fn generate_large_test_json(size: usize) -> String {
    let mut json = String::from("{\"items\": [");
    
    for i in 0..size {
        if i > 0 {
            json.push(',');
        }
        json.push_str(&format!(
            r#"{{"id": {}, "name": "item_{}", "value": {}, "active": {}}}"#,
            i,
            i,
            i * 10,
            i % 2 == 0
        ));
    }
    
    json.push_str("], \"metadata\": {\"count\": ");
    json.push_str(&size.to_string());
    json.push_str(", \"generated\": true}}");
    
    json
}