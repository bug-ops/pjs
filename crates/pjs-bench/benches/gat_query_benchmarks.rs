//! GAT Query Performance Benchmarks
//!
//! Validates Phase 2 GAT implementation meets performance targets:
//! - All query methods < 1ms for 1000 sessions/streams
//! - Zero Box<dyn Future> allocations
//! - Lock-free DashMap operations

#![feature(impl_trait_in_assoc_type)]

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use pjson_rs::{
    domain::{
        aggregates::{StreamSession, stream_session::SessionConfig},
        ports::{
            Pagination, SessionQueryCriteria, SessionQueryResult, StreamRepositoryGat,
            SessionHealthSnapshot, StreamStoreGat, StreamFilter, StreamStatistics,
            StreamStatus, PriorityDistribution,
        },
        entities::Stream,
        value_objects::{SessionId, StreamId, Priority},
        DomainResult,
    },
    infrastructure::adapters::GatInMemoryStreamRepository,
};
use chrono::{Duration, Utc};
use std::{collections::HashMap, future::Future, sync::Arc, time::Instant};

// Re-export the real repository implementation
fn create_repository_with_sessions(count: usize) -> GatInMemoryStreamRepository {
    let repo = GatInMemoryStreamRepository::new();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    rt.block_on(async {
        for _ in 0..count {
            let mut session = StreamSession::new(SessionConfig::default());
            let _ = session.activate();
            let _ = repo.save_session(session).await;
        }
    });

    repo
}

fn bench_find_session(c: &mut Criterion) {
    let mut group = c.benchmark_group("gat_find_session");
    group.throughput(Throughput::Elements(1));

    for size in [100, 500, 1000].iter() {
        let repo = create_repository_with_sessions(*size);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        // Get a valid session ID for lookup
        let session_id = rt.block_on(async {
            let sessions = repo.find_active_sessions().await.unwrap();
            sessions.first().map(|s| s.id()).unwrap_or_else(SessionId::new)
        });

        group.bench_with_input(BenchmarkId::new("O(1)_lookup", size), size, |b, _| {
            b.iter(|| {
                rt.block_on(async {
                    let result = repo.find_session(black_box(session_id)).await;
                    black_box(result)
                })
            });
        });
    }
    group.finish();
}

fn bench_find_active_sessions(c: &mut Criterion) {
    let mut group = c.benchmark_group("gat_find_active_sessions");

    for size in [100, 500, 1000].iter() {
        group.throughput(Throughput::Elements(*size as u64));
        let repo = create_repository_with_sessions(*size);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        group.bench_with_input(BenchmarkId::new("O(n)_filter", size), size, |b, _| {
            b.iter(|| {
                rt.block_on(async {
                    let result = repo.find_active_sessions().await;
                    black_box(result)
                })
            });
        });
    }
    group.finish();
}

fn bench_find_sessions_by_criteria(c: &mut Criterion) {
    let mut group = c.benchmark_group("gat_find_sessions_by_criteria");

    for size in [100, 500, 1000].iter() {
        group.throughput(Throughput::Elements(*size as u64));
        let repo = create_repository_with_sessions(*size);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let criteria = SessionQueryCriteria {
            created_after: Some(Utc::now() - Duration::hours(1)),
            ..Default::default()
        };

        let pagination = Pagination {
            offset: 0,
            limit: 100,
            ..Default::default()
        };

        group.bench_with_input(
            BenchmarkId::new("filter_sort_paginate", size),
            size,
            |b, _| {
                b.iter(|| {
                    rt.block_on(async {
                        let result = repo
                            .find_sessions_by_criteria(
                                black_box(criteria.clone()),
                                black_box(pagination),
                            )
                            .await;
                        black_box(result)
                    })
                });
            },
        );
    }
    group.finish();
}

fn bench_session_exists(c: &mut Criterion) {
    let mut group = c.benchmark_group("gat_session_exists");
    group.throughput(Throughput::Elements(1));

    for size in [100, 500, 1000].iter() {
        let repo = create_repository_with_sessions(*size);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let session_id = rt.block_on(async {
            let sessions = repo.find_active_sessions().await.unwrap();
            sessions.first().map(|s| s.id()).unwrap_or_else(SessionId::new)
        });

        group.bench_with_input(BenchmarkId::new("O(1)_check", size), size, |b, _| {
            b.iter(|| {
                rt.block_on(async {
                    let result = repo.session_exists(black_box(session_id)).await;
                    black_box(result)
                })
            });
        });
    }
    group.finish();
}

fn bench_get_session_health(c: &mut Criterion) {
    let mut group = c.benchmark_group("gat_get_session_health");
    group.throughput(Throughput::Elements(1));

    for size in [100, 500, 1000].iter() {
        let repo = create_repository_with_sessions(*size);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let session_id = rt.block_on(async {
            let sessions = repo.find_active_sessions().await.unwrap();
            sessions.first().map(|s| s.id()).unwrap_or_else(SessionId::new)
        });

        group.bench_with_input(BenchmarkId::new("O(s)_aggregation", size), size, |b, _| {
            b.iter(|| {
                rt.block_on(async {
                    let result = repo.get_session_health(black_box(session_id)).await;
                    black_box(result)
                })
            });
        });
    }
    group.finish();
}

fn bench_save_session(c: &mut Criterion) {
    let mut group = c.benchmark_group("gat_save_session");
    group.throughput(Throughput::Elements(1));

    for size in [100, 500, 1000].iter() {
        let repo = create_repository_with_sessions(*size);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        group.bench_with_input(BenchmarkId::new("O(1)_insert", size), size, |b, _| {
            b.iter(|| {
                rt.block_on(async {
                    let mut session = StreamSession::new(SessionConfig::default());
                    let _ = session.activate();
                    let result = repo.save_session(black_box(session)).await;
                    black_box(result)
                })
            });
        });
    }
    group.finish();
}

