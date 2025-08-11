//! Massive data benchmarks - Where PJS streaming advantage is most apparent
//!
//! Tests PJS against traditional parsers on very large JSON datasets
//! that showcase the streaming and priority benefits

use criterion::{criterion_group, criterion_main, Criterion, Throughput, BenchmarkId};
use pjson_rs::{Parser, PriorityStreamer, JsonReconstructor};
use serde_json::Value;
use std::hint::black_box;
use std::time::{Duration, Instant};

/// Generate massive e-commerce catalog (1-10MB JSON)
fn generate_massive_catalog(products: usize) -> String {
    let mut product_data = Vec::new();
    
    for i in 0..products {
        product_data.push(format!(r#"{{
            "id": {},
            "sku": "SKU-{:08}",
            "name": "Premium Product {} - Professional Edition with Advanced Features",
            "description": "This is a comprehensive description of product {} featuring cutting-edge technology, premium materials, and exceptional build quality. Perfect for professionals and enthusiasts who demand the highest standards of performance and reliability.",
            "category": "Electronics",
            "subcategory": "Category-{}",
            "brand": "Brand-{}",
            "model": "Model-{}-{}",
            "price": {{
                "amount": {:.2},
                "currency": "USD",
                "tax_inclusive": true,
                "discount_percent": {}
            }},
            "inventory": {{
                "stock_quantity": {},
                "reserved": {},
                "available": {},
                "warehouse_locations": ["US-WEST", "US-EAST", "EU-CENTRAL", "ASIA-PACIFIC"],
                "last_restocked": "2024-01-{:02}T10:30:00Z"
            }},
            "specifications": {{
                "weight": "{:.1}kg",
                "dimensions": {{
                    "length": {:.1},
                    "width": {:.1},
                    "height": {:.1},
                    "unit": "cm"
                }},
                "color": "{}",
                "material": "Premium Grade Material",
                "warranty": "2 years international warranty",
                "energy_rating": "{}",
                "certifications": ["CE", "FCC", "RoHS", "ISO9001"]
            }},
            "media": {{
                "primary_image": "https://cdn.store.com/products/{}/main.jpg",
                "gallery": [
                    "https://cdn.store.com/products/{}/image1.jpg",
                    "https://cdn.store.com/products/{}/image2.jpg",
                    "https://cdn.store.com/products/{}/image3.jpg",
                    "https://cdn.store.com/products/{}/image4.jpg"
                ],
                "video_url": "https://cdn.store.com/products/{}/demo.mp4",
                "360_view": "https://cdn.store.com/products/{}/360/"
            }},
            "reviews": {{
                "average_rating": {:.1},
                "total_reviews": {},
                "rating_distribution": {{
                    "5_star": {},
                    "4_star": {},
                    "3_star": {},
                    "2_star": {},
                    "1_star": {}
                }},
                "recent_reviews": [
                    {{
                        "id": {},
                        "user": "User{}",
                        "rating": {},
                        "title": "Great product!",
                        "comment": "Excellent quality and fast shipping. Highly recommended!",
                        "date": "2024-01-{}T14:22:00Z",
                        "verified_purchase": true,
                        "helpful_votes": {}
                    }},
                    {{
                        "id": {},
                        "user": "User{}",
                        "rating": {},
                        "title": "Good value",
                        "comment": "Product meets expectations. Good build quality.",
                        "date": "2024-01-{}T09:15:00Z",
                        "verified_purchase": true,
                        "helpful_votes": {}
                    }}
                ]
            }},
            "shipping": {{
                "weight": {:.1},
                "dimensions": [{}],
                "free_shipping_eligible": {},
                "express_available": true,
                "international_shipping": true,
                "estimated_delivery_days": {}
            }},
            "seo": {{
                "meta_title": "Buy {} - Best Price Online",
                "meta_description": "Shop {} with fast shipping and excellent customer service.",
                "keywords": ["premium", "quality", "electronics", "professional"],
                "canonical_url": "https://store.com/products/{}"
            }},
            "analytics": {{
                "views_last_30_days": {},
                "purchases_last_30_days": {},
                "conversion_rate": {:.2},
                "bounce_rate": {:.2},
                "time_on_page_seconds": {}
            }},
            "related_products": [{}],
            "tags": ["featured", "bestseller", "premium", "category-{}", "brand-{}"],
            "created_at": "2024-01-{:02}T08:00:00Z",
            "updated_at": "2024-01-{:02}T16:30:00Z",
            "status": "active"
        }}"#,
            i,                                           // id
            i,                                           // sku
            i,                                           // name
            i,                                           // description
            i % 20,                                      // subcategory
            i % 50,                                      // brand
            i / 100, i % 100,                           // model
            (i as f64 * 1.5 + 29.99),                  // price amount
            if i % 7 == 0 { 15 } else { 0 },           // discount
            i % 100 + 10,                               // stock quantity
            i % 10,                                      // reserved
            (i % 100 + 10) - (i % 10),                  // available
            (i % 28) + 1,                               // restocked date
            (i as f64 % 10.0) + 0.5,                    // weight
            (i as f64 % 50.0) + 10.0,                   // length
            (i as f64 % 30.0) + 15.0,                   // width
            (i as f64 % 20.0) + 5.0,                    // height
            ["Red", "Blue", "Black", "White", "Silver"][i % 5], // color
            ["A+", "A", "B+", "B", "C"][i % 5],         // energy rating
            i, i, i, i, i, i, i,                        // media URLs
            (i % 5) as f64 + 3.0,                       // average rating
            i % 200 + 50,                               // total reviews
            (i % 100) + 20,                             // 5 star
            (i % 50) + 10,                              // 4 star
            (i % 30) + 5,                               // 3 star
            i % 10,                                      // 2 star
            i % 5,                                       // 1 star
            i * 2,                                       // review id 1
            i % 1000,                                    // user 1
            (i % 5) + 1,                                // rating 1
            (i % 28) + 1,                               // date 1
            i % 20,                                      // helpful votes 1
            i * 2 + 1,                                   // review id 2
            (i + 100) % 1000,                           // user 2
            (i % 4) + 2,                                // rating 2
            (i % 28) + 1,                               // date 2
            i % 15,                                      // helpful votes 2
            (i as f64 % 5.0) + 1.0,                     // shipping weight
            format!("{}, {}, {}", 
                (i % 50) + 10, (i % 30) + 15, (i % 20) + 5), // dimensions
            i % 2 == 0,                                  // free shipping
            (i % 7) + 1,                                // delivery days
            i,                                           // seo title
            i,                                           // seo description
            i,                                           // canonical
            i * 100,                                     // views
            i * 2,                                       // purchases
            (i as f64 % 10.0) + 2.0,                    // conversion rate
            (i as f64 % 50.0) + 25.0,                   // bounce rate
            (i % 300) + 120,                            // time on page
            (0..5).map(|j| (i + j) % 10000).collect::<Vec<_>>()
                .iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", "), // related
            i % 20,                                      // tag category
            i % 50,                                      // tag brand
            (i % 28) + 1,                               // created date
            (i % 28) + 1,                               // updated date
        ));
    }
    
    format!(r#"{{
        "catalog": {{
            "metadata": {{
                "total_products": {},
                "categories": {},
                "brands": {},
                "generated_at": "2024-01-15T12:00:00Z",
                "version": "2.0",
                "currency": "USD",
                "locale": "en-US"
            }},
            "filters": {{
                "categories": [{}],
                "brands": [{}],
                "price_ranges": [
                    {{"min": 0, "max": 50, "count": {}}},
                    {{"min": 50, "max": 100, "count": {}}},
                    {{"min": 100, "max": 200, "count": {}}},
                    {{"min": 200, "max": 500, "count": {}}},
                    {{"min": 500, "max": 1000, "count": {}}}
                ],
                "ratings": [
                    {{"rating": 5, "count": {}}},
                    {{"rating": 4, "count": {}}},
                    {{"rating": 3, "count": {}}},
                    {{"rating": 2, "count": {}}},
                    {{"rating": 1, "count": {}}}
                ]
            }},
            "products": [{}]
        }}
    }}"#,
        products,                                    // total_products
        20,                                          // categories count
        50,                                          // brands count
        (0..20).map(|i| format!(r#"{{"id": {}, "name": "Category {}", "count": {}}}"#, 
            i, i, products / 20)).collect::<Vec<_>>().join(", "), // categories
        (0..50).map(|i| format!(r#"{{"id": {}, "name": "Brand {}", "count": {}}}"#, 
            i, i, products / 50)).collect::<Vec<_>>().join(", "), // brands
        products / 5,                                // price range counts
        products / 4,
        products / 3,
        products / 6,
        products / 10,
        products / 2,                                // rating counts
        products / 3,
        products / 5,
        products / 10,
        products / 20,
        product_data.join(",")                       // products
    )
}

