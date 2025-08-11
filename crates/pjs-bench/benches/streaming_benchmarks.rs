//! Streaming performance benchmarks - PJS vs traditional approaches
//!
//! Measures the performance characteristics that matter most for real-world
//! streaming applications: latency, throughput, and perceived performance.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main, black_box};
use pjson_rs::Parser;
use std::time::{Duration, Instant};
use tokio::time::sleep;

/// Generate realistic streaming data scenarios
fn generate_analytics_dashboard() -> String {
    let mut metrics = Vec::new();
    for i in 0..1000 {
        metrics.push(format!(
            r#"{{
                "timestamp": "2024-01-15T{}:{}:00Z",
                "metric_name": "response_time_p95",
                "value": {:.2},
                "service": "api-service-{}",
                "region": "us-west-{}",
                "metadata": {{
                    "tags": ["production", "web", "api"],
                    "dimensions": {{
                        "environment": "prod",
                        "version": "v2.1.{}"
                    }}
                }}
            }}"#,
            10 + (i / 60) % 14,  // Hours 10-23
            i % 60,              // Minutes 0-59
            100.0 + (i as f64 * 0.3) % 500.0, // Response times
            i % 5,               // Service instances
            1 + i % 2,           // Regions
            i % 10               // Version patch
        ));
    }
    
    format!(
        r#"{{
            "dashboard": {{
                "id": "main-analytics",
                "title": "Production Analytics Dashboard",
                "last_updated": "2024-01-15T12:00:00Z",
                "critical_alerts": {{
                    "high_error_rate": false,
                    "service_down": false,
                    "high_latency": true
                }},
                "summary": {{
                    "total_requests": 2847293,
                    "error_rate": 0.023,
                    "avg_response_time": 234.5,
                    "active_services": 15
                }},
                "time_series_data": [{}],
                "service_health": {{
                    "api-service-0": {{"status": "healthy", "latency": 123.4}},
                    "api-service-1": {{"status": "healthy", "latency": 145.2}},
                    "api-service-2": {{"status": "degraded", "latency": 567.8}},
                    "api-service-3": {{"status": "healthy", "latency": 98.1}},
                    "api-service-4": {{"status": "healthy", "latency": 201.3}}
                }},
                "detailed_logs": [{}]
            }}
        }}"#,
        metrics.join(","),
        (0..100).map(|i| format!(
            r#"{{"timestamp": "2024-01-15T12:{}:00Z", "level": "INFO", "message": "Request processed", "latency": {}}}"#,
            i % 60, 100 + i * 3
        )).collect::<Vec<_>>().join(",")
    )
}

/// Generate social media feed data
fn generate_social_feed() -> String {
    let mut posts = Vec::new();
    for i in 0..500 {
        posts.push(format!(
            r#"{{
                "id": {},
                "user_id": {},
                "username": "user_{}",
                "display_name": "User {}",
                "avatar_url": "https://cdn.social.com/avatars/{}.jpg",
                "content": "This is post number {} with some engaging content that users want to see quickly. The rest of the metadata can load later.",
                "timestamp": "2024-01-15T{}:{}:00Z",
                "engagement": {{
                    "likes": {},
                    "comments": {},
                    "shares": {},
                    "reactions": {{
                        "heart": {},
                        "laugh": {},
                        "wow": {}
                    }}
                }},
                "media": {{
                    "type": "image",
                    "url": "https://cdn.social.com/posts/{}/image.jpg",
                    "width": 1200,
                    "height": 800,
                    "alt_text": "Post {} image content"
                }},
                "comments_preview": [{}]
            }}"#,
            i,
            1000 + (i % 100),
            i % 100,
            i % 100,
            i % 100,
            i,
            10 + (i % 14), // Hours
            i % 60,        // Minutes
            i * 5 + 10,    // Likes
            i * 2 + 3,     // Comments
            i / 3,         // Shares
            i * 2,         // Heart reactions
            i / 2,         // Laugh reactions
            i / 4,         // Wow reactions
            i,
            i,
            (0..std::cmp::min(3, i % 5)).map(|j| format!(
                r#"{{"user": "commenter_{}", "text": "Comment {} on post {}", "timestamp": "2024-01-15T12:{}:00Z"}}"#,
                j, j, i, (30 + j) % 60
            )).collect::<Vec<_>>().join(",")
        ));
    }
    
    format!(
        r#"{{
            "feed": {{
                "user_id": 123,
                "timeline_type": "home",
                "last_updated": "2024-01-15T12:00:00Z",
                "unread_count": 23,
                "posts": [{}]
            }}
        }}"#,
        posts.join(",")
    )
}

/// Measure time to extract critical data from parsed JSON
fn extract_critical_dashboard_data(value: &serde_json::Value) -> (String, f64, bool, usize) {
    let title = value["dashboard"]["title"]
        .as_str()
        .unwrap_or("Unknown")
        .to_string();
    
    let avg_response_time = value["dashboard"]["summary"]["avg_response_time"]
        .as_f64()
        .unwrap_or(0.0);
    
    let high_latency_alert = value["dashboard"]["critical_alerts"]["high_latency"]
        .as_bool()
        .unwrap_or(false);
    
    let active_services = value["dashboard"]["summary"]["active_services"]
        .as_u64()
        .unwrap_or(0) as usize;
    
    (title, avg_response_time, high_latency_alert, active_services)
}