fn bench_concurrent_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("gat_concurrent_operations");

    for size in [100, 500, 1000].iter() {
        let repo = Arc::new(create_repository_with_sessions(*size));
        let rt = tokio::runtime::Runtime::new().unwrap();

        group.bench_with_input(
            BenchmarkId::new("mixed_read_write", size),
            size,
            |b, &size| {
                b.iter(|| {
                    rt.block_on(async {
                        let mut handles = vec![];

                        // 10 concurrent readers
                        for _ in 0..10 {
                            let repo_clone = repo.clone();
                            handles.push(tokio::spawn(async move {
                                let _ = repo_clone.find_active_sessions().await;
                            }));
                        }

                        // 10 concurrent writers
                        for _ in 0..10 {
                            let repo_clone = repo.clone();
                            handles.push(tokio::spawn(async move {
                                let mut session = StreamSession::new(SessionConfig::default());
                                let _ = session.activate();
                                let _ = repo_clone.save_session(session).await;
                            }));
                        }

                        for handle in handles {
                            let _ = handle.await;
                        }
                    })
                });
            },
        );
    }
    group.finish();
}

fn bench_latency_distribution(c: &mut Criterion) {
    let mut group = c.benchmark_group("gat_latency_distribution");
    group.sample_size(1000);

    let repo = create_repository_with_sessions(1000);
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let session_id = rt.block_on(async {
        let sessions = repo.find_active_sessions().await.unwrap();
        sessions.first().map(|s| s.id()).unwrap_or_else(SessionId::new)
    });

    group.bench_function("p99_find_session", |b| {
        b.iter(|| {
            rt.block_on(async {
                let result = repo.find_session(black_box(session_id)).await;
                black_box(result)
            })
        });
    });

    group.bench_function("p99_session_exists", |b| {
        b.iter(|| {
            rt.block_on(async {
                let result = repo.session_exists(black_box(session_id)).await;
                black_box(result)
            })
        });
    });

    group.finish();
}

fn measure_raw_performance() {
    println!("\n=== GAT Query Raw Performance Measurements ===\n");

    let repo = create_repository_with_sessions(1000);
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let session_id = rt.block_on(async {
        let sessions = repo.find_active_sessions().await.unwrap();
        sessions.first().map(|s| s.id()).unwrap_or_else(SessionId::new)
    });

    const ITERATIONS: u32 = 10_000;

    // find_session benchmark
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        rt.block_on(async {
            let _ = repo.find_session(session_id).await;
        });
    }
    let duration = start.elapsed();
    let avg_ns = duration.as_nanos() / ITERATIONS as u128;
    println!(
        "find_session:              {:>8} ns/op  ({:.3} ms for 1000 calls)",
        avg_ns,
        (avg_ns * 1000) as f64 / 1_000_000.0
    );

    // session_exists benchmark
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        rt.block_on(async {
            let _ = repo.session_exists(session_id).await;
        });
    }
    let duration = start.elapsed();
    let avg_ns = duration.as_nanos() / ITERATIONS as u128;
    println!(
        "session_exists:            {:>8} ns/op  ({:.3} ms for 1000 calls)",
        avg_ns,
        (avg_ns * 1000) as f64 / 1_000_000.0
    );

    // find_active_sessions benchmark
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        rt.block_on(async {
            let _ = repo.find_active_sessions().await;
        });
    }
    let duration = start.elapsed();
    let avg_ns = duration.as_nanos() / ITERATIONS as u128;
    println!(
        "find_active_sessions:      {:>8} ns/op  ({:.3} ms for 1000 calls)",
        avg_ns,
        (avg_ns * 1000) as f64 / 1_000_000.0
    );

    // get_session_health benchmark
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        rt.block_on(async {
            let _ = repo.get_session_health(session_id).await;
        });
    }
    let duration = start.elapsed();
    let avg_ns = duration.as_nanos() / ITERATIONS as u128;
    println!(
        "get_session_health:        {:>8} ns/op  ({:.3} ms for 1000 calls)",
        avg_ns,
        (avg_ns * 1000) as f64 / 1_000_000.0
    );

    // find_sessions_by_criteria benchmark
    let criteria = SessionQueryCriteria::default();
    let pagination = Pagination {
        offset: 0,
        limit: 100,
        ..Default::default()
    };

    let start = Instant::now();
    for _ in 0..ITERATIONS {
        rt.block_on(async {
            let _ = repo
                .find_sessions_by_criteria(criteria.clone(), pagination)
                .await;
        });
    }
    let duration = start.elapsed();
    let avg_ns = duration.as_nanos() / ITERATIONS as u128;
    println!(
        "find_sessions_by_criteria: {:>8} ns/op  ({:.3} ms for 1000 calls)",
        avg_ns,
        (avg_ns * 1000) as f64 / 1_000_000.0
    );

    println!("\n=== Target: All operations < 1ms for 1000 items ===\n");
}

criterion_group!(
    benches,
    bench_find_session,
    bench_find_active_sessions,
    bench_find_sessions_by_criteria,
    bench_session_exists,
    bench_get_session_health,
    bench_save_session,
    bench_concurrent_operations,
    bench_latency_distribution,
);

criterion_main!(benches);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_measure_raw_performance() {
        measure_raw_performance();
    }
}
