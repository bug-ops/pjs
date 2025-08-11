//! Analytics dashboard dataset generation

use super::DatasetSize;
use serde_json::{json, Value};
use rand::Rng;

/// Generate analytics dashboard data
pub fn generate_analytics_data(size: DatasetSize) -> Value {
    let data_points = size.item_count(super::DatasetType::Analytics);
    let mut rng = rand::rng();
    
    // Time series data for charts
    let time_series: Vec<Value> = (0..data_points).map(|i| {
        let timestamp = chrono::Utc::now() - chrono::Duration::hours((data_points - i) as i64);
        json!({
            "timestamp": timestamp.to_rfc3339(),
            "metrics": {
                "revenue": rng.random_range(1000.0..50000.0),
                "users": rng.random_range(100..5000),
                "sessions": rng.random_range(500..25000),
                "page_views": rng.random_range(2000..100000),
                "bounce_rate": rng.random_range(20.0..60.0),
                "conversion_rate": rng.random_range(1.0..8.0)
            }
        })
    }).collect();
    
    json!({
        "dashboard": {
            "name": "PJS Analytics Demo",
            "description": "Real-time analytics dashboard",
            "last_updated": chrono::Utc::now().to_rfc3339()
        },
        "time_series": time_series,
        "summary": {
            "total_revenue": rng.random_range(100000.0..1000000.0),
            "total_users": rng.random_range(10000..500000),
            "growth_rate": rng.random_range(-5.0..25.0)
        },
        "generated_at": chrono::Utc::now().to_rfc3339()
    })
}

/// Generate real-time monitoring data  
pub fn generate_realtime_data(size: DatasetSize) -> Value {
    let metric_count = size.item_count(super::DatasetType::RealTime);
    let mut rng = rand::rng();
    
    let metrics: Vec<Value> = (0..metric_count).map(|i| {
        // Fixed: Extract complex expressions outside json! macro
        let statuses = ["healthy", "warning", "critical"];
        let status = statuses[rng.random_range(0..3)];
        
        json!({
            "id": format!("metric_{}", i),
            "name": format!("System Metric {}", i + 1),
            "value": rng.random_range(0.0..100.0),
            "status": status,
            "timestamp": chrono::Utc::now().to_rfc3339()
        })
    }).collect();
    
    json!({
        "system": {
            "name": "PJS Realtime Demo",
            "status": "operational"
        },
        "metrics": metrics,
        "generated_at": chrono::Utc::now().to_rfc3339()
    })
}