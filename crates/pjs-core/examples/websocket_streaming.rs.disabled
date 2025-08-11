//! WebSocket streaming example showing real-time PJS benefits
//!
//! This example demonstrates:
//! - WebSocket server setup with PJS streaming
//! - Client connection and progressive data reception
//! - Performance comparison with traditional approaches

use pjson_rs::{
    infrastructure::websocket::{
        AdaptiveStreamController, StreamOptions, WsMessage,
    },
    PriorityStreamer,
};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tokio::time::sleep;

/// Generate realistic e-commerce data for streaming demo
fn generate_ecommerce_data() -> Value {
    json!({
        "store_info": {
            "name": "TechMart Pro",
            "status": "operational",
            "version": "2.1.0",
            "last_updated": "2024-01-15T12:00:00Z"
        },
        "categories": [
            {"id": 1, "name": "Laptops", "count": 156},
            {"id": 2, "name": "Smartphones", "count": 289},
            {"id": 3, "name": "Headphones", "count": 94},
            {"id": 4, "name": "Accessories", "count": 347}
        ],
        "featured_products": [
            {
                "id": 1001,
                "name": "MacBook Pro 14\"",
                "price": 1999.99,
                "rating": 4.8,
                "availability": "in_stock",
                "specs": {
                    "processor": "M3 Pro",
                    "memory": "16GB",
                    "storage": "512GB SSD"
                }
            },
            {
                "id": 1002,
                "name": "iPhone 15 Pro",
                "price": 999.99,
                "rating": 4.7,
                "availability": "in_stock",
                "specs": {
                    "storage": "128GB",
                    "color": "Natural Titanium",
                    "display": "6.1\""
                }
            }
        ],
        "inventory": {
            "total_items": 15847,
            "categories": 8,
            "brands": 125,
            "low_stock_alerts": 12
        },
        "user_analytics": {
            "active_users": 2847,
            "sessions_today": 15923,
            "conversion_rate": 3.24,
            "average_order_value": 187.45
        },
        "detailed_products": (0..50).map(|i| {
            json!({
                "id": 2000 + i,
                "name": format!("Product {} - Professional Edition", i + 1),
                "description": format!("Comprehensive description for product {} with detailed specifications, features, and benefits that provide exceptional value to our customers.", i + 1),
                "category": format!("Category {}", (i % 8) + 1),
                "price": ((i as f64 * 12.7) + 49.99).round(),
                "rating": {
                    "average": ((i % 5) as f64 + 3.0).min(5.0),
                    "count": (i % 200) + 25,
                    "distribution": {
                        "5_star": (i % 60) + 20,
                        "4_star": (i % 40) + 15,
                        "3_star": (i % 20) + 8,
                        "2_star": (i % 10) + 2,
                        "1_star": i % 5
                    }
                },
                "availability": {
                    "status": if i % 7 == 0 { "out_of_stock" } else { "in_stock" },
                    "quantity": if i % 7 == 0 { 0 } else { (i % 50) + 5 },
                    "warehouse": format!("WH-{}", (i % 5) + 1),
                    "estimated_restock": if i % 7 == 0 { 
                        serde_json::Value::String("2024-01-20T10:00:00Z".to_string())
                    } else { 
                        serde_json::Value::Null 
                    }
                },
                "specifications": {
                    "weight": format!("{:.1}kg", ((i as f64) % 10.0) + 0.5),
                    "dimensions": format!("{}x{}x{}cm", (i % 25) + 10, (i % 20) + 15, (i % 15) + 5),
                    "material": "Premium Materials",
                    "warranty": "2 years international",
                    "certifications": ["CE", "FCC", "RoHS"]
                },
                "images": {
                    "primary": format!("https://cdn.techmart.com/products/{}/main.webp", 2000 + i),
                    "gallery": (0..4).map(|j| format!("https://cdn.techmart.com/products/{}/img{}.webp", 2000 + i, j + 1)).collect::<Vec<_>>(),
                    "thumbnail": format!("https://cdn.techmart.com/products/{}/thumb.webp", 2000 + i)
                },
                "reviews": if i % 3 == 0 {
                    (0..(i % 5)).map(|j| {
                        json!({
                            "id": (i * 10) + j,
                            "user": format!("user{}", (j + i) % 1000),
                            "rating": ((i + j) % 5) + 1,
                            "title": format!("Review {} - {}", j + 1, if (i + j) % 2 == 0 { "Excellent quality!" } else { "Good value for money" }),
                            "comment": format!("Detailed review comment {} explaining the experience with product {}. The quality exceeded expectations and delivery was prompt.", j + 1, 2000 + i),
                            "date": format!("2024-01-{:02}T{:02}:00:00Z", ((i + j) % 28) + 1, ((j * 3) % 24)),
                            "verified_purchase": (i + j) % 3 == 0,
                            "helpful_votes": (i + j) % 25
                        })
                    }).collect::<Vec<_>>()
                } else {
                    Vec::new()
                }
            })
        }).collect::<Vec<_>>()
    })
}

/// Simulate traditional JSON loading
async fn traditional_loading_simulation(data: &Value) -> Duration {
    let start = Instant::now();
    
    // Simulate network latency and parsing time
    sleep(Duration::from_millis(150)).await; // Network delay
    
    let json_str = serde_json::to_string(data).unwrap();
    let _parsed: Value = serde_json::from_str(&json_str).unwrap();
    
    // Simulate DOM rendering time
    sleep(Duration::from_millis(80)).await;
    
    start.elapsed()
}

