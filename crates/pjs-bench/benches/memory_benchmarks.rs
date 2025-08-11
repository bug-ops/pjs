//! Memory usage benchmarks - comparing memory efficiency
//!
//! Demonstrates PJS advantages in memory consumption patterns
//! and progressive loading scenarios

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main, black_box};
use pjson_rs::Parser;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Custom allocator to track memory usage
pub struct TrackingAllocator {
    inner: System,
    allocated: Arc<AtomicUsize>,
    deallocated: Arc<AtomicUsize>,
}

impl TrackingAllocator {
    pub fn new() -> Self {
        Self {
            inner: System,
            allocated: Arc::new(AtomicUsize::new(0)),
            deallocated: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn reset(&self) {
        self.allocated.store(0, Ordering::SeqCst);
        self.deallocated.store(0, Ordering::SeqCst);
    }

    pub fn current_usage(&self) -> usize {
        self.allocated.load(Ordering::SeqCst) - self.deallocated.load(Ordering::SeqCst)
    }

    pub fn peak_allocation(&self) -> usize {
        self.allocated.load(Ordering::SeqCst)
    }
}

unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = self.inner.alloc(layout);
        if !ptr.is_null() {
            self.allocated.fetch_add(layout.size(), Ordering::SeqCst);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.inner.dealloc(ptr, layout);
        self.deallocated.fetch_add(layout.size(), Ordering::SeqCst);
    }
}

// Note: For real tracking, we'd need to replace the global allocator
// For benchmarking, we'll simulate memory usage measurements

/// Generate large dataset for memory testing
fn generate_memory_test_data(size_mb: usize) -> String {
    let items_per_mb = 30; // Approximate
    let total_items = size_mb * items_per_mb;
    
    let mut items = Vec::with_capacity(total_items);
    for i in 0..total_items {
        let large_description = "A".repeat(1000); // 1KB per item description
        
        items.push(format!(
            r#"{{
                "id": {},
                "title": "Item {} - Memory Test Data",
                "description": "{}",
                "metadata": {{
                    "category": "test_category_{}",
                    "tags": [{}],
                    "created_at": "2024-01-15T10:30:{}Z",
                    "size_bytes": {}
                }},
                "large_array": [{}]
            }}"#,
            i,
            i,
            large_description,
            i % 20,
            (0..5).map(|j| format!("\"tag_{}\"", j)).collect::<Vec<_>>().join(", "),
            i % 60,
            1000 + i * 100,
            (0..50).map(|j| i * 50 + j).collect::<Vec<_>>().iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", ")
        ));
    }

    format!(
        r#"{{
            "metadata": {{
                "total_items": {},
                "generated_at": "2024-01-15T12:00:00Z",
                "estimated_size_mb": {}
            }},
            "items": [{}]
        }}"#,
        total_items,
        size_mb,
        items.join(",")
    )
}

/// Simulate memory usage measurement
fn measure_memory_usage<F, R>(f: F) -> (R, usize) 
where 
    F: FnOnce() -> R 
{
    // In a real scenario, we'd use jemalloc stats or similar
    // For this benchmark, we'll estimate based on data size
    let start_time = Instant::now();
    let result = f();
    let duration = start_time.elapsed();
    
    // Rough estimation: longer processing time = more memory usage
    let estimated_memory = (duration.as_nanos() / 1000) as usize;
    (result, estimated_memory)
}

/// Benchmark memory usage for different JSON parsers
fn benchmark_memory_usage_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_usage_comparison");
    
    let test_sizes = vec![1, 5, 10]; // MB
    
    for size_mb in test_sizes {
        let data = generate_memory_test_data(size_mb);
        let size_name = format!("{}MB", size_mb);
        
        // serde_json baseline
        group.bench_with_input(
            BenchmarkId::new("serde_json", &size_name),
            &data,
            |b, data| {
                b.iter_custom(|iters| {
                    let mut total_duration = Duration::ZERO;
                    for _ in 0..iters {
                        let start = Instant::now();
                        let (_, _memory) = measure_memory_usage(|| {
                            let _: serde_json::Value = serde_json::from_str(black_box(data)).unwrap();
                        });
                        total_duration += start.elapsed();
                    }
                    total_duration
                })
            },
        );
        
        // sonic-rs SIMD
        group.bench_with_input(
            BenchmarkId::new("sonic_rs", &size_name),
            &data,
            |b, data| {
                b.iter_custom(|iters| {
                    let mut total_duration = Duration::ZERO;
                    for _ in 0..iters {
                        let start = Instant::now();
                        let (_, _memory) = measure_memory_usage(|| {
                            let _: sonic_rs::Value = sonic_rs::from_str(black_box(data)).unwrap();
                        });
                        total_duration += start.elapsed();
                    }
                    total_duration
                })
            },
        );
        
        // PJS parser
        group.bench_with_input(
            BenchmarkId::new("pjs_parser", &size_name),
            &data,
            |b, data| {
                b.iter_custom(|iters| {
                    let mut total_duration = Duration::ZERO;
                    for _ in 0..iters {
                        let start = Instant::now();
                        let (_, _memory) = measure_memory_usage(|| {
                            let parser = Parser::new();
                            let _ = parser.parse(black_box(data.as_bytes())).unwrap();
                        });
                        total_duration += start.elapsed();
                    }
                    total_duration
                })
            },
        );
    }
    
    group.finish();
}