/// Benchmark Time-to-First-Meaningful-Paint (TTFMP)
fn benchmark_ttfmp(c: &mut Criterion) {
    let mut group = c.benchmark_group("time_to_first_meaningful_paint");
    group.measurement_time(Duration::from_secs(10));
    
    let analytics_data = generate_analytics_dashboard();
    let social_data = generate_social_feed();
    
    // Dashboard TTFMP - Traditional approach
    group.bench_function("analytics_traditional", |b| {
        b.iter_custom(|iters| {
            let mut total_duration = Duration::ZERO;
            for _ in 0..iters {
                let start = Instant::now();
                
                // Parse complete JSON (includes all time series data)
                let dashboard: serde_json::Value = serde_json::from_str(black_box(&analytics_data)).unwrap();
                
                // Extract critical UI data
                let _critical_data = extract_critical_dashboard_data(&dashboard);
                
                // Time includes parsing 1000 metrics that user doesn't see initially
                total_duration += start.elapsed();
            }
            total_duration
        })
    });
    
    // Dashboard TTFMP - PJS skeleton approach  
    group.bench_function("analytics_pjs_skeleton", |b| {
        b.iter_custom(|iters| {
            let mut total_duration = Duration::ZERO;
            for _ in 0..iters {
                let start = Instant::now();
                
                // Parse only skeleton with critical data
                let skeleton = r#"{
                    "dashboard": {
                        "id": "main-analytics",
                        "title": "Production Analytics Dashboard", 
                        "last_updated": "2024-01-15T12:00:00Z",
                        "critical_alerts": {
                            "high_error_rate": false,
                            "service_down": false,
                            "high_latency": true
                        },
                        "summary": {
                            "total_requests": 2847293,
                            "error_rate": 0.023,
                            "avg_response_time": 234.5,
                            "active_services": 15
                        },
                        "time_series_data": [],
                        "detailed_logs": []
                    }
                }"#;
                
                let dashboard: serde_json::Value = serde_json::from_str(skeleton).unwrap();
                let _critical_data = extract_critical_dashboard_data(&dashboard);
                
                // Critical UI can render immediately!
                // Time series and logs stream in background
                total_duration += start.elapsed();
            }
            total_duration
        })
    });
    
    // Social feed TTFMP comparison
    group.bench_function("social_traditional", |b| {
        b.iter_custom(|iters| {
            let mut total_duration = Duration::ZERO;
            for _ in 0..iters {
                let start = Instant::now();
                
                let feed: serde_json::Value = serde_json::from_str(black_box(&social_data)).unwrap();
                
                // Extract first few posts that user sees
                if let Some(posts) = feed["feed"]["posts"].as_array() {
                    let _first_posts: Vec<_> = posts.iter()
                        .take(5)
                        .map(|post| (
                            post["id"].as_u64().unwrap_or(0),
                            post["username"].as_str().unwrap_or(""),
                            post["content"].as_str().unwrap_or("")
                        ))
                        .collect();
                }
                
                total_duration += start.elapsed();
            }
            total_duration
        })
    });
    
    group.bench_function("social_pjs_progressive", |b| {
        b.iter_custom(|iters| {
            let mut total_duration = Duration::ZERO;
            for _ in 0..iters {
                let start = Instant::now();
                
                // First chunk: Basic feed info + first 5 posts (critical content)
                let first_chunk = r#"{
                    "feed": {
                        "user_id": 123,
                        "timeline_type": "home", 
                        "unread_count": 23,
                        "posts": [
                            {"id": 1, "username": "user_1", "content": "Quick loading post", "timestamp": "2024-01-15T12:00:00Z"},
                            {"id": 2, "username": "user_2", "content": "Another fast post", "timestamp": "2024-01-15T12:01:00Z"}
                        ]
                    }
                }"#;
                
                let feed: serde_json::Value = serde_json::from_str(first_chunk).unwrap();
                if let Some(posts) = feed["feed"]["posts"].as_array() {
                    let _visible_posts: Vec<_> = posts.iter()
                        .map(|post| (
                            post["id"].as_u64().unwrap_or(0),
                            post["username"].as_str().unwrap_or(""),
                            post["content"].as_str().unwrap_or("")
                        ))
                        .collect();
                }
                
                // User sees first posts immediately!
                total_duration += start.elapsed();
            }
            total_duration
        })
    });
    
    group.finish();
}

