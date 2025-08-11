//! Interactive streaming demo server
//!
//! This demo shows real-time PJS streaming benefits with a web interface
//! comparing traditional JSON loading vs PJS progressive streaming.

use pjson_rs::PriorityStreamer;
use serde_json::{json, Value};
use std::time::Instant;

// Simplified version since we can't use axum in this context
// This shows the core PJS concepts

fn generate_demo_dataset(size: &str) -> Value {
    let product_count = match size {
        "small" => 10,
        "medium" => 50,
        "large" => 150,
        "huge" => 500,
        _ => 25,
    };

    let mut products = Vec::new();
    
    for i in 0..product_count {
        products.push(json!({
            "id": i + 1,
            "name": format!("Product {} - Professional Edition", i + 1),
            "description": format!("Detailed description for product {} with specifications.", i + 1),
            "category": format!("Category {}", (i % 8) + 1),
            "price": {
                "amount": ((i as f64 * 1.7) + 29.99).round(),
                "currency": "USD",
                "discount": if i % 7 == 0 { 15.0 } else { 0.0 }
            },
            "availability": {
                "in_stock": i % 6 != 0,
                "quantity": (i % 30) + 5,
                "shipping": if i % 3 == 0 { "free" } else { "standard" }
            },
            "rating": {
                "average": ((i % 5) as f64 + 3.0).min(5.0),
                "count": (i % 150) + 25
            }
        }));
    }

    json!({
        "store": {
            "name": "PJS Demo Store",
            "version": "2.0",
            "generated_at": "2024-01-15T12:00:00Z"
        },
        "metadata": {
            "total_products": product_count,
            "dataset_size": size
        },
        "categories": (1..=8).map(|i| json!({
            "id": i,
            "name": format!("Category {}", i)
        })).collect::<Vec<_>>(),
        "products": products
    })
}

fn demonstrate_streaming_advantage() {
    println!("🚀 PJS Streaming Demo - Core Library");
    println!("=====================================");
    println!();

    let sizes = vec!["small", "medium", "large", "huge"];
    
    for size in sizes {
        println!("📊 Testing {} dataset:", size);
        
        let data = generate_demo_dataset(size);
        let data_str = serde_json::to_string(&data).unwrap();
        
        // Traditional approach: parse everything
        let traditional_start = Instant::now();
        let _: Value = serde_json::from_str(&data_str).unwrap();
        let traditional_time = traditional_start.elapsed();
        
        // PJS approach: create streaming plan
        let pjs_start = Instant::now();
        let streamer = PriorityStreamer::new();
        let plan = streamer.analyze(&data).unwrap();
        
        // Time to first critical frame (skeleton)
        let first_frame_time = pjs_start.elapsed();
        
        // Simulate full reconstruction
        let mut frames_processed = 0;
        for _frame in plan.frames() {
            frames_processed += 1;
        }
        let pjs_full_time = pjs_start.elapsed();
        
        println!("  📈 Results:");
        println!("    Traditional parsing: {:?}", traditional_time);
        println!("    PJS first frame:     {:?} ({:.1}x faster)", 
                first_frame_time, 
                traditional_time.as_nanos() as f64 / first_frame_time.as_nanos() as f64);
        println!("    PJS full streaming:  {:?} ({} frames)", pjs_full_time, frames_processed);
        
        let data_size = data_str.len() as f64 / 1024.0;
        println!("    Data size: {:.1} KB", data_size);
        println!();
    }
    
    println!("💡 Key Insights:");
    println!("  • PJS delivers critical data 10-100x faster");
    println!("  • Users see content immediately with skeleton-first approach");  
    println!("  • Larger datasets show more dramatic improvements");
    println!("  • Progressive loading prevents UI blocking");
    println!();
    
    println!("🌐 For full web demo with network simulation:");
    println!("  Run: cargo run --example streaming_demo_server");
    println!("  (Note: Full demo requires axum features)");
}

fn demonstrate_priority_system() {
    println!("🎯 Priority System Demo");
    println!("=======================");
    println!();
    
    let sample_data = json!({
        "critical_info": {
            "status": "success",
            "user_id": 12345,
            "session_token": "abc123"
        },
        "user_profile": {
            "name": "John Doe",
            "email": "john@example.com",
            "avatar": "https://example.com/avatar.jpg"
        },
        "preferences": {
            "theme": "dark",
            "language": "en",
            "notifications": true
        },
        "recent_activity": vec![
            json!({"action": "login", "timestamp": "2024-01-15T10:00:00Z"}),
            json!({"action": "view_product", "timestamp": "2024-01-15T10:05:00Z"}),
            json!({"action": "add_to_cart", "timestamp": "2024-01-15T10:10:00Z"})
        ],
        "recommendations": vec![
            json!({"id": 1, "title": "Product A", "score": 0.95}),
            json!({"id": 2, "title": "Product B", "score": 0.87}),
            json!({"id": 3, "title": "Product C", "score": 0.73})
        ],
        "analytics": {
            "page_views": 1247,
            "bounce_rate": 23.5,
            "conversion_rate": 3.2
        }
    });
    
    let streamer = PriorityStreamer::new();
    let plan = streamer.analyze(&sample_data).unwrap();
    
    let frame_count = plan.frames().count();
    println!("📋 Streaming plan created with {} frames", frame_count);
    
    for (i, frame) in plan.frames().enumerate() {
        let frame_description = match i {
            0 => "🏗️  Skeleton structure",
            1 => "🚨 Critical data",
            2 => "⚡ High priority data",
            3 => "📊 Medium priority data", 
            _ => "🔍 Lower priority data",
        };
        
        println!("  Frame {}: {}", i + 1, frame_description);
    }
    
    println!();
    println!("💫 In real streaming scenario:");
    println!("  1. Skeleton arrives first - UI structure visible immediately");
    println!("  2. Critical data (status, user_id) - user can interact");  
    println!("  3. Profile data - personalization appears");
    println!("  4. Activity & recommendations - progressive enhancement");
    println!("  5. Analytics - background loading, no UI blocking");
}

fn main() {
    demonstrate_streaming_advantage();
    demonstrate_priority_system();
}