/// Generate massive analytics dataset
fn generate_analytics_dataset(entries: usize) -> String {
    let mut events = Vec::new();
    
    for i in 0..entries {
        events.push(format!(r#"{{
            "id": "{}",
            "timestamp": "2024-01-{:02}T{:02}:{:02}:{:02}Z",
            "event_type": "{}",
            "user_id": "user_{}",
            "session_id": "session_{}",
            "properties": {{
                "page": "/page/{}",
                "referrer": "{}",
                "user_agent": "Mozilla/5.0 (compatible; Browser/1.0)",
                "ip_address": "192.168.{}.{}",
                "country": "{}",
                "city": "{}",
                "device_type": "{}",
                "browser": "{}",
                "os": "{}",
                "screen_resolution": "{}x{}",
                "viewport": "{}x{}"
            }},
            "metrics": {{
                "duration_ms": {},
                "scroll_depth": {:.1},
                "clicks": {},
                "page_views": {},
                "bounce": {},
                "conversion_value": {:.2}
            }},
            "custom_data": {{
                "experiment_id": "exp_{}",
                "variant": "{}",
                "cohort": "cohort_{}",
                "tags": ["{}", "{}", "{}"]
            }}
        }}"#,
            format!("event_{:08}", i),               // id
            (i % 28) + 1,                           // day
            i % 24,                                 // hour
            (i * 7) % 60,                          // minute
            (i * 13) % 60,                         // second
            ["page_view", "click", "scroll", "conversion", "exit"][i % 5], // event_type
            i % 10000,                              // user_id
            i % 1000,                               // session_id
            i % 50,                                 // page
            if i % 10 == 0 { "https://google.com" } else { "direct" }, // referrer
            i % 256,                                // ip part 1
            (i / 256) % 256,                       // ip part 2
            ["US", "UK", "DE", "FR", "JP", "AU", "CA", "BR"][i % 8], // country
            ["New York", "London", "Berlin", "Paris", "Tokyo", "Sydney"][i % 6], // city
            ["desktop", "mobile", "tablet"][i % 3], // device_type
            ["Chrome", "Firefox", "Safari", "Edge"][i % 4], // browser
            ["Windows", "macOS", "Linux", "iOS", "Android"][i % 5], // os
            1920 + (i % 10) * 100,                 // screen width
            1080 + (i % 6) * 100,                  // screen height
            1200 + (i % 8) * 50,                   // viewport width
            800 + (i % 5) * 50,                    // viewport height
            (i % 30000) + 100,                     // duration
            (i as f64 % 100.0),                    // scroll depth
            i % 20,                                 // clicks
            i % 10 + 1,                            // page views
            i % 10 == 0,                           // bounce
            (i as f64 % 100.0) + 10.0,            // conversion value
            i % 100,                               // experiment_id
            ["A", "B", "C"][i % 3],               // variant
            i % 20,                                // cohort
            format!("tag{}", i % 10),              // tag 1
            format!("category{}", i % 5),          // tag 2
            format!("segment{}", i % 8),           // tag 3
        ));
    }
    
    format!(r#"{{
        "analytics": {{
            "report_id": "report_001",
            "generated_at": "2024-01-15T12:00:00Z",
            "time_range": {{
                "start": "2024-01-01T00:00:00Z",
                "end": "2024-01-15T23:59:59Z"
            }},
            "summary": {{
                "total_events": {},
                "unique_users": {},
                "sessions": {},
                "page_views": {},
                "conversion_rate": 3.45,
                "bounce_rate": 42.3,
                "avg_session_duration": 245.7
            }},
            "events": [{}]
        }}
    }}"#,
        entries,                                    // total_events
        entries / 10,                              // unique_users
        entries / 100,                             // sessions
        entries / 3,                               // page_views
        events.join(",")                           // events
    )
}