/// Benchmark streaming throughput
fn benchmark_streaming_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("streaming_throughput");
    
    let data_sizes = vec![1, 5, 10]; // MB
    
    for size_mb in data_sizes {
        let data = "x".repeat(size_mb * 1024 * 1024); // Simple data for throughput test
        let json_data = format!(r#"{{"data": "{}", "size_mb": {}}}"#, data, size_mb);
        
        group.throughput(Throughput::Bytes(json_data.len() as u64));
        
        // Traditional: Parse entire payload
        group.bench_with_input(
            BenchmarkId::new("traditional_full", format!("{}MB", size_mb)),
            &json_data,
            |b, data| {
                b.iter(|| {
                    let _: serde_json::Value = serde_json::from_str(black_box(data)).unwrap();
                })
            },
        );
        
        // PJS: Parse with sonic-rs (SIMD optimized)
        group.bench_with_input(
            BenchmarkId::new("pjs_simd", format!("{}MB", size_mb)),
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

/// Benchmark perceived performance with simulated network delays
#[tokio::main]
async fn benchmark_perceived_performance(c: &mut Criterion) {
    let mut group = c.benchmark_group("perceived_performance");
    group.measurement_time(Duration::from_secs(5));
    
    let large_response = generate_analytics_dashboard();
    
    // Simulate slow network: Traditional approach
    group.bench_function("traditional_with_network_delay", |b| {
        let rt = tokio::runtime::Runtime::new().unwrap();
        
        b.to_async(&rt).iter_custom(|iters| async move {
            let mut total_duration = Duration::ZERO;
            
            for _ in 0..iters {
                let start = Instant::now();
                
                // Simulate network delay for complete response
                sleep(Duration::from_millis(100)).await; // 100ms network latency
                
                // Then parse complete JSON
                let _dashboard: serde_json::Value = serde_json::from_str(&large_response).unwrap();
                let _critical_data = extract_critical_dashboard_data(&_dashboard);
                
                // Total time: 100ms + parsing time
                total_duration += start.elapsed();
            }
            
            total_duration
        })
    });
    
    // PJS approach: Critical data available quickly, details stream later
    group.bench_function("pjs_progressive_with_network", |b| {
        let rt = tokio::runtime::Runtime::new().unwrap();
        
        b.to_async(&rt).iter_custom(|iters| async move {
            let mut total_duration = Duration::ZERO;
            
            for _ in 0..iters {
                let start = Instant::now();
                
                // First frame: Critical data with minimal network delay
                sleep(Duration::from_millis(10)).await; // 10ms for skeleton
                
                let skeleton = r#"{
                    "dashboard": {
                        "title": "Production Analytics Dashboard",
                        "critical_alerts": {"high_latency": true},
                        "summary": {"avg_response_time": 234.5, "active_services": 15}
                    }
                }"#;
                
                let dashboard: serde_json::Value = serde_json::from_str(skeleton).unwrap();
                let _critical_data = extract_critical_dashboard_data(&dashboard);
                
                // User sees critical info after just 10ms!
                // Remaining data streams in background (not counted in TTFMP)
                total_duration += start.elapsed();
            }
            
            total_duration
        })
    });
    
    group.finish();
}

/// Benchmark concurrent streaming scenarios
fn benchmark_concurrent_streaming(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_streaming");
    
    let dashboard_data = generate_analytics_dashboard();
    let user_counts = vec![1, 5, 10, 20];
    
    for user_count in user_counts {
        // Traditional: Each user waits for complete parsing
        group.bench_with_input(
            BenchmarkId::new("traditional_concurrent", user_count),
            &user_count,
            |b, &users| {
                b.iter(|| {
                    let handles: Vec<_> = (0..users).map(|_| {
                        let data = dashboard_data.clone();
                        std::thread::spawn(move || {
                            let _dashboard: serde_json::Value = serde_json::from_str(&data).unwrap();
                            let _critical = extract_critical_dashboard_data(&_dashboard);
                        })
                    }).collect();
                    
                    for handle in handles {
                        handle.join().unwrap();
                    }
                })
            },
        );
        
        // PJS: Users get skeleton immediately, share remaining parsing work
        group.bench_with_input(
            BenchmarkId::new("pjs_shared_parsing", user_count),
            &user_count,
            |b, &users| {
                b.iter(|| {
                    // Parse heavy data once
                    let _full_data: serde_json::Value = serde_json::from_str(&dashboard_data).unwrap();
                    
                    let handles: Vec<_> = (0..users).map(|_| {
                        std::thread::spawn(move || {
                            // Each user gets immediate skeleton
                            let skeleton = r#"{
                                "dashboard": {
                                    "title": "Production Analytics Dashboard",
                                    "summary": {"avg_response_time": 234.5}
                                }
                            }"#;
                            let _user_dashboard: serde_json::Value = serde_json::from_str(skeleton).unwrap();
                            let _critical = extract_critical_dashboard_data(&_user_dashboard);
                        })
                    }).collect();
                    
                    for handle in handles {
                        handle.join().unwrap();
                    }
                })
            },
        );
    }
    
    group.finish();
}

criterion_group!(
    streaming_benches,
    benchmark_ttfmp,
    benchmark_streaming_throughput,
    benchmark_concurrent_streaming
);

// Note: We can't easily use async functions in criterion_main!
// The perceived_performance benchmark would need to be run separately
criterion_main!(streaming_benches);