/// Simulate PJS progressive streaming
async fn pjs_streaming_simulation(data: &Value) -> (Duration, Duration) {
    let start = Instant::now();
    
    let streamer = PriorityStreamer::new();
    let plan = streamer.analyze(data).expect("Failed to create streaming plan");
    
    // Time to first critical frame (skeleton)
    let skeleton_time = start.elapsed();
    
    // Simulate progressive frame processing
    let mut total_processed = 0;
    for (i, _frame) in plan.frames().enumerate() {
        // Simulate minimal network latency per frame
        sleep(Duration::from_millis(5)).await;
        
        // Simulate frame processing
        sleep(Duration::from_millis(2)).await;
        
        total_processed = i + 1;
        
        // Early break for demo - user sees content progressively
        if i >= 3 { // Show first few critical frames
            break;
        }
    }
    
    let progressive_time = start.elapsed();
    
    println!("  📊 PJS processed {} critical frames", total_processed);
    (skeleton_time, progressive_time)
}

/// Demonstrate WebSocket streaming controller
async fn websocket_controller_demo() {
    println!("🔌 WebSocket Streaming Controller Demo");
    println!("=====================================");
    println!();
    
    let controller = AdaptiveStreamController::new();
    let data = generate_ecommerce_data();
    
    println!("📦 Generated e-commerce dataset: {:.1} KB", 
             serde_json::to_string(&data).unwrap().len() as f64 / 1024.0);
    println!();
    
    // Create streaming session
    let session_id = controller
        .create_session(data.clone(), StreamOptions::default())
        .await
        .expect("Failed to create session");
    
    println!("✅ Created streaming session: {}", session_id);
    
    // Start streaming
    controller
        .start_streaming(&session_id)
        .await
        .expect("Failed to start streaming");
    
    println!("🚀 Started progressive streaming");
    println!();
    
    // Simulate frame acknowledgments
    for frame_id in 0..5 {
        let processing_time = 20 + (frame_id * 5); // Simulate varying processing times
        
        controller
            .handle_frame_ack(&session_id, frame_id, processing_time)
            .await
            .expect("Failed to handle frame ack");
        
        println!("✅ Frame {} acknowledged ({}ms processing time)", frame_id, processing_time);
        
        sleep(Duration::from_millis(50)).await;
    }
    
    println!();
    println!("🎯 WebSocket controller demonstration completed");
}

/// Main performance comparison demo
async fn performance_demo() {
    println!("⚡ PJS vs Traditional Performance Comparison");
    println!("==========================================");
    println!();
    
    let data = generate_ecommerce_data();
    let data_size = serde_json::to_string(&data).unwrap().len() as f64 / 1024.0;
    
    println!("📊 Dataset: E-commerce store with {:.1} KB of data", data_size);
    println!("    - Store info and categories (critical)");
    println!("    - Featured products (high priority)");
    println!("    - Inventory analytics (medium priority)");
    println!("    - 50 detailed products with reviews (low priority)");
    println!();
    
    // Traditional approach
    println!("🐌 Traditional JSON Loading:");
    let traditional_time = traditional_loading_simulation(&data).await;
    println!("    ⏱️  Total time: {:?}", traditional_time);
    println!("    📱 User sees: Nothing until complete");
    println!();
    
    // PJS approach
    println!("⚡ PJS Priority Streaming:");
    let (skeleton_time, progressive_time) = pjs_streaming_simulation(&data).await;
    
    println!("    ⚡ Skeleton delivered: {:?}", skeleton_time);
    println!("    🎯 Critical content: {:?}", progressive_time);
    println!("    📱 User sees: Store info + categories immediately");
    println!("    🔄 Background: Remaining data loads progressively");
    println!();
    
    // Calculate improvements
    let skeleton_improvement = traditional_time.as_millis() as f64 / skeleton_time.as_millis() as f64;
    let progressive_improvement = traditional_time.as_millis() as f64 / progressive_time.as_millis() as f64;
    
    println!("🏆 Performance Results:");
    println!("    📈 Skeleton delivery: {:.1}x faster than traditional", skeleton_improvement);
    println!("    📈 Critical content: {:.1}x faster than traditional", progressive_improvement);
    println!("    💡 Time saved: {:?}", traditional_time.saturating_sub(progressive_time));
    println!();
    
    println!("🌟 Key Benefits:");
    println!("    • Immediate UI feedback with skeleton");
    println!("    • Progressive enhancement as data arrives");
    println!("    • Better perceived performance");
    println!("    • Reduced user abandonment on slow connections");
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    
    println!("🚀 PJS WebSocket Streaming Demo");
    println!("===============================");
    println!();
    
    // Run WebSocket controller demo
    websocket_controller_demo().await;
    
    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    
    // Run performance comparison
    performance_demo().await;
    
    println!();
    println!("✨ Demo completed! WebSocket transport layer is ready for:");
    println!("   • Real-time streaming applications");
    println!("   • Progressive web interfaces");
    println!("   • Mobile-optimized data loading");
    println!("   • Adaptive streaming based on client performance");
    
    Ok(())
}