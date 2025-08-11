//! Schema-based compression demonstration
//!
//! Shows how PJS protocol can optimize bandwidth usage through intelligent
//! compression strategies based on JSON schema analysis.

use pjson_rs::{
    CompressionStrategy, SchemaAnalyzer, SchemaCompressor,
    Priority, StreamFrame, StreamingCompressor,
};
use serde_json::json;
use std::collections::HashMap;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🗜️  PJS Schema-Based Compression Demo");
    println!("=====================================\n");

    // Demo 1: Basic schema analysis
    demo_basic_schema_analysis()?;
    
    // Demo 2: Dictionary compression
    demo_dictionary_compression()?;
    
    // Demo 3: Streaming compression with priorities
    demo_streaming_compression()?;
    
    // Demo 4: Compression with real-world data patterns
    demo_realworld_patterns()?;

    Ok(())
}

fn demo_basic_schema_analysis() -> Result<(), Box<dyn std::error::Error>> {
    println!("1️⃣  Basic Schema Analysis");
    println!("-----------------------");

    let mut analyzer = SchemaAnalyzer::new();
    
    // Sample e-commerce data with repetitive patterns
    let sample_data = json!({
        "products": [
            {
                "id": 1001,
                "name": "MacBook Pro",
                "category": "Electronics",
                "status": "available",
                "brand": "Apple",
                "price": 2399.99
            },
            {
                "id": 1002, 
                "name": "iPhone 15",
                "category": "Electronics",
                "status": "available", 
                "brand": "Apple",
                "price": 999.99
            },
            {
                "id": 1003,
                "name": "AirPods Pro",
                "category": "Electronics", 
                "status": "available",
                "brand": "Apple",
                "price": 249.99
            }
        ],
        "store": {
            "name": "Tech Store",
            "status": "operational",
            "location": "San Francisco"
        }
    });

    let strategy = analyzer.analyze(&sample_data)?;
    
    println!("📊 Analyzed data structure:");
    println!("   - Products: {} items", sample_data["products"].as_array().unwrap().len());
    println!("   - Detected strategy: {:?}", strategy);
    
    match strategy {
        CompressionStrategy::Dictionary { ref dictionary } => {
            println!("   - Dictionary size: {} entries", dictionary.len());
            for (string, index) in dictionary.iter().take(3) {
                println!("     '{}' → {}", string, index);
            }
        }
        CompressionStrategy::Hybrid { ref string_dict, ref numeric_deltas } => {
            println!("   - String dictionary: {} entries", string_dict.len());
            println!("   - Numeric deltas: {} fields", numeric_deltas.len());
        }
        _ => println!("   - Strategy: {:?}", strategy),
    }

    let original_size = serde_json::to_string(&sample_data)?.len();
    println!("   - Original size: {} bytes\n", original_size);

    Ok(())
}

fn demo_dictionary_compression() -> Result<(), Box<dyn std::error::Error>> {
    println!("2️⃣  Dictionary Compression");
    println!("--------------------------");

    // Create compressor with manual dictionary for demonstration
    let mut dictionary = HashMap::new();
    dictionary.insert("available".to_string(), 0);
    dictionary.insert("Electronics".to_string(), 1);
    dictionary.insert("Apple".to_string(), 2);
    dictionary.insert("operational".to_string(), 3);

    let compressor = SchemaCompressor::with_strategy(
        CompressionStrategy::Dictionary { dictionary }
    );

    let data = json!({
        "product": {
            "name": "MacBook",
            "category": "Electronics",
            "brand": "Apple", 
            "status": "available"
        },
        "store_status": "operational"
    });

    let original_size = serde_json::to_string(&data)?.len();
    let compressed = compressor.compress(&data)?;

    println!("📦 Compression Results:");
    println!("   - Original size: {} bytes", original_size);
    println!("   - Compressed size: {} bytes", compressed.compressed_size);
    println!("   - Compression ratio: {:.2}", compressed.compression_ratio(original_size));
    println!("   - Bytes saved: {}", compressed.compression_savings(original_size));
    println!("   - Strategy: {:?}", compressed.strategy);
    
    println!("\n🔍 Compressed data preview:");
    println!("   {}", serde_json::to_string_pretty(&compressed.data)?);
    
    println!("\n📋 Metadata (for decompression):");
    for (key, value) in compressed.compression_metadata.iter().take(3) {
        println!("   {} → {}", key, value);
    }
    println!();

    Ok(())
}

