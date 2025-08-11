//! Simple throughput benchmarks - basic parsing speed comparison
//!
//! Simplified version that focuses on the core parsing capabilities

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use pjson_rs::Parser;
use serde_json::Value;
use std::hint::black_box;
use std::time::Duration;

const SMALL_JSON: &str = r#"{"id": 1, "name": "test", "status": "active"}"#;

const MEDIUM_JSON: &str = r#"{
  "user": {
    "id": 12345,
    "name": "John Doe",
    "email": "john.doe@example.com",
    "status": "active",
    "profile": {
      "bio": "Software engineer",
      "location": "San Francisco",
      "company": "TechCorp"
    },
    "posts": [
      {"id": 1, "title": "Hello World", "likes": 25},
      {"id": 2, "title": "Tech Tips", "likes": 42}
    ]
  }
}"#;

fn generate_large_json() -> String {
    let mut items = Vec::new();
    
    for i in 0..1000 {
        items.push(format!(r#"{{
            "id": {},
            "name": "Item {}",
            "description": "This is item number {} with some content",
            "price": {:.2},
            "category": "Category {}",
            "active": {},
            "metadata": {{
                "created": "2024-01-01T10:30:00Z",
                "tags": ["tag1", "tag2", "tag3"]
            }}
        }}"#, i, i, i, (i as f64 * 1.5 + 10.0), i % 10, i % 2 == 0));
    }
    
    format!(r#"{{
        "data": [{}],
        "total": 1000,
        "page": 1,
        "metadata": {{
            "generated_at": "2024-01-15T12:00:00Z",
            "version": "1.0"
        }}
    }}"#, items.join(","))
}

fn benchmark_serde_json(c: &mut Criterion) {
    let mut group = c.benchmark_group("serde_json_parsing");
    
    // Small JSON
    group.throughput(Throughput::Bytes(SMALL_JSON.len() as u64));
    group.bench_function("small", |b| {
        b.iter(|| {
            let _: Value = serde_json::from_str(black_box(SMALL_JSON)).unwrap();
        })
    });
    
    // Medium JSON
    group.throughput(Throughput::Bytes(MEDIUM_JSON.len() as u64));
    group.bench_function("medium", |b| {
        b.iter(|| {
            let _: Value = serde_json::from_str(black_box(MEDIUM_JSON)).unwrap();
        })
    });
    
    // Large JSON
    let large_json = generate_large_json();
    group.throughput(Throughput::Bytes(large_json.len() as u64));
    group.bench_function("large", |b| {
        b.iter(|| {
            let _: Value = serde_json::from_str(black_box(&large_json)).unwrap();
        })
    });
    
    group.finish();
}

fn benchmark_sonic_rs(c: &mut Criterion) {
    let mut group = c.benchmark_group("sonic_rs_parsing");
    
    // Small JSON
    group.throughput(Throughput::Bytes(SMALL_JSON.len() as u64));
    group.bench_function("small", |b| {
        b.iter(|| {
            let _: sonic_rs::Value = sonic_rs::from_str(black_box(SMALL_JSON)).unwrap();
        })
    });
    
    // Medium JSON
    group.throughput(Throughput::Bytes(MEDIUM_JSON.len() as u64));
    group.bench_function("medium", |b| {
        b.iter(|| {
            let _: sonic_rs::Value = sonic_rs::from_str(black_box(MEDIUM_JSON)).unwrap();
        })
    });
    
    // Large JSON
    let large_json = generate_large_json();
    group.throughput(Throughput::Bytes(large_json.len() as u64));
    group.bench_function("large", |b| {
        b.iter(|| {
            let _: sonic_rs::Value = sonic_rs::from_str(black_box(&large_json)).unwrap();
        })
    });
    
    group.finish();
}

fn benchmark_pjs_parser(c: &mut Criterion) {
    let mut group = c.benchmark_group("pjs_parser");
    
    // Small JSON
    group.throughput(Throughput::Bytes(SMALL_JSON.len() as u64));
    group.bench_function("small", |b| {
        b.iter(|| {
            let parser = Parser::new();
            let _ = parser.parse(black_box(SMALL_JSON.as_bytes())).unwrap();
        })
    });
    
    // Medium JSON
    group.throughput(Throughput::Bytes(MEDIUM_JSON.len() as u64));
    group.bench_function("medium", |b| {
        b.iter(|| {
            let parser = Parser::new();
            let _ = parser.parse(black_box(MEDIUM_JSON.as_bytes())).unwrap();
        })
    });
    
    // Large JSON
    let large_json = generate_large_json();
    group.throughput(Throughput::Bytes(large_json.len() as u64));
    group.bench_function("large", |b| {
        b.iter(|| {
            let parser = Parser::new();
            let _ = parser.parse(black_box(large_json.as_bytes())).unwrap();
        })
    });
    
    group.finish();
}

fn benchmark_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("parsing_comparison");
    group.measurement_time(Duration::from_secs(10));
    
    let large_json = generate_large_json();
    let json_size = large_json.len() as u64;
    
    group.throughput(Throughput::Bytes(json_size));
    
    // serde_json baseline
    group.bench_function("serde_json", |b| {
        b.iter(|| {
            let _: Value = serde_json::from_str(black_box(&large_json)).unwrap();
        })
    });
    
    // sonic-rs (SIMD optimized)
    group.bench_function("sonic_rs", |b| {
        b.iter(|| {
            let _: sonic_rs::Value = sonic_rs::from_str(black_box(&large_json)).unwrap();
        })
    });
    
    // PJS Parser
    group.bench_function("pjs_parser", |b| {
        b.iter(|| {
            let parser = Parser::new();
            let _ = parser.parse(black_box(large_json.as_bytes())).unwrap();
        })
    });
    
    group.finish();
}

criterion_group!(
    benches,
    benchmark_serde_json,
    benchmark_sonic_rs,
    benchmark_pjs_parser,
    benchmark_comparison
);

criterion_main!(benches);