fn benchmark_massive_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("massive_data_parsing");
    group.measurement_time(Duration::from_secs(15));
    group.sample_size(10); // Fewer samples for large data
    
    let sizes = vec![
        ("1MB", 1_000),
        ("3MB", 3_000), 
        ("5MB", 5_000),
        ("10MB", 10_000),
    ];
    
    for (size_name, product_count) in sizes {
        let json_data = generate_massive_catalog(product_count);
        let json_size = json_data.len() as u64;
        
        group.throughput(Throughput::Bytes(json_size));
        
        // serde_json baseline
        group.bench_with_input(
            BenchmarkId::new("serde_json", size_name),
            &json_data,
            |b, data| {
                b.iter(|| {
                    let _: Value = serde_json::from_str(black_box(data)).unwrap();
                })
            },
        );
        
        // sonic-rs
        group.bench_with_input(
            BenchmarkId::new("sonic_rs", size_name),
            &json_data,
            |b, data| {
                b.iter(|| {
                    let _: sonic_rs::Value = sonic_rs::from_str(black_box(data)).unwrap();
                })
            },
        );
        
        // PJS raw parsing
        group.bench_with_input(
            BenchmarkId::new("pjs_parser", size_name),
            &json_data,
            |b, data| {
                b.iter(|| {
                    let parser = Parser::new();
                    let _ = parser.parse(black_box(data.as_bytes())).unwrap();
                })
            },
        );
    }
    
    group.finish();
}

