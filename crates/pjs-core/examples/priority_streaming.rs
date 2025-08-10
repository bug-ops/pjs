//! Priority JSON Streaming example
//!
//! This example demonstrates the core Priority JSON Streaming functionality:
//! - Analyzing JSON structure
//! - Generating skeleton + patches
//! - Priority-based frame delivery
//! - Incremental reconstruction

use pjson_rs::{StreamProcessor, Priority, StreamFrame};
use pjson_rs::stream::PatchOperation;
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Priority JSON Streaming Protocol Demo\n");
    
    // Create sample data representing a typical API response
    let sample_data = json!({
        "store": {
            "id": 12345,
            "name": "Book Haven",
            "status": "active",
            "location": "New York",
            "books": [
                {
                    "id": 1,
                    "title": "Clean Code",
                    "author": "Robert C. Martin",
                    "price": 32.99,
                    "category": "Programming",
                    "reviews": [
                        {"user": "Alice", "rating": 5, "text": "Excellent book for developers!"},
                        {"user": "Bob", "rating": 4, "text": "Very practical examples"},
                        {"user": "Charlie", "rating": 5, "text": "Must-read for any programmer"}
                    ]
                },
                {
                    "id": 2,
                    "title": "Design Patterns",
                    "author": "Gang of Four",
                    "price": 45.00,
                    "category": "Programming",
                    "reviews": [
                        {"user": "Diana", "rating": 5, "text": "Classic computer science"},
                        {"user": "Eve", "rating": 4, "text": "Good reference material"}
                    ]
                }
            ],
            "analytics": {
                "daily_visitors": 1250,
                "conversion_rate": 0.034,
                "popular_categories": ["Programming", "Science Fiction", "Business"],
                "sales_data": {
                    "last_30_days": [450, 523, 612, 578, 689, 734, 698, 756, 823, 891],
                    "revenue": 15678.90
                }
            },
            "staff": [
                {"name": "John Manager", "role": "manager", "email": "john@bookhaven.com"},
                {"name": "Jane Clerk", "role": "clerk", "email": "jane@bookhaven.com"}
            ]
        }
    });
    
    println!("📊 Original JSON size: {} bytes", 
             serde_json::to_string(&sample_data)?.len());
    
    // Create stream processor
    let processor = StreamProcessor::new();
    
    // Convert to bytes for processing
    let json_bytes = serde_json::to_vec(&sample_data)?;
    
    // Process JSON into streaming plan
    println!("🔄 Analyzing JSON structure and creating streaming plan...");
    let mut plan = processor.process_json(&json_bytes)?;
    
    println!("📋 Streaming plan created with {} frames\n", plan.remaining_frames());
    
    // Simulate streaming each frame
    let mut frame_count = 1;
    while let Some(frame) = plan.next_frame() {
        match frame {
            StreamFrame::Skeleton { data, priority, complete } => {
                println!("📦 Frame {}: SKELETON (Priority: {:?})", frame_count, priority);
                println!("   Complete: {}", complete);
                println!("   Structure: {}", 
                         serde_json::to_string_pretty(&data)?.lines().take(10).collect::<Vec<_>>().join("\n"));
                if serde_json::to_string(&data)?.lines().count() > 10 {
                    println!("   ... (truncated)");
                }
                println!();
            },
            StreamFrame::Patch { patches, priority } => {
                println!("🔧 Frame {}: PATCH (Priority: {:?})", frame_count, priority);
                println!("   {} patch operations:", patches.len());
                
                for (i, patch) in patches.iter().take(3).enumerate() {
                    let op_desc = match &patch.operation {
                        PatchOperation::Set { .. } => "SET".to_string(),
                        PatchOperation::Append { values } => 
                            format!("APPEND {} items", values.len()),
                        PatchOperation::Replace { .. } => "REPLACE".to_string(),
                        PatchOperation::Remove => "REMOVE".to_string(),
                    };
                    println!("     {}. Path: {} -> Operation: {}", 
                             i + 1, 
                             patch.path.to_json_pointer(),
                             op_desc);
                }
                
                if patches.len() > 3 {
                    println!("     ... and {} more patches", patches.len() - 3);
                }
                println!();
            },
            StreamFrame::Complete { checksum } => {
                println!("✅ Frame {}: COMPLETE", frame_count);
                if let Some(checksum) = checksum {
                    println!("   Checksum: {}", checksum);
                }
                println!();
            }
        }
        
        frame_count += 1;
        
        // Simulate network delay between frames
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    
    println!("🎉 Streaming completed!");
    println!("\n📈 Performance Benefits:");
    println!("   • Critical data (ID, name, status) delivered in first frames");
    println!("   • User sees essential information immediately");  
    println!("   • Large arrays (reviews, analytics) streamed progressively");
    println!("   • Total frames transmitted: {}", frame_count - 1);
    println!("   • Client can start rendering immediately after skeleton");
    
    // Demonstrate priority ordering
    println!("\n🎯 Priority Analysis:");
    demonstrate_priority_calculation();
    
    Ok(())
}

fn demonstrate_priority_calculation() {
    let examples = vec![
        ("id", Priority::CRITICAL),
        ("name", Priority::HIGH), 
        ("title", Priority::HIGH),
        ("description", Priority::MEDIUM),
        ("reviews", Priority::BACKGROUND),
        ("analytics", Priority::LOW),
    ];
    
    for (field, expected_priority) in examples {
        println!("   • '{}' field -> {:?} priority", field, expected_priority);
    }
    
    println!("\n💡 This ensures users see the most important data first!");
}