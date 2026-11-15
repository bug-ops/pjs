---
aliases:
  - benchmark suite plan
tags:
  - sdd
  - plan
  - performance
created: 2026-11-15
status: implemented
related:
  - "[[075-bench-suite/spec]]"
verified: 2026-11-15
---

# Plan 075: as-built

| Bench target | Subject under test | Compared against | Fixture size |
|--------------|--------------------|------------------|--------------|
| `simple_throughput` | `serde_json::from_str`, `sonic_rs::from_str` | each other | small/medium/large(1000 items) |
| `zero_copy_bench` | `ZeroCopyParser`/`LazyParser` | - | strings, objects, arrays, memory_efficiency |
| `serde_overhead_bench` | `pjs-domain` `DomainEvent` serde, value objects | - | single events, batch of 100 |
| `gat_query_benchmarks` | `GatInMemoryStreamRepository` | 1 ms target | 100/500/1000 sessions |
| `http_streaming` | `format_batch_owned`/`BatchFrameStream` | `sonic_rs` vs `serde_json` | 10/100/1000 frames |

## Design decisions

- Criterion with `harness = false`; `black_box` on inputs and outputs.
- `[profile.bench]` inherits `release` (fat LTO, `codegen-units = 1`, `panic = "abort"`) and keeps `debug = true` for flamegraphs. `.cargo/config.toml` adds `-C target-cpu=native` for x86_64/aarch64 hosts, so numbers are CPU specific.
- Setup (repository population, frame construction, `serde_json::Value` preparation) is outside the timed closure.

## Contracts

| Command | Result |
|---------|--------|
| `cargo bench -p pjs-bench` | all five groups, HTML in `target/criterion/report/index.html` |
| `cargo bench -p pjs-bench --bench http_streaming -- --save-baseline <name>` / `--baseline <name>` | baseline comparison documented in `http_streaming.rs` |
| `scripts/profile.sh bench` | all benches with `--profile-time=10`, opens the report on macOS/Linux |

## Testing strategy

Benches have no assertions; compile coverage via `cargo clippy --workspace --all-features --all-targets` in the `quality` CI job. See [[078-ci-pipeline/spec]].

## Security notes

None (benchmarks use synthetic data; `publish = false`).