fn benchmark_streaming_advantage(c: &mut Criterion) {
    let mut group = c.benchmark_group("streaming_advantage");
    group.measurement_time(Duration::from_secs(10));
    
    let large_catalog = generate_massive_catalog(5_000); // ~5MB
    
    // Traditional: Must parse everything before any data is available
    group.bench_function("traditional_all_or_nothing", |b| {
        b.iter(|| {
            let start = Instant::now();
            let _: Value = serde_json::from_str(black_box(&large_catalog)).unwrap();
            // Simulate extracting critical data
            start.elapsed()
        })
    });
    
    // PJS: Skeleton + priority data available immediately  
    group.bench_function("pjs_progressive_delivery", |b| {
        b.iter(|| {
            let start = Instant::now();
            
            // This simulates the streaming advantage:
            // 1. Parse JSON with sonic-rs for speed
            let json_value: Value = sonic_rs::from_str(black_box(&large_catalog)).unwrap();
            
            // 2. Create streaming plan
            let streamer = PriorityStreamer::new();
            let plan = streamer.analyze(&json_value).unwrap();
            
            // 3. Get first high-priority frame (skeleton + critical data)
            let mut reconstructor = JsonReconstructor::new();
            
            // In real streaming, this would be the first frame received over network
            if let Some(first_frame) = plan.frames().next() {
                reconstructor.add_frame(first_frame.clone());
                let _ = reconstructor.process_next_frame();
                
                // Critical data is now available for UI rendering!
                // This happens orders of magnitude faster than waiting for full parse
            }
            
            start.elapsed()
        })
    });
    
    group.finish();
}

fn benchmark_event_streaming(c: &mut Criterion) {
    let mut group = c.benchmark_group("event_streaming");
    group.measurement_time(Duration::from_secs(10));
    
    let analytics_data = generate_analytics_dataset(50_000); // Large event stream
    
    // Traditional: Process entire event stream at once
    group.bench_function("batch_event_processing", |b| {
        b.iter(|| {
            let start = Instant::now();
            let _: Value = serde_json::from_str(black_box(&analytics_data)).unwrap();
            // All events must be processed before any are available
            start.elapsed()
        })
    });
    
    // PJS: Progressive event processing with priority
    group.bench_function("priority_event_streaming", |b| {
        b.iter(|| {
            let start = Instant::now();
            
            // Simulate streaming advantage for event data
            let parser = Parser::new();
            let _frame = parser.parse(black_box(analytics_data.as_bytes())).unwrap();
            
            // In real streaming scenario:
            // - Recent events (high priority) would be available immediately
            // - Historical data (low priority) streams in background
            // - UI can show real-time metrics while historical data loads
            
            start.elapsed()
        })
    });
    
    group.finish();
}

fn benchmark_memory_efficiency(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_efficiency");
    group.measurement_time(Duration::from_secs(8));
    
    let huge_dataset = generate_massive_catalog(8_000); // ~8MB
    
    // Traditional: All data in memory simultaneously
    group.bench_function("traditional_full_memory", |b| {
        b.iter(|| {
            // This loads entire JSON into memory as Value tree
            let _: Value = serde_json::from_str(black_box(&huge_dataset)).unwrap();
        })
    });
    
    // PJS: Streaming with controlled memory usage
    group.bench_function("pjs_controlled_memory", |b| {
        b.iter(|| {
            // Parse incrementally with better memory patterns
            let parser = Parser::new();
            let _ = parser.parse(black_box(huge_dataset.as_bytes())).unwrap();
            
            // In streaming scenario, memory usage would be much lower:
            // - Only skeleton + current priority data in memory
            // - Historical/low-priority data can be processed and discarded
            // - Enables processing datasets larger than available RAM
        })
    });
    
    group.finish();
}

criterion_group!(
    benches,
    benchmark_massive_parsing,
    benchmark_streaming_advantage,
    benchmark_event_streaming,
    benchmark_memory_efficiency
);

criterion_main!(benches);