---
aliases:
  - benchmark suite
  - pjs-bench
tags:
  - sdd
  - spec
  - performance
  - benchmarks
created: 2026-11-15
status: implemented
priority: P2
related:
  - "[[021-bench-crate-workspace-isolation/spec]]"
  - "[[078-ci-pipeline/spec]]"
issue: "#514"
pr: "#519"
code:
  - crates/pjs-bench/Cargo.toml
  - crates/pjs-bench/src/lib.rs
  - crates/pjs-bench/benches/simple_throughput.rs
  - crates/pjs-bench/benches/zero_copy_bench.rs
  - crates/pjs-bench/benches/serde_overhead_bench.rs
  - crates/pjs-bench/benches/gat_query_benchmarks.rs
  - crates/pjs-bench/benches/http_streaming.rs
  - scripts/profile.sh
verified: 2026-11-15
---

# Spec 075: pjs-bench benchmark suite

Part of [[MOC-specs]]; see [[constitution]].

> [!info] Verification (2026-11-15)
> Implemented: Criterion benches (FR-002 to FR-006, FR-008, FR-009) exist and compile as workspace member `pjs-bench` (workspace isolation history in [[021-bench-crate-workspace-isolation/spec]]). No CI job runs them; regression checking is manual per `.claude/rules/continuous-improvement.md` (more than 10% regression is a P1 issue). Open: #619 (document quiet-machine A/B procedure).

## Overview

**Problem.** PJS claims high performance (SIMD parsing, GAT zero-overhead ports, fast serialization); claims need repeatable measurements.

**Goal.** Describe each benchmark target, which claim it guards, and how to run and profile it.

**Out of scope.** The benchmarks' numbers (machine dependent); README result tables (stale, see Open Questions).

## Functional Requirements

### FR-001 Crate layout
`pjs-bench` shall be a workspace member (root `members = ["crates/*"]`, not in `default-members`), `publish = false`, with `harness = false` for each `[[bench]]` target, depending on `criterion` (html_reports), `pjson-rs`, `pjson-rs-domain`, `serde_json`, `sonic-rs`, `tokio`, `futures`, `chrono`, `uuid`, with dev-dependency `dhat` and an empty feature `dhat-heap`. `src/lib.rs` contains documentation only.
Code: `crates/pjs-bench/Cargo.toml`, `src/lib.rs`.
Status: done.

