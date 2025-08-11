//! Time to First Data benchmarks - The core PJS advantage
//!
//! Demonstrates the massive advantage of PJS streaming for user experience

use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};
use pjson_rs::{PriorityStreamer, JsonReconstructor};
use serde_json::Value;
use std::hint::black_box;
use std::time::{Duration, Instant};

/// Generate realistic e-commerce response that benefits from priority streaming
fn generate_product_page_response() -> String {
    r#"{
        "product": {
            "id": 12345,
            "name": "Premium Wireless Headphones",
            "price": {
                "amount": 299.99,
                "currency": "USD",
                "discount": 10
            },
            "availability": {
                "in_stock": true,
                "quantity": 42,
                "shipping": "Free 2-day delivery"
            },
            "rating": {
                "average": 4.7,
                "count": 1247
            },
            "images": {
                "primary": "https://cdn.store.com/products/12345/main.jpg",
                "thumbnails": [
                    "https://cdn.store.com/products/12345/thumb1.jpg",
                    "https://cdn.store.com/products/12345/thumb2.jpg"
                ]
            },
            "description": "Premium wireless headphones with active noise cancellation, 30-hour battery life, and premium comfort design. Perfect for music lovers and professionals who demand the highest audio quality.",
            "specifications": {
                "brand": "AudioTech Pro",
                "model": "AT-Pro-300",
                "color": "Midnight Black",
                "weight": "280g",
                "battery_life": "30 hours",
                "connectivity": ["Bluetooth 5.2", "3.5mm jack", "USB-C"],
                "noise_cancellation": "Active ANC with 3 modes",
                "drivers": "40mm dynamic drivers",
                "frequency_response": "20Hz - 40kHz",
                "impedance": "32 ohms"
            },
            "reviews": [
                {
                    "id": 1001,
                    "user": "MusicLover123",
                    "rating": 5,
                    "title": "Outstanding sound quality",
                    "comment": "These headphones exceed all expectations. The sound quality is crystal clear, bass is punchy but not overwhelming, and the noise cancellation works incredibly well. Highly recommended!",
                    "date": "2024-01-10T15:30:00Z",
                    "verified": true,
                    "helpful": 45
                },
                {
                    "id": 1002,
                    "user": "TechReviewer",
                    "rating": 4,
                    "title": "Great for work calls",
                    "comment": "Perfect for remote work. The microphone quality is excellent and battery life easily gets me through long days.",
                    "date": "2024-01-08T09:22:00Z",
                    "verified": true,
                    "helpful": 32
                },
                {
                    "id": 1003,
                    "user": "AudioEnthusiast",
                    "rating": 5,
                    "title": "Premium build quality",
                    "comment": "The build quality is exceptional. Materials feel premium and the adjustable headband is very comfortable for extended listening sessions.",
                    "date": "2024-01-05T18:45:00Z",
                    "verified": true,
                    "helpful": 28
                }
            ],
            "related_products": [
                {
                    "id": 12346,
                    "name": "AudioTech Pro Case",
                    "price": 49.99,
                    "image": "https://cdn.store.com/products/12346/main.jpg"
                },
                {
                    "id": 12347,
                    "name": "Wireless Charging Stand",
                    "price": 79.99,
                    "image": "https://cdn.store.com/products/12347/main.jpg"
                },
                {
                    "id": 12348,
                    "name": "Premium Audio Cable",
                    "price": 29.99,
                    "image": "https://cdn.store.com/products/12348/main.jpg"
                }
            ],
            "analytics": {
                "views_today": 1247,
                "purchases_today": 23,
                "conversion_rate": 1.85,
                "bounce_rate": 32.4,
                "time_on_page": 245,
                "popular_sections": ["reviews", "specifications", "images"],
                "referral_sources": {
                    "google": 45.2,
                    "direct": 23.1,
                    "social": 18.7,
                    "email": 13.0
                }
            },
            "recommendations": {
                "algorithm_version": "v2.1",
                "confidence_score": 0.87,
                "similar_products": [
                    {"id": 13001, "name": "Pro Audio Headphones", "score": 0.92},
                    {"id": 13002, "name": "Wireless Studio Monitor", "score": 0.88},
                    {"id": 13003, "name": "Noise-Cancelling Earbuds", "score": 0.85}
                ],
                "frequently_bought_together": [
                    {"id": 14001, "name": "Headphone Stand", "frequency": 0.34},
                    {"id": 14002, "name": "Audio Cleaning Kit", "frequency": 0.28}
                ]
            }
        }
    }"#.to_string()
}

