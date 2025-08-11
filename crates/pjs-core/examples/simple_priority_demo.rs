//! Simple Priority JSON Streaming example
//!
//! Demonstrates basic PJS functionality with working API

use pjson_rs::{Priority, StreamFrame, StreamProcessor, StreamConfig};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Simple Priority JSON Streaming Demo\n");

    // Sample data representing an analytics dashboard
    let sample_data = json!({
        "dashboard": {
            "title": "Analytics Dashboard",
            "last_updated": "2024-01-15T12:00:00Z",
            "critical_alerts": {
                "high_error_rate": false,
                "service_down": false
            },
            "summary": {
                "total_requests": 284729,
                "error_rate": 0.023,
                "avg_response_time": 234.5
            },
            "charts": [
                {"type": "timeseries", "data": [1, 2, 3, 4, 5]},
                {"type": "histogram", "data": [10, 20, 30]}
            ],
            "logs": [
                {"timestamp": "12:00:00", "level": "INFO", "message": "System healthy"},
                {"timestamp": "12:01:00", "level": "WARN", "message": "High memory usage"}
            ]
        }
    });

    println!("📊 Original JSON size: {} bytes", 
        serde_json::to_string(&sample_data)?.len()
    );

    // Create stream processor
    let config = StreamConfig::default();
    let mut processor = StreamProcessor::new(config);

    // Create high-priority skeleton frame
    let skeleton_frame = StreamFrame {
        data: json!({
            "dashboard": {
                "title": sample_data["dashboard"]["title"],
                "last_updated": sample_data["dashboard"]["last_updated"],
                "critical_alerts": sample_data["dashboard"]["critical_alerts"],
                "summary": null,
                "charts": [],
                "logs": []
            }
        }),
        priority: Priority::new(100).unwrap(),
        metadata: std::collections::HashMap::new(),
    };

    // Create medium-priority data frame
    let data_frame = StreamFrame {
        data: json!({
            "dashboard": {
                "summary": sample_data["dashboard"]["summary"],
                "charts": sample_data["dashboard"]["charts"]
            }
        }),
        priority: Priority::new(70).unwrap(),
        metadata: std::collections::HashMap::new(),
    };

    // Create low-priority logs frame
    let logs_frame = StreamFrame {
        data: json!({
            "dashboard": {
                "logs": sample_data["dashboard"]["logs"]
            }
        }),
        priority: Priority::new(30).unwrap(),
        metadata: std::collections::HashMap::new(),
    };

    let frames = vec![skeleton_frame, data_frame, logs_frame];

    println!("\n🔄 Processing {} frames in priority order...\n", frames.len());

    // Process frames in priority order
    for (i, frame) in frames.into_iter().enumerate() {
        println!("📦 Frame {}: Priority {}", i + 1, frame.priority.value());
        println!("   📄 Data keys: {:?}", 
            frame.data.as_object()
                .and_then(|obj| obj.get("dashboard"))
                .and_then(|dash| dash.as_object())
                .map(|dash| dash.keys().collect::<Vec<_>>())
                .unwrap_or_default()
        );

        // Process the frame
        match processor.process_frame(frame) {
            Ok(result) => {
                match result {
                    pjson_rs::stream::ProcessResult::Processed(processed_frame) => {
                        println!("   ✅ Processed successfully - Priority: {}", 
                            processed_frame.priority.value());
                        println!("   🖥️  Client can render this data immediately");
                    },
                    pjson_rs::stream::ProcessResult::Complete(_) => {
                        println!("   🎯 Stream processing completed");
                    },
                    pjson_rs::stream::ProcessResult::Incomplete => {
                        println!("   ⏳ Frame processing incomplete, waiting for more data");
                    },
                    pjson_rs::stream::ProcessResult::Error(e) => {
                        println!("   ❌ Processing error: {}", e);
                    },
                }
            },
            Err(e) => {
                println!("   ❌ Processing error: {:?}", e);
            }
        }
        println!();
    }

    println!("🏁 Demo completed successfully!");
    println!("\n💡 Key benefits demonstrated:");
    println!("   • Critical data (title, alerts) available immediately");
    println!("   • Charts load next for visual feedback");
    println!("   • Logs load last as they're least critical");
    println!("   • User sees meaningful content within milliseconds");

    Ok(())
}