### FR-002 simple_throughput
The bench shall measure parsing of three fixed JSON documents (small, medium nested, large with 1000 items) with `serde_json` and `sonic_rs` separately (`serde_json_parsing`, `sonic_rs_parsing`) and head to head (`parsing_comparison`: `serde_json`, `sonic_rs`), reporting bytes/second throughput.
Guards: the parser-backend baseline.
Status: done. The former `pjs_parser` group was removed with the dead `Parser` type (#486/#488).

### FR-003 zero_copy_bench
The bench shall measure `pjson_rs::parser::{ZeroCopyParser, LazyParser}` over simple strings (`simple_string`), objects (`json_objects`), arrays (`json_arrays`) at several sizes via `BenchmarkId::new("zero_copy", name)` and a `memory_efficiency` group.
Guards: zero-copy parser speed and allocation behaviour.
Status: done.

### FR-004 serde_overhead_bench
The bench shall measure serialisation and deserialisation of domain events (`SessionActivated`, `StreamCreated`, a batch of 100 events) in groups `custom_serde_serialization` / `custom_serde_deserialization`, plus value-object creation (`Priority::new`, `SessionId::new`, `StreamId::new`) and UUID conversions (`SessionId::as_uuid`, `from_uuid`, `to_string`).
Guards: custom `serde` implementations of `pjs-domain` and constructor cost.
Status: done.

### FR-005 gat_query_benchmarks
The bench shall measure `GatInMemoryStreamRepository` (`StreamRepositoryGat`) over 100/500/1000 sessions: `find_session` (O(1)), `find_active_sessions` (O(n)), `find_sessions_by_criteria` (filter, sort, paginate), `session_exists`, `get_session_health`, `save_session`, a mixed concurrent read/write group, and a 1000-sample latency distribution (`p99_find_session`, `p99_session_exists`).
Guards: stated targets in the file header, namely every query under 1 ms for 1000 sessions, no `Box<dyn Future>` allocations, lock-free `DashMap` operations. The targets are not asserted in code; they are read from Criterion output.
Status: done.

### FR-006 http_streaming
The bench shall measure `infrastructure::http::streaming`: `http_streaming_serialization_many_small_calls` and `..._one_big_call` (`sonic_rs` vs `serde_json` at 10/100/1000 frames, values pre-built outside the timed loop) and `http_streaming_batch_frame_stream_e2e` (`BatchFrameStream::into_stream` for JSON and SSE at batch sizes 1 and 100; `noise_threshold(0.15)`, 8 s measurement time, `BatchSize::PerIteration`). NDJSON is skipped (byte-identical to JSON). Module docs state the answer to #514: sonic_rs contributes roughly 9-11% of the end-to-end route time.
Guards: the serialiser choice from #510 and the HTTP streaming hot path.
Status: done (#519).

### FR-007 Running and profiling
The suite shall run with `cargo bench -p pjs-bench [--bench <name>]`, using the workspace `[profile.bench]` (inherits release, `debug = true`). `scripts/profile.sh` shall offer `cpu` (cargo-flamegraph on `simple_throughput`), `heap` (dhat) and `bench` (all benches with `--profile-time=10`) subcommands.
Code: `scripts/profile.sh`.
Status: partial. See Open Questions.

### FR-008 websocket_controller
The bench shall measure `AdaptiveStreamController` (#664, #674): `ws_controller_create_session` (planning a 1 KiB to 1 MiB document; the input for `INLINE_PLAN_MAX_BYTES` and the byte-equivalent cost model of `planner::exceeds_plan_cost`), `ws_controller_create_session_shapes` (#683, #686: seven entry-dense calibration shapes `short_strings`, `small_ints`, `long_floats`, `short_floats`, `escape_strings`, `nested_arrays` and `small_objects`, with keys `k{i:04}`, each sized just under the pre-#683 gate cost of 32768 byte-equivalents so that all plan inline before #683; the sizes are fixed constants and the bench asserts the serialized size window of the float-free shapes; indicative before-#683 inline baseline on an idle machine, criterion point estimates: `document(32 KiB)` 90 us, `short_strings` 384 us, `small_ints` 393 us, `long_floats` 355 us, `short_floats` 346 us, `escape_strings` 63 us, `nested_arrays` 246 us, `small_objects` 203 us. Calibration protocol: other builds and tests idle, before and after runs interleaved with `uptime` recorded, compare ratios only, never absolute times. With the final weights (F 7, I 3, E 3, R 38) every shape is offloaded at its bench size; the largest document the gate keeps inline plans at at most 1.06x of the 90 us reference time, and offloaded runs add the 6-10 us blocking-pool hop, so after-run ratios use the before-run reference as denominator), `ws_controller_spawn_blocking_round_trip` and `ws_controller_handle_frame_ack` (500 connections, 1/4/8/16 tasks; proxy for the `release_sent` global write lock). Run: `cargo bench -p pjs-bench --bench websocket_controller`. Its recorded baseline lives in the `crates/pjs-bench` row of `.local/testing/coverage-status.md`; the per-cycle benchmark run compares against it (regression above 10% is P1), the baseline is re-recorded when `planner.rs` or the gate changes, and the figures quoted in `controller.rs` and spec 066 (planning rate, 16/32 KiB planning time, `spawn_blocking` hop, lock ops/s, sessions supported) are re-checked when a group moves by more than 10%.
Note (#685): in an A/B run against the old regex, the JS client's `isValidJsonPath` on a single repeated path measures 1.28x (about 8 ns absolute) while `validateFrame` over 1000 patches measures 1.07-1.09x.
Guards: the offload threshold of #658 and the keep-the-lock decision of #664.
Code: `crates/pjs-bench/benches/websocket_controller.rs`.
Status: done (#674).

### FR-009 client_apply_patches
The bench shall measure `ClientStreamSession::apply_patches` (#670) through the `fuzzing::ClientPatchHarness` entry point, which exists only under `--cfg pjs_bench`. Run: `RUSTFLAGS="-C target-cpu=native --cfg pjs_bench" cargo bench -p pjs-bench --bench client_apply_patches`; without the cfg the target prints that command and exits. Its baseline is recorded in the same `coverage-status.md` row and compared per cycle like FR-008.
Code: `crates/pjs-bench/benches/client_apply_patches.rs`.
Status: done (#670).

## Non-Functional Requirements

- NFR-PERF-001: no regression above 10% on any benchmark between cycles (policy in `.claude/rules/continuous-improvement.md`, manual).
- NFR-PERF-002: GAT queries below 1 ms for 1000 sessions (documented target in `gat_query_benchmarks.rs`).
- NFR-PERF-003: `http_streaming_serialization_*` groups are the preferred gate; the e2e group is a baseline with a wide noise threshold.

## Data Model

Fixtures are inline: `SMALL_JSON`, `MEDIUM_JSON`, `generate_large_json()` (1000 items); `make_frames(n)` with escape-heavy Unicode text; sessions created with `SessionConfig::default()` and activated.

## Edge Cases

| Case | Note |
|------|------|
| Shared machine | drift above 30% seen on few-microsecond cells (module doc of `http_streaming.rs`) |
| Changing `make_frames` | breaks comparison with saved baselines; save with `--save-baseline` first |

## Success Criteria

1. `cargo bench -p pjs-bench --no-run` compiles all targets (`client_apply_patches` compiles to a stub without `--cfg pjs_bench`).
2. A run of each target completes and writes `target/criterion/`.

## Agent Boundaries

- Always: compare against a saved baseline before claiming a regression; use `-p pjs-bench`.
- Ask first: adding a CI bench job (shared runners are noisy).
- Never: commit machine-specific numbers as requirements.

## Open Questions

- Benchmarks are not built or run in CI (`ci.yml` has no bench job); even `--no-run` compilation of the benches is covered only through `cargo clippy --workspace --all-targets`.
- `crates/pjs-bench/README.md` still contains stale tables and emoji from removed benches (`memory_benchmarks`, "PJS Parser" columns) and a CI badge pointing to `rust.yml`, which does not exist.
- `dhat-heap` feature is declared but no bench or source references `dhat` or the feature. `scripts/profile.sh heap` runs `cargo build --bench simple_throughput --features dhat-heap` without `-p pjs-bench`, so it cannot resolve in the virtual workspace root; `cpu` has the same missing `-p`. [NEEDS CLARIFICATION: not executed; flagged from reading]
- `profile.sh` help text refers to `.local/performance-profile.md`, which is untracked.
- SIMD paths (`simd-auto`, `simd-avx512`) and the hybrid parser selection have no dedicated bench (only `sonic_rs` vs `serde_json` directly).
- No benchmark for WASM, compression, or priority queue ordering, though those are listed as critical paths.

## See Also

[[plan]] | [[021-bench-crate-workspace-isolation/spec]] | [[078-ci-pipeline/spec]] | `.claude/rules/continuous-improvement.md`
