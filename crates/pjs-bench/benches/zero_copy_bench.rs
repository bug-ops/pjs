//! Benchmarks for zero-copy JSON parser
//!
//! Run with: cargo bench --bench zero_copy_bench

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use pjson_rs::parser::{LazyParser, SimdZeroCopyConfig, SimdZeroCopyParser, ZeroCopyParser};
use std::hint::black_box;

fn bench_simple_string(c: &mut Criterion) {
    let mut group = c.benchmark_group("simple_string");

    let large_string = {
        let mut v = vec![b'"'];
        v.extend(b"x".repeat(1000));
        v.push(b'"');
        v
    };

    let test_cases = vec![
        ("small", br#""hello world""# as &[u8]),
        ("medium", br#""The quick brown fox jumps over the lazy dog. This is a medium length string for testing.""#),
        ("large", &large_string),
    ];

    for (name, input) in test_cases {
        group.throughput(Throughput::Bytes(input.len() as u64));

        group.bench_with_input(BenchmarkId::new("zero_copy", name), input, |b, input| {
            let mut parser = ZeroCopyParser::new();
            b.iter(|| {
                parser.reset();
                let result = parser.parse_lazy(black_box(input)).unwrap();
                black_box(result);
            })
        });

        group.bench_with_input(BenchmarkId::new("simd", name), input, |b, input| {
            let mut parser = SimdZeroCopyParser::new();
            b.iter(|| {
                parser.reset();
                let result = parser.parse_simd(black_box(input)).unwrap();
                black_box(result);
            })
        });
    }

    group.finish();
}

fn bench_json_objects(c: &mut Criterion) {
    let mut group = c.benchmark_group("json_objects");

    let small_json = br#"{"name": "test", "value": 42}"#;
    let medium_json = br#"{
        "id": 12345,
        "name": "John Doe",
        "email": "john.doe@example.com",
        "active": true,
        "metadata": {
            "created": "2025-01-01",
            "updated": "2025-01-15",
            "tags": ["user", "premium", "verified"]
        }
    }"#;

    let large_json = generate_large_json(100);
    let large_bytes = large_json.as_bytes();

    let test_cases = vec![
        ("small", &small_json[..]),
        ("medium", &medium_json[..]),
        ("large", large_bytes),
    ];

    for (name, input) in test_cases {
        group.throughput(Throughput::Bytes(input.len() as u64));

        group.bench_with_input(BenchmarkId::new("zero_copy", name), input, |b, input| {
            let mut parser = ZeroCopyParser::new();
            b.iter(|| {
                parser.reset();
                let result = parser.parse_lazy(black_box(input)).unwrap();
                black_box(result);
            })
        });

        group.bench_with_input(BenchmarkId::new("simd", name), input, |b, input| {
            let mut parser = SimdZeroCopyParser::new();
            b.iter(|| {
                parser.reset();
                let result = parser.parse_simd(black_box(input)).unwrap();
                black_box(result);
            })
        });
    }

    group.finish();
}

fn bench_arrays(c: &mut Criterion) {
    let mut group = c.benchmark_group("json_arrays");

    let small_array = b"[1, 2, 3, 4, 5]";
    let medium_array = format!(
        "[{}]",
        (0..100)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
    let large_array = format!(
        "[{}]",
        (0..10000)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );

    let test_cases = vec![
        ("small", &small_array[..]),
        ("medium", medium_array.as_bytes()),
        ("large", large_array.as_bytes()),
    ];

    for (name, input) in test_cases {
        group.throughput(Throughput::Bytes(input.len() as u64));

        group.bench_with_input(BenchmarkId::new("zero_copy", name), input, |b, input| {
            let mut parser = ZeroCopyParser::new();
            b.iter(|| {
                parser.reset();
                let result = parser.parse_lazy(black_box(input)).unwrap();
                black_box(result);
            })
        });

        group.bench_with_input(
            BenchmarkId::new("simd_high_perf", name),
            input,
            |b, input| {
                let mut parser =
                    SimdZeroCopyParser::with_config(SimdZeroCopyConfig::high_performance());
                b.iter(|| {
                    parser.reset();
                    let result = parser.parse_simd(black_box(input)).unwrap();
                    black_box(result);
                })
            },
        );
    }

    group.finish();
}

fn bench_memory_efficiency(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_efficiency");

    let test_json = br#"{
        "users": [
            {"id": 1, "name": "Alice", "email": "alice@example.com"},
            {"id": 2, "name": "Bob", "email": "bob@example.com"},
            {"id": 3, "name": "Charlie", "email": "charlie@example.com"}
        ],
        "metadata": {
            "total": 3,
            "page": 1,
            "per_page": 10
        }
    }"#;

    group.bench_function("zero_copy_efficiency", |b| {
        let mut parser = ZeroCopyParser::new();
        b.iter(|| {
            parser.reset();
            let result = parser.parse_lazy(black_box(test_json)).unwrap();
            let memory_usage = result.memory_usage();
            assert!(memory_usage.efficiency() > 0.8); // Should be highly efficient
            black_box(memory_usage);
        })
    });

    group.bench_function("simd_efficiency", |b| {
        let mut parser = SimdZeroCopyParser::new();
        b.iter(|| {
            parser.reset();
            let result = parser.parse_simd(black_box(test_json)).unwrap();
            assert!(result.memory_usage.efficiency() > 0.8);
            black_box(result.memory_usage);
        })
    });

    group.finish();
}

fn generate_large_json(items: usize) -> String {
    let mut json = String::from(r#"{"items": ["#);

    for i in 0..items {
        if i > 0 {
            json.push(',');
        }
        json.push_str(&format!(
            r#"{{"id": {}, "name": "item_{}", "value": {}}}"#,
            i,
            i,
            i * 10
        ));
    }

    json.push_str(r#"], "count": "#);
    json.push_str(&items.to_string());
    json.push('}');

    json
}

criterion_group!(
    benches,
    bench_simple_string,
    bench_json_objects,
    bench_arrays,
    bench_memory_efficiency
);

criterion_main!(benches);