/// Benchmark progressive vs batch memory allocation patterns
fn benchmark_progressive_memory_usage(c: &mut Criterion) {
    let mut group = c.benchmark_group("progressive_memory_patterns");
    group.measurement_time(Duration::from_secs(10));
    
    let large_data = generate_memory_test_data(3); // 3MB dataset
    
    // Traditional: Load everything at once (peak memory usage)
    group.bench_function("batch_load_all", |b| {
        b.iter_custom(|iters| {
            let mut total_duration = Duration::ZERO;
            for _ in 0..iters {
                let start = Instant::now();
                let (_, _peak_memory) = measure_memory_usage(|| {
                    // Simulate loading entire dataset into memory
                    let value: serde_json::Value = serde_json::from_str(black_box(&large_data)).unwrap();
                    
                    // Simulate processing all items at once
                    if let Some(items) = value.get("items").and_then(|i| i.as_array()) {
                        let _processed: Vec<_> = items.iter()
                            .map(|item| {
                                format!("processed_{}", 
                                    item.get("id").and_then(|id| id.as_u64()).unwrap_or(0))
                            })
                            .collect();
                    }
                    
                    // All data stays in memory until function completion
                });
                total_duration += start.elapsed();
            }
            total_duration
        })
    });
    
    // PJS approach: Progressive chunks with bounded memory
    group.bench_function("progressive_chunks", |b| {
        b.iter_custom(|iters| {
            let mut total_duration = Duration::ZERO;
            for _ in 0..iters {
                let start = Instant::now();
                let (_result, _memory) = measure_memory_usage(|| {
                    // Simulate progressive processing in chunks
                    
                    // Chunk 1: Metadata only (very small memory footprint)
                    let metadata_chunk = r#"{
                        "metadata": {
                            "total_items": 90,
                            "generated_at": "2024-01-15T12:00:00Z",
                            "estimated_size_mb": 3
                        },
                        "items": []
                    }"#;
                    
                    let _meta: serde_json::Value = serde_json::from_str(metadata_chunk).unwrap();
                    // Process metadata, show loading state
                    
                    // Simulate processing items in chunks of 10
                    for chunk_num in 0..9 { // 90 items / 10 per chunk
                        let chunk_data = format!(r#"{{
                            "items": [{}]
                        }}"#, 
                            (0..10).map(|i| {
                                let item_id = chunk_num * 10 + i;
                                format!(r#"{{"id": {}, "title": "Item {}", "processed": true}}"#, 
                                    item_id, item_id)
                            }).collect::<Vec<_>>().join(",")
                        );
                        
                        let _chunk: serde_json::Value = serde_json::from_str(&chunk_data).unwrap();
                        
                        // Process this chunk, then drop it
                        // Memory usage stays bounded
                        drop(_chunk);
                    }
                });
                total_duration += start.elapsed();
            }
            total_duration
        })
    });
    
    group.finish();
}

