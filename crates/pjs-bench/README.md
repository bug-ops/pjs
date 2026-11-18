# PJS Benchmarking Suite

[![CI](https://github.com/bug-ops/pjs/actions/workflows/ci.yml/badge.svg)](https://github.com/bug-ops/pjs/actions/workflows/ci.yml)
[![License](https://img.shields.io/crates/l/pjson-rs)](../../LICENSE-MIT)

Comprehensive performance benchmarking for the Priority JSON Streaming Protocol (PJS).

> [!NOTE]
> This crate is part of the [PJS workspace](https://github.com/bug-ops/pjs). Run benchmarks with `cargo bench -p pjs-bench` (the full suite, including `client_apply_patches`, additionally needs `--cfg pjs_bench`, see [Running Benchmarks](#running-benchmarks)).

## Overview

This suite compares PJS against major JSON parsing libraries to demonstrate the performance advantages of priority-based streaming:

- **serde_json** - Standard Rust JSON library (baseline)
- **sonic-rs** - SIMD-optimized JSON parser  
- **PJS** - Priority JSON Streaming Protocol

## Performance Results

### Raw Parsing Performance (serde_json vs sonic-rs)

> **Note:** the standalone "PJS Parser" benchmark group (`benchmark_pjs_parser`, plus the `pjs_parser` arm of `benchmark_comparison`) and the `pjson_rs::Parser` type it measured were removed as dead code in #486/#488 — the type had no production caller. The tables that used to compare a "PJS Parser" column against `serde_json`/`sonic-rs` here are removed along with it; `simple_throughput`'s `parsing_comparison` group still benchmarks `serde_json` against `sonic_rs` directly. Re-run `cargo bench -p pjs-bench --bench simple_throughput` for current numbers on your hardware.

## Benchmark Categories

The `[[bench]]` targets below are the ones wired into `Cargo.toml`. The former `memory_benchmarks`, `streaming_benchmarks` and `time_to_first_data` targets were removed in an earlier benchmark-suite cleanup, so no published numbers from them remain here.

### 1. Basic Throughput (`cargo bench --bench simple_throughput`)

Raw parsing speed comparison across different JSON sizes (small/medium/large payloads), `serde_json` vs `sonic_rs` (`benchmark_serde_json`, `benchmark_sonic_rs`, `benchmark_comparison`). The dedicated "PJS Parser" comparison arm was removed along with the dead parser it measured (#486, #488); this bench now only compares `serde_json` against `sonic_rs` directly.

### 2. Zero-Copy Parser (`cargo bench --bench zero_copy_bench`)

Benchmarks `ZeroCopyParser`/`LazyParser` against simple strings, JSON objects, arrays, and memory efficiency (`bench_simple_string`, `bench_json_objects`, `bench_arrays`, `bench_memory_efficiency`) — the crate's actual zero-copy parsing hot path.

### 3. Serde Overhead (`cargo bench --bench serde_overhead_bench`)

Measures the cost of this crate's hand-written `Serialize`/`Deserialize` impls and value-object construction: custom serde serialization/deserialization, value-object creation, and UUID/string conversion (`benchmark_custom_serde_serialization`, `benchmark_custom_serde_deserialization`, `benchmark_value_object_creation`, `benchmark_uuid_string_conversion`).

### 4. GAT Query Performance (`cargo bench --bench gat_query_benchmarks`)

Validates the GAT-based repository ports meet their performance targets (sub-millisecond query methods at 1000 sessions/streams, lock-free `DashMap` operations, zero `Box<dyn Future>` allocations): session lookup, active-session queries, criteria search, existence checks, health snapshots, saves, concurrent operations, and latency distribution.

### 5. HTTP Streaming (`cargo bench --bench http_streaming`)

Added to answer #514: isolates `sonic_rs` vs `serde_json` on the exact serialization primitive #510 swapped (`bench_serialization_many_small_calls`, `bench_serialization_one_big_call`), plus an end-to-end baseline over the actual production call chain (`bench_batch_frame_stream_e2e`, via `BatchFrameStream::into_stream()`). Measured: `sonic_rs` is ~1.4-1.6x faster per-frame and ~1.7-1.8x faster per-batch at the primitive level, but only ~9-11% faster on the full production route once `frame_to_value`'s unchanged `serde_json`-based prep and stream/async overhead are accounted for — see the bench's own module doc for the full caveat.

### 6. Connection Lifecycle (`cargo bench --bench http_serve_lifecycle`)

Covers the `serve_with_limits` accept loop and graceful shutdown: request round trip, a pre-triggered shutdown signal, and a shutdown trigger with idle connections waiting to drain.

### 7. WebSocket Controller (`cargo bench --bench websocket_controller`)

Benchmarks `AdaptiveStreamController`: `create_session` planning cost for 1 KiB to 1 MiB documents, the entry-dense document shapes that calibrate the blocking-plan offload gate, the `spawn_blocking` hop, and per-frame `handle_frame_ack` throughput under concurrent tasks.

### 8. Client Patch Application (`cargo bench --bench client_apply_patches`)

Measures applying patches on the client. It is a stub that only prints a skip line unless built with `--cfg pjs_bench` (see [Running Benchmarks](#running-benchmarks)).

### 9. Patch Extraction (`cargo bench --bench patch_extraction`)

Measures the two halves of `Stream::create_patch_frames` on a wide document: the priority-priced traversal and the chunking commit.

### 10. Buffer Pool (`cargo bench --bench buffer_pool`)

`BufferPool::acquire` plus drop, for a warmed pool (hit) and a fresh pool (miss).

### 11. Priority Streamer (`cargo bench --bench priority_streamer`)

`PriorityStreamer::analyze` on wide documents (many scalar leaves) and deep documents (a chain of nested objects).

### 12. Instruction Counts (`cargo bench --bench gungraun_parser`)

Deterministic Callgrind instruction-count counterpart of the `simple_throughput` parser group. Valgrind runs on Linux only; other targets compile an empty `main`.

## Modelled Scenarios

The payload shapes below illustrate how priorities map to documents; they are not measured results.

### Social Media Feed

```json
{
  "posts": [...],           // Priority: High (show first)
  "pagination": {...},      // Priority: Critical  
  "user_context": {...}     // Priority: High
}
```


### E-commerce Catalog

```json
{
  "products": [...],        // Priority: Critical (show grid)
  "filters": {...},         // Priority: High (show sidebar)
  "recommendations": [...]  // Priority: Low (load later)
}
```


### Analytics Dashboard

```json
{
  "metrics": {...},         // Priority: Critical (KPIs first)
  "charts": {...},          // Priority: High (main charts)
  "detailed_reports": [...] // Priority: Low (background)
}
```


## Running Benchmarks

### All Benchmarks

```bash
cargo bench -p pjs-bench
```

Without the `pjs_bench` cfg, `client_apply_patches` compiles to a stub that only prints a skip line. The full suite needs `RUSTFLAGS="-C target-cpu=native --cfg pjs_bench" cargo bench -p pjs-bench`; `RUSTFLAGS` replaces the flags from `.cargo/config.toml`, so `target-cpu=native` must be repeated.

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

# Connection lifecycle, WebSocket controller, patch extraction, buffer pool, streamer
cargo bench --bench http_serve_lifecycle
cargo bench --bench websocket_controller
cargo bench --bench patch_extraction
cargo bench --bench buffer_pool
cargo bench --bench priority_streamer

# Client patch application (needs the pjs_bench cfg)
RUSTFLAGS="-C target-cpu=native --cfg pjs_bench" cargo bench -p pjs-bench --bench client_apply_patches
```

### Before/After (A/B) Comparison

Criterion's `--save-baseline` and `--baseline` flags are only understood by the Criterion targets. Without an explicit `--bench <name>`, `cargo bench -p pjs-bench` also runs the library's libtest target, which fails with `Unrecognized option` and produces no data while the command can still look finished.

1. Run on a quiet machine: no builds, tests or other benchmarks in parallel. Concurrent load makes the numbers meaningless (a load average of ~28 once showed +10..+544% on benchmarks that only exercise `serde_json`).
2. Save a baseline on the unchanged code, one explicit `--bench` per target:

   ```bash
   cargo bench -p pjs-bench --bench simple_throughput -- --save-baseline before
   ```

3. Apply the change and compare against it:

   ```bash
   cargo bench -p pjs-bench --bench simple_throughput -- --baseline before
   ```

4. Use a benchmark that only exercises third-party dependencies as a noise gauge, for example the `serde_json_parsing` group of `simple_throughput` (`-- serde_json_parsing`). If it reports a change, the machine is too noisy: discard the run and repeat.
5. Check that the output contains `time:` lines for every benchmark. A run without them produced no data, whatever the exit status.

## Interpreting Results

### Throughput Metrics

- **ns/μs** - Lower is better (latency)
- **MiB/s or GiB/s** - Higher is better (throughput)
- Focus on real-world JSON sizes (1KB-1MB)

### Streaming Advantage

The suite measures costs (parsing, planning, patch application, pool operations), not end-to-end time to first render. An end-to-end comparison is not part of this crate yet.

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
