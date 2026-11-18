//! Domain patch extraction benchmarks (#755, #756)
//!
//! Measures the two halves of `Stream::create_patch_frames` on a wide document:
//! the priority-priced traversal (`extract_prioritized_patches`) and the
//! chunking commit (`commit_patch_frames`).

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use pjson_rs_domain::Stream;
use pjson_rs_domain::entities::stream::StreamConfig;
use pjson_rs_domain::value_objects::{JsonData, Priority, SessionId};
use serde_json::{Map, Value, json};
use std::hint::black_box;

const LEAVES: usize = 10_000;
const MAX_FRAMES: usize = 100;

fn wide_stream() -> Stream {
    let source: Map<String, Value> = (0..LEAVES)
        .map(|i| (format!("field_{i}"), json!(i)))
        .collect();
    let mut stream = Stream::new(
        SessionId::new(),
        JsonData::from(Value::Object(source)),
        StreamConfig::default(),
    );
    stream
        .start_streaming()
        .expect("a new stream starts streaming");
    stream
}

fn bench_extract(c: &mut Criterion) {
    let stream = wide_stream();
    let mut group = c.benchmark_group("patch_extraction");
    group.throughput(Throughput::Elements(LEAVES as u64));

    group.bench_function("extract_prioritized_patches", |b| {
        b.iter(|| {
            stream
                .extract_prioritized_patches(black_box(Priority::BACKGROUND))
                .expect("extraction succeeds")
        });
    });

    group.bench_function("commit_patch_frames", |b| {
        b.iter_batched(
            || {
                let stream = wide_stream();
                let patches = stream
                    .extract_prioritized_patches(Priority::BACKGROUND)
                    .expect("extraction succeeds");
                (stream, patches)
            },
            |(mut stream, patches)| {
                stream
                    .commit_patch_frames(black_box(patches), MAX_FRAMES)
                    .expect("commit succeeds")
            },
            BatchSize::LargeInput,
        );
    });

    group.finish();
}

criterion_group!(benches, bench_extract);
criterion_main!(benches);