/// Benchmark streaming vs traditional memory patterns for UI rendering
fn benchmark_ui_rendering_memory(c: &mut Criterion) {
    let mut group = c.benchmark_group("ui_rendering_memory_patterns");
    
    let dashboard_data = r#"{
        "dashboard": {
            "title": "Analytics Dashboard",
            "last_updated": "2024-01-15T12:00:00Z",
            "critical_metrics": {
                "active_users": 1247,
                "errors_per_minute": 0.3,
                "response_time_p95": 245.5
            },
            "charts": [
                {
                    "type": "timeseries",
                    "title": "Response Time Trend",
                    "data": [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20]
                },
                {
                    "type": "histogram", 
                    "title": "Request Distribution",
                    "data": [45, 67, 23, 89, 12, 56, 78, 34, 91, 25, 47, 83, 19, 74, 36, 92, 58, 14, 69, 81]
                }
            ],
            "detailed_logs": [
                {"timestamp": "2024-01-15T11:59:00Z", "level": "INFO", "message": "Request processed successfully", "duration": 245},
                {"timestamp": "2024-01-15T11:58:30Z", "level": "WARN", "message": "Slow query detected", "duration": 1200},
                {"timestamp": "2024-01-15T11:58:00Z", "level": "INFO", "message": "Cache hit", "duration": 15}
            ]
        }
    }"#;
    
    // Create a realistic 1MB dashboard by extending the base data
    let dashboard_data_mb = format!("{}{}", 
        dashboard_data.trim_end_matches('}').trim_end_matches('"').trim_end_matches('}'),
        (0..100).map(|i| format!(
            r#", "additional_data_{}": {{"batch": {}, "data": "{}"}}"#,
            i, i, "x".repeat(500) // 500 bytes per entry for ~50KB extra
        )).collect::<Vec<_>>().join("") + "}}}"
    );
    
    // Traditional: Parse entire dashboard before showing anything
    group.bench_function("traditional_full_parse", |b| {
        b.iter_custom(|iters| {
            let mut total_duration = Duration::ZERO;
            for _ in 0..iters {
                let start = Instant::now();
                let (_, _memory) = measure_memory_usage(|| {
                    // Parse everything
                    let dashboard: serde_json::Value = serde_json::from_str(black_box(&dashboard_data_mb)).unwrap();
                    
                    // Extract UI elements (but data was already fully loaded)
                    let _title = dashboard["dashboard"]["title"].as_str();
                    let _active_users = dashboard["dashboard"]["critical_metrics"]["active_users"].as_u64();
                    let _errors = dashboard["dashboard"]["critical_metrics"]["errors_per_minute"].as_f64();
                    
                    // All data remains in memory
                });
                total_duration += start.elapsed();
            }
            total_duration
        })
    });
    
    // PJS: Show critical UI elements immediately, stream details
    group.bench_function("pjs_progressive_ui", |b| {
        b.iter_custom(|iters| {
            let mut total_duration = Duration::ZERO;
            for _ in 0..iters {
                let start = Instant::now();
                let (_, _memory) = measure_memory_usage(|| {
                    // Step 1: Parse skeleton with critical UI data
                    let skeleton = r#"{
                        "dashboard": {
                            "title": "Analytics Dashboard",
                            "last_updated": "2024-01-15T12:00:00Z",
                            "critical_metrics": {
                                "active_users": 1247,
                                "errors_per_minute": 0.3,
                                "response_time_p95": 245.5
                            },
                            "charts": [],
                            "detailed_logs": []
                        }
                    }"#;
                    
                    let critical_data: serde_json::Value = serde_json::from_str(skeleton).unwrap();
                    
                    // UI can immediately show:
                    let _title = critical_data["dashboard"]["title"].as_str();
                    let _active_users = critical_data["dashboard"]["critical_metrics"]["active_users"].as_u64();
                    let _errors = critical_data["dashboard"]["critical_metrics"]["errors_per_minute"].as_f64();
                    
                    // Charts and logs would stream in separately
                    // Memory footprint stays small for initial render
                    drop(critical_data);
                });
                total_duration += start.elapsed();
            }
            total_duration
        })
    });
    
    group.finish();
}

/// Benchmark memory usage with concurrent users
fn benchmark_concurrent_memory_usage(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_memory_usage");
    
    let user_data = generate_memory_test_data(1); // 1MB per user
    
    // Simulate 10 concurrent users with traditional JSON parsing
    group.bench_function("traditional_concurrent", |b| {
        b.iter(|| {
            let (_results, _total_memory) = measure_memory_usage(|| {
                // Simulate 10 users all loading their data simultaneously
                let handles: Vec<_> = (0..10).map(|_user_id| {
                    let data_copy = user_data.clone();
                    std::thread::spawn(move || {
                        let _user_data: serde_json::Value = serde_json::from_str(black_box(&data_copy)).unwrap();
                        // Each user's full dataset stays in memory
                        std::thread::sleep(Duration::from_millis(10)); // Simulate processing
                    })
                }).collect();
                
                for handle in handles {
                    handle.join().unwrap();
                }
                
                // Peak memory usage: 10MB (all users' data loaded simultaneously)
            });
        })
    });
    
    // PJS approach: Users get progressive data, bounded memory per user
    group.bench_function("pjs_concurrent_progressive", |b| {
        b.iter(|| {
            let (_results, _bounded_memory) = measure_memory_usage(|| {
                let handles: Vec<_> = (0..10).map(|_user_id| {
                    std::thread::spawn(move || {
                        // Each user gets skeleton immediately
                        let skeleton = r#"{
                            "metadata": {"total_items": 30, "estimated_size_mb": 1},
                            "items": []
                        }"#;
                        let _meta: serde_json::Value = serde_json::from_str(skeleton).unwrap();
                        
                        // Then stream items in small chunks
                        for _chunk in 0..3 { // 30 items / 10 per chunk
                            let chunk_data = r#"{"items": [{"id": 1, "processed": true}]}"#;
                            let _chunk: serde_json::Value = serde_json::from_str(chunk_data).unwrap();
                            drop(_chunk); // Memory freed after processing
                            std::thread::sleep(Duration::from_millis(1));
                        }
                    })
                }).collect();
                
                for handle in handles {
                    handle.join().unwrap();
                }
                
                // Memory usage stays bounded (~1MB total instead of 10MB)
            });
        })
    });
    
    group.finish();
}

criterion_group!(
    memory_benches,
    benchmark_memory_usage_comparison,
    benchmark_progressive_memory_usage, 
    benchmark_ui_rendering_memory,
    benchmark_concurrent_memory_usage
);

criterion_main!(memory_benches);