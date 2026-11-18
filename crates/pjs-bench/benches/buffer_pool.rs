//! `BufferPool::acquire` plus drop benchmarks (#774)
//!
//! Hit: a warmed pool hands the same buffer back on every iteration.
//! Miss: a fresh pool allocates (and the drop returns the buffer to the pool).

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use pjson_rs::parser::{BufferPool, BufferSize};
use std::hint::black_box;

const CLASSES: [BufferSize; 3] = [BufferSize::Small, BufferSize::Medium, BufferSize::Large];

fn bench_acquire_drop(c: &mut Criterion) {
    let mut group = c.benchmark_group("buffer_pool_acquire_drop");

    for size in CLASSES {
        let id = format!("{size:?}");

        let pool = BufferPool::new();
        drop(pool.acquire(size).expect("warm-up acquire succeeds"));
        group.bench_with_input(BenchmarkId::new("hit", &id), &size, |b, &size| {
            b.iter(|| drop(black_box(pool.acquire(size).expect("acquire succeeds"))));
        });

        group.bench_with_input(BenchmarkId::new("miss", &id), &size, |b, &size| {
            b.iter_batched(
                BufferPool::new,
                |pool| drop(black_box(pool.acquire(size).expect("acquire succeeds"))),
                BatchSize::PerIteration,
            );
        });
    }

    group.finish();
}

criterion_group!(benches, bench_acquire_drop);
criterion_main!(benches);