fn demo_streaming_compression() -> Result<(), Box<dyn std::error::Error>> {
    println!("3️⃣  Streaming Compression with Priorities");
    println!("------------------------------------------");

    let mut streaming_compressor = StreamingCompressor::new();

    // Critical frame (error, status)
    let critical_frame = StreamFrame {
        data: json!({
            "error": "Authentication failed",
            "error_code": 401,
            "timestamp": "2024-01-15T10:30:00Z"
        }),
        priority: Priority::CRITICAL,
        metadata: HashMap::new(),
    };

    // High priority frame (user data)  
    let high_frame = StreamFrame {
        data: json!({
            "user": {
                "id": 12345,
                "name": "Alice Johnson", 
                "role": "admin",
                "status": "active"
            }
        }),
        priority: Priority::HIGH,
        metadata: HashMap::new(),
    };

    // Low priority frame (analytics)
    let low_frame = StreamFrame {
        data: json!({
            "analytics": {
                "page_views": 15420,
                "unique_visitors": 3241,
                "bounce_rate": 0.34,
                "session_duration": 245,
                "traffic_sources": {
                    "organic": 0.45,
                    "direct": 0.32, 
                    "referral": 0.23
                }
            }
        }),
        priority: Priority::LOW,
        metadata: HashMap::new(),
    };

    println!("🚀 Processing frames by priority:");
    
    let compressed_critical = streaming_compressor.compress_frame(critical_frame)?;
    println!("   ⚠️  CRITICAL: {} bytes → {} bytes", 
        serde_json::to_string(&compressed_critical.frame.data)?.len(),
        compressed_critical.compressed_data.compressed_size
    );

    let compressed_high = streaming_compressor.compress_frame(high_frame)?;
    println!("   🔥 HIGH: {} bytes → {} bytes",
        serde_json::to_string(&compressed_high.frame.data)?.len(), 
        compressed_high.compressed_data.compressed_size
    );

    let compressed_low = streaming_compressor.compress_frame(low_frame)?;
    println!("   📊 LOW: {} bytes → {} bytes",
        serde_json::to_string(&compressed_low.frame.data)?.len(),
        compressed_low.compressed_data.compressed_size
    );

    let stats = streaming_compressor.get_stats();
    println!("\n📈 Streaming Compression Stats:");
    println!("   - Frames processed: {}", stats.frames_processed);
    println!("   - Total input: {} bytes", stats.total_input_bytes);
    println!("   - Total output: {} bytes", stats.total_output_bytes);
    println!("   - Overall ratio: {:.3}", stats.overall_compression_ratio());
    println!("   - Bytes saved: {} ({:.1}%)", stats.bytes_saved(), stats.percentage_saved());

    if !stats.priority_ratios.is_empty() {
        println!("   - Priority ratios:");
        for (priority, ratio) in &stats.priority_ratios {
            let priority_name = match *priority {
                p if p == Priority::CRITICAL.value() => "CRITICAL",
                p if p == Priority::HIGH.value() => "HIGH",
                p if p == Priority::LOW.value() => "LOW",
                _ => "OTHER",
            };
            println!("     {} → {:.3}", priority_name, ratio);
        }
    }
    println!();

    Ok(())
}

fn demo_realworld_patterns() -> Result<(), Box<dyn std::error::Error>> {
    println!("4️⃣  Real-world Data Patterns");
    println!("----------------------------");

    let mut analyzer = SchemaAnalyzer::new();

    // Simulate API response with common patterns
    let api_response = json!({
        "status": "success",
        "data": {
            "users": [
                {
                    "id": "user_001",
                    "email": "alice@example.com",
                    "status": "active",
                    "role": "admin",
                    "created_at": "2024-01-01T00:00:00Z",
                    "last_login": "2024-01-15T10:30:00Z"
                },
                {
                    "id": "user_002", 
                    "email": "bob@example.com",
                    "status": "active",
                    "role": "user",
                    "created_at": "2024-01-02T00:00:00Z",
                    "last_login": "2024-01-15T09:15:00Z"
                },
                {
                    "id": "user_003",
                    "email": "charlie@example.com", 
                    "status": "inactive",
                    "role": "user",
                    "created_at": "2024-01-03T00:00:00Z",
                    "last_login": "2024-01-10T14:22:00Z"
                }
            ]
        },
        "pagination": {
            "page": 1,
            "per_page": 25,
            "total_pages": 4,
            "total_items": 89
        },
        "meta": {
            "request_id": "req_12345",
            "timestamp": "2024-01-15T10:30:15Z",
            "version": "v1.2.3"
        }
    });

    let strategy = analyzer.analyze(&api_response)?;
    let mut compressor = SchemaCompressor::new();
    let optimized_strategy = compressor.analyze_and_optimize(&api_response)?;
    let optimized_strategy = optimized_strategy.clone(); // Clone to avoid borrow checker issues

    let original_size = serde_json::to_string(&api_response)?.len();
    let compressed = compressor.compress(&api_response)?;

    println!("📡 API Response Analysis:");
    println!("   - Original size: {} bytes", original_size);
    println!("   - Users count: {}", api_response["data"]["users"].as_array().unwrap().len());
    println!("   - Detected strategy: {:?}", strategy);
    println!("   - Optimized strategy: {:?}", optimized_strategy);

    println!("\n🎯 Compression Results:");
    println!("   - Compressed size: {} bytes", compressed.compressed_size);
    println!("   - Compression ratio: {:.3}", compressed.compression_ratio(original_size));
    println!("   - Space savings: {} bytes ({:.1}%)", 
        compressed.compression_savings(original_size),
        (1.0 - compressed.compression_ratio(original_size)) * 100.0
    );

    // Estimate bandwidth savings for 1000 similar requests
    let estimated_original_mb = (original_size * 1000) as f64 / (1024.0 * 1024.0);
    let estimated_compressed_mb = (compressed.compressed_size * 1000) as f64 / (1024.0 * 1024.0);
    let bandwidth_saved_mb = estimated_original_mb - estimated_compressed_mb;

    println!("\n🌐 Bandwidth Impact (1000 requests):");
    println!("   - Without compression: {:.2} MB", estimated_original_mb);
    println!("   - With compression: {:.2} MB", estimated_compressed_mb);
    println!("   - Bandwidth saved: {:.2} MB ({:.1}%)", 
        bandwidth_saved_mb,
        (bandwidth_saved_mb / estimated_original_mb) * 100.0
    );

    println!("\n✨ PJS Advantages:");
    println!("   - Intelligent compression based on schema analysis");
    println!("   - Priority-aware compression for critical vs. non-critical data");
    println!("   - Streaming-compatible compression that doesn't block data flow");
    println!("   - Adaptive strategies that learn from data patterns");
    println!("   - Significant bandwidth savings for API-heavy applications");

    Ok(())
}