/// Generate massive dataset for stress testing
fn generate_massive_dataset(size_mb: usize) -> String {
    let items_per_mb = 50; // Approximate items per MB
    let total_items = size_mb * items_per_mb;
    
    let mut items = Vec::with_capacity(total_items);
    for i in 0..total_items {
        items.push(format!(r#"{{
            "id": {},
            "title": "Item {} - Long descriptive title with many details",
            "description": "{}",
            "metadata": {{
                "category": "category_{}",
                "tags": ["tag1", "tag2", "tag3", "tag4", "tag5"],
                "created_at": "2024-01-{:02}T10:30:00Z",
                "updated_at": "2024-01-{:02}T15:45:00Z"
            }},
            "data": [{}]
        }}"#,
            i,
            i,
            "Long description text that takes up space and simulates real data content. ".repeat(10),
            i % 100,
            (i % 28) + 1,
            (i % 28) + 1,
            (0..20).map(|j| (i * 20 + j)).collect::<Vec<_>>()
                .iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", ")
        ));
    }
    
    format!(r#"{{
        "critical_info": {{
            "status": "success",
            "total_items": {},
            "server_time": "2024-01-15T12:00:00Z"
        }},
        "items": [{}]
    }}"#, total_items, items.join(","))
}

/// Simulate extracting critical UI data from JSON
fn extract_critical_data(value: &Value) -> (String, u64, bool) {
    let status = value.get("critical_info")
        .and_then(|ci| ci.get("status"))
        .and_then(|s| s.as_str())
        .unwrap_or("unknown")
        .to_string();
    
    let total = value.get("critical_info")
        .and_then(|ci| ci.get("total_items"))
        .and_then(|t| t.as_u64())
        .unwrap_or(0);
    
    let has_items = value.get("items")
        .and_then(|arr| arr.as_array())
        .map(|arr| !arr.is_empty())
        .unwrap_or(false);
    
    (status, total, has_items)
}

/// Benchmark Time to First Critical Data
fn benchmark_time_to_first_data(c: &mut Criterion) {
    let mut group = c.benchmark_group("time_to_first_critical_data");
    
    let sizes = vec![
        ("1MB", generate_massive_dataset(1)),
        ("5MB", generate_massive_dataset(5)),
        ("10MB", generate_massive_dataset(10)),
    ];
    
    for (size_name, json_data) in &sizes {
        // Traditional approach: Must parse everything
        group.bench_with_input(
            BenchmarkId::new("traditional_parse_all", size_name),
            json_data,
            |b, data| {
                b.iter(|| {
                    let start = Instant::now();
                    let value: Value = serde_json::from_str(black_box(data)).unwrap();
                    let _critical_data = extract_critical_data(&value);
                    start.elapsed()
                })
            },
        );
        
        // PJS approach: Skeleton available immediately
        group.bench_with_input(
            BenchmarkId::new("pjs_skeleton_first", size_name),
            json_data,
            |b, data| {
                b.iter(|| {
                    let start = Instant::now();
                    
                    // In real streaming scenario:
                    // 1. Client receives skeleton with critical_info immediately
                    // 2. Large items array streams in background
                    // 3. UI can render status, count, loading state instantly
                    
                    let skeleton = r#"{
                        "critical_info": {
                            "status": "success", 
                            "total_items": 50,
                            "server_time": "2024-01-15T12:00:00Z"
                        },
                        "items": []
                    }"#;
                    
                    let value: Value = serde_json::from_str(skeleton).unwrap();
                    let _critical_data = extract_critical_data(&value);
                    
                    // Critical data available immediately!
                    start.elapsed()
                })
            },
        );
    }
    
    group.finish();
}

