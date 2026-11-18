//! `PriorityStreamer::analyze` benchmarks (#774)
//!
//! Wide: one object with many scalar leaves. Deep: a chain of nested objects.

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use pjson_rs::PriorityStreamer;
use serde_json::{Map, Value, json};
use std::hint::black_box;

const WIDE_LEAVES: usize = 10_000;
const DEEP_LEVELS: usize = 64;

fn wide_document() -> Value {
    Value::Object(
        (0..WIDE_LEAVES)
            .map(|i| (format!("field_{i}"), json!(i)))
            .collect::<Map<_, _>>(),
    )
}

fn deep_document() -> Value {
    (0..DEEP_LEVELS).fold(
        json!({"id": 1, "name": "leaf"}),
        |inner, level| json!({"level": level, "id": level, "child": inner}),
    )
}

fn bench_analyze(c: &mut Criterion) {
    let streamer = PriorityStreamer::new();
    let mut group = c.benchmark_group("priority_streamer_analyze");

    let wide = wide_document();
    group.throughput(Throughput::Elements(WIDE_LEAVES as u64));
    group.bench_function("wide", |b| {
        b.iter(|| {
            streamer
                .analyze(black_box(&wide))
                .expect("analyze succeeds")
        });
    });

    let deep = deep_document();
    group.throughput(Throughput::Elements(DEEP_LEVELS as u64));
    group.bench_function("deep", |b| {
        b.iter(|| {
            streamer
                .analyze(black_box(&deep))
                .expect("analyze succeeds")
        });
    });

    group.finish();
}

criterion_group!(benches, bench_analyze);
criterion_main!(benches);
