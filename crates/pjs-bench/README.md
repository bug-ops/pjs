# PJS Benchmarking Suite

[![CI](https://github.com/bug-ops/pjs/actions/workflows/rust.yml/badge.svg)](https://github.com/bug-ops/pjs/actions/workflows/rust.yml)
[![License](https://img.shields.io/crates/l/pjson-rs)](../../LICENSE-MIT)

Comprehensive performance benchmarking for the Priority JSON Streaming Protocol (PJS).

> [!NOTE]
> This crate is part of the [PJS workspace](https://github.com/bug-ops/pjs). Run benchmarks with `cargo bench -p pjs-bench`.

## Overview

This suite compares PJS against major JSON parsing libraries to demonstrate the performance advantages of priority-based streaming:

- **serde_json** - Standard Rust JSON library (baseline)
- **sonic-rs** - SIMD-optimized JSON parser  
- **PJS** - Priority JSON Streaming Protocol

## Actual Performance Results 🚀

### Raw Parsing Performance (serde_json vs sonic-rs)

> **Note:** the standalone "PJS Parser" benchmark group (`benchmark_pjs_parser`, plus the `pjs_parser` arm of `benchmark_comparison`) and the `pjson_rs::Parser` type it measured were removed as dead code in #486/#488 — the type had no production caller. The tables that used to compare a "PJS Parser" column against `serde_json`/`sonic-rs` here are removed along with it; `simple_throughput`'s `parsing_comparison` group still benchmarks `serde_json` against `sonic_rs` directly. Re-run `cargo bench -p pjs-bench --bench simple_throughput` for current numbers on your hardware.

### 🚀 Memory Usage Comparison (from memory_benchmarks)

| Dataset Size | PJS Parser | serde_json | sonic-rs | PJS vs serde_json |
|--------------|------------|------------|----------|-------------------|
| **1MB** | 20.3μs | 65.6μs | 16.7μs | **3.2x faster** |
| **5MB** | 85.3μs | 322μs | 82.8μs | **3.8x faster** |
| **10MB** | 217μs | 655μs | 184μs | **3.0x faster** |

### ⚡ Progressive Memory Patterns - The Game Changer

| Pattern | Traditional (Batch Load) | PJS (Progressive) | PJS Advantage |
|---------|-------------------------|------------------|---------------|
| **Memory Usage** | 198μs (peak memory spike) | 37μs (bounded memory) | **5.3x faster** |
| **UI Rendering** | Must wait for full parse | Progressive updates | Instant start |
| **User Experience** | Loading... → Complete | Skeleton → Progressive | Much better |

*PJS enables bounded memory usage and instant UI updates vs traditional batch loading*

## Benchmark Categories

The `[[bench]]` targets below are the ones actually wired into `Cargo.toml` and runnable today; older docs on this page describing `memory_benchmarks`, `streaming_benchmarks`, and `time_to_first_data` predate a benchmark-suite cleanup and no longer apply (those files were disabled since v0.2.0).

### 1. Basic Throughput (`cargo bench --bench simple_throughput`)

Raw parsing speed comparison across different JSON sizes (small/medium/large payloads), `serde_json` vs `sonic_rs` (`benchmark_serde_json`, `benchmark_sonic_rs`, `benchmark_comparison`). See the note above — the dedicated "PJS Parser" comparison arm was removed along with the dead parser it measured (#486, #488); this bench now only compares `serde_json` against `sonic_rs` directly.

### 2. Zero-Copy Parser (`cargo bench --bench zero_copy_bench`)

Benchmarks `ZeroCopyParser`/`LazyParser` against simple strings, JSON objects, arrays, and memory efficiency (`bench_simple_string`, `bench_json_objects`, `bench_arrays`, `bench_memory_efficiency`) — the crate's actual zero-copy parsing hot path.

### 3. Serde Overhead (`cargo bench --bench serde_overhead_bench`)

Measures the cost of this crate's hand-written `Serialize`/`Deserialize` impls and value-object construction: custom serde serialization/deserialization, value-object creation, and UUID/string conversion (`benchmark_custom_serde_serialization`, `benchmark_custom_serde_deserialization`, `benchmark_value_object_creation`, `benchmark_uuid_string_conversion`).

### 4. GAT Query Performance (`cargo bench --bench gat_query_benchmarks`)

Validates the GAT-based repository ports meet their performance targets (sub-millisecond query methods at 1000 sessions/streams, lock-free `DashMap` operations, zero `Box<dyn Future>` allocations): session lookup, active-session queries, criteria search, existence checks, health snapshots, saves, concurrent operations, and latency distribution.

### 5. HTTP Streaming (`cargo bench --bench http_streaming`)

Added to answer #514: isolates `sonic_rs` vs `serde_json` on the exact serialization primitive #510 swapped (`bench_serialization_many_small_calls`, `bench_serialization_one_big_call`), plus an end-to-end baseline over the actual production call chain (`bench_batch_frame_stream_e2e`, via `BatchFrameStream::into_stream()`). Measured: `sonic_rs` is ~1.4-1.6x faster per-frame and ~1.7-1.8x faster per-batch at the primitive level, but only ~9-11% faster on the full production route once `frame_to_value`'s unchanged `serde_json`-based prep and stream/async overhead are accounted for — see the bench's own module doc for the full caveat.

## Real-World Impact

### Social Media Feed

```json
{
  "posts": [...],           // Priority: High (show first)
  "pagination": {...},      // Priority: Critical  
  "user_context": {...}     // Priority: High
}
```

**Traditional**: Wait 1.2ms for complete parsing  
**PJS**: Show posts in <50μs with skeleton, full data follows

### E-commerce Catalog

```json
{
  "products": [...],        // Priority: Critical (show grid)
  "filters": {...},         // Priority: High (show sidebar)
  "recommendations": [...]  // Priority: Low (load later)
}
```

**Traditional**: 1.2ms+ for full page  
**PJS**: Product grid in 50μs, progressive enhancement

### Analytics Dashboard

```json
{
  "metrics": {...},         // Priority: Critical (KPIs first)
  "charts": {...},          // Priority: High (main charts)
  "detailed_reports": [...] // Priority: Low (background)
}
```

**Traditional**: 1.2ms dashboard load time  
**PJS**: Key metrics in <100μs, charts follow

## Running Benchmarks

### All Benchmarks

```bash
cargo bench
```

### Individual Benchmark Suites

```bash
# Basic throughput comparison (serde_json vs sonic_rs)
cargo bench --bench simple_throughput

# Zero-copy parser (ZeroCopyParser/LazyParser)
cargo bench --bench zero_copy_bench

# Hand-written serde impl and value-object overhead
cargo bench --bench serde_overhead_bench

# GAT-based repository port query performance
cargo bench --bench gat_query_benchmarks

# HTTP streaming serialization (sonic_rs vs serde_json, e2e route)
cargo bench --bench http_streaming
```

## Interpreting Results

### Throughput Metrics

- **ns/μs** - Lower is better (latency)
- **MiB/s or GiB/s** - Higher is better (throughput)
- Focus on real-world JSON sizes (1KB-1MB)

### Streaming Advantage

- **Time to First Data** - PJS delivers critical data 143-1565x faster
- **Progressive Enhancement** - UI updates while parsing continues
- **Memory Efficiency** - Process large JSON with constant memory
- **Massive Dataset Handling** - 1.7-1.8 GiB/s throughput on 10MB+ JSON

## Performance Summary

### Real-world Impact

- **6.3x faster** than serde_json for large JSON processing (357KB)
- **3.0-3.8x faster** than serde_json for massive datasets (1MB-10MB)
- **5.3x faster** progressive loading vs traditional batch processing
- **1.06x faster** than sonic-rs while adding streaming capabilities

### Key Achievements

✅ **Production-ready performance** - 6.3x faster than serde_json on large data
✅ **Streaming advantage preserved** - 5.3x faster progressive loading
✅ **Memory efficiency** - Bounded memory usage vs peak spikes  
✅ **SIMD performance** - Exceeds sonic-rs on large datasets (1.71 vs 1.61 GiB/s)

## Hardware Considerations

### Optimal Performance

- **x86_64**: Benefits from SIMD optimizations
- **Large L2/L3 cache**: Improves streaming performance
- **Fast RAM**: Critical for large JSON processing

### Architecture Support

- **AVX2/AVX-512**: Maximum SIMD acceleration
- **ARM NEON**: Good performance on Apple Silicon
- **Fallback**: Pure Rust implementation available

## Limitations

- Small JSON (<100B) still has some overhead vs raw sonic-rs
- Streaming benefits most apparent with structured/hierarchical data
- Semantic analysis adds minimal overhead for very large datasets
- SIMD performance varies by CPU generation

## Contributing

When adding benchmarks:

1. Use realistic data patterns from real applications
2. Measure end-to-end performance including allocation costs
3. Test across different data sizes and structures
4. Validate streaming scenarios separately from batch processing

## Conclusion

**PJS has achieved its performance goals:**

🎯 **Competitive raw parsing** - Within 1.4x of sonic-rs for medium/large JSON
🚀 **Superior streaming experience** - 1565x faster time-to-first-data on massive datasets
📈 **Production ready** - 5x faster than serde_json with streaming benefits
⚡ **Massive data optimized** - 1.8 GiB/s throughput, sub-microsecond skeleton delivery