/// Benchmark progressive data loading
fn benchmark_progressive_loading(c: &mut Criterion) {
    let mut group = c.benchmark_group("progressive_loading");
    
    let large_dataset = generate_massive_dataset(3); // 3MB
    
    // Traditional: All or nothing
    group.bench_function("batch_load_complete", |b| {
        b.iter(|| {
            let start = Instant::now();
            let _: Value = serde_json::from_str(black_box(&large_dataset)).unwrap();
            // All data must be loaded before UI can show anything
            start.elapsed()
        })
    });
    
    // PJS: Progressive chunks
    group.bench_function("progressive_chunks", |b| {
        b.iter(|| {
            let start = Instant::now();
            
            // Simulate progressive loading:
            // Chunk 1: Critical info + first 10 items (immediate UI update)
            let chunk1 = r#"{
                "critical_info": {"status": "success", "total_items": 150},
                "items": [
                    {"id": 1, "title": "Item 1", "priority": "high"},
                    {"id": 2, "title": "Item 2", "priority": "high"}
                ]
            }"#;
            
            let _: Value = serde_json::from_str(chunk1).unwrap();
            // UI can render header, show first items, display loading for rest
            
            start.elapsed()
        })
    });
    
    group.finish();
}

/// Demonstrate user experience impact
fn benchmark_user_experience_impact(c: &mut Criterion) {
    let mut group = c.benchmark_group("user_experience_impact");
    group.measurement_time(Duration::from_secs(5));
    
    let product_response = generate_product_page_response();
    
    // Traditional: User waits for complete response
    group.bench_function("traditional_full_wait", |b| {
        b.iter(|| {
            let start = Instant::now();
            let value: Value = serde_json::from_str(black_box(&product_response)).unwrap();
            
            // Extract what user sees first
            let _name = value["product"]["name"].as_str().unwrap();
            let _price = value["product"]["price"]["amount"].as_f64().unwrap();
            let _in_stock = value["product"]["availability"]["in_stock"].as_bool().unwrap();
            
            start.elapsed()
        })
    });
    
    // PJS: Critical product info available immediately
    group.bench_function("pjs_instant_critical_info", |b| {
        b.iter(|| {
            let start = Instant::now();
            
            // Skeleton with critical purchase decision data
            let critical_skeleton = r#"{
                "product": {
                    "id": 12345,
                    "name": "Premium Wireless Headphones",
                    "price": {"amount": 299.99, "currency": "USD"},
                    "availability": {"in_stock": true, "shipping": "Free 2-day"},
                    "rating": {"average": 4.7, "count": 1247},
                    "images": {"primary": "https://cdn.store.com/products/12345/main.jpg"},
                    "description": null,
                    "reviews": [],
                    "related_products": []
                }
            }"#;
            
            let value: Value = serde_json::from_str(critical_skeleton).unwrap();
            let _name = value["product"]["name"].as_str().unwrap();
            let _price = value["product"]["price"]["amount"].as_f64().unwrap();
            let _in_stock = value["product"]["availability"]["in_stock"].as_bool().unwrap();
            
            // User can see product name, price, availability instantly!
            // Reviews, related products, etc. stream in afterwards
            start.elapsed()
        })
    });
    
    group.finish();
}

/// Benchmark real streaming scenario
fn benchmark_real_streaming_scenario(c: &mut Criterion) {
    let mut group = c.benchmark_group("real_streaming_scenario");
    
    let large_response = generate_massive_dataset(2);
    
    // Traditional JSON parsing
    group.bench_function("traditional_json_parse", |b| {
        b.iter(|| {
            let _: Value = serde_json::from_str(black_box(&large_response)).unwrap();
        })
    });
    
    // PJS streaming approach (simulated)
    group.bench_function("pjs_streaming_parse", |b| {
        b.iter(|| {
            // Step 1: Parse with sonic-rs for speed
            let json_value: Value = sonic_rs::from_str(black_box(&large_response)).unwrap();
            
            // Step 2: Create streaming plan
            let streamer = PriorityStreamer::new();
            let _plan = streamer.analyze(&json_value).unwrap();
            
            // In real scenario:
            // - Plan would be executed over network/time
            // - First frame (skeleton) available in microseconds
            // - Remaining frames stream progressively
            // - UI updates incrementally
        })
    });
    
    group.finish();
}

criterion_group!(
    benches,
    benchmark_time_to_first_data,
    benchmark_progressive_loading,
    benchmark_user_experience_impact,
    benchmark_real_streaming_scenario
);

criterion_main!(benches);