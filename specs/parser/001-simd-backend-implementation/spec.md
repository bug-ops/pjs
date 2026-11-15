---
aliases:
  - SIMD Backend Implementation
  - Spec 001
tags:
  - sdd
  - spec
  - parser
  - simd
  - obsolete
created: 2026-04-28
status: obsolete
priority: P3
issue: "#115"
pr: "#157"
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
  - "[[003-sonic-rs-avx512-feature/spec|003 AVX-512 flag]]"
  - "[[006-sonic-rs-primary-parser-path/spec|006 sonic-rs primary path]]"
code:
  - crates/pjs-core/Cargo.toml
  - crates/pjs-core/build.rs
  - .cargo/config.toml
  - crates/pjs-core/src/parser/simd/mod.rs
verified: 2026-11-15
---

# Spec 001: SIMD Backend Implementation

> [!info] Verification (2026-11-15)
> **State: obsolete (superseded and then removed).** The `HybridParser`/`SimdBackend` design this spec targeted no longer exists.
>
> Evidence:
> - Issue #115 closed 2026-04-28; delivered by PR #157 (`1af0eb6`, "activate SIMD backend and wire AVX-512 feature flag") together with #137 (`3356eaa`), which deleted `parser/hybrid.rs` (`HybridParser`, `SimdBackend`, `SerdeBackend`) as never-wired dead code (CHANGELOG, "Removed", #126).
> - The replacement `Parser`/`SonicParser`/`SimpleParser` facade was itself removed in #495 (`fd15815`, CHANGELOG "BREAKING ... `pjson_rs::{Parser, SimpleParser, SonicParser, ParseConfig, ParseStats}`"). `rg 'HybridParser|SimdBackend' crates` returns no hits.
> - Remaining SIMD surface: Cargo feature `simd-auto` (empty feature, default) and `simd-avx512 = ["sonic-rs/avx512"]` in `crates/pjs-core/Cargo.toml`; `crates/pjs-core/build.rs` (emits `cfg(pjs_simd)`, which has no readers in `pjs-core`); `.cargo/config.toml` (`-C target-cpu=native` for x86_64/aarch64); `parser/simd/mod.rs` (`SimdClassifier`, `SimdNumericOps` over `sonic_rs` values, used only by tests).
>
> Drift from original spec:
> - `simd-avx2`, `simd-neon`, `simd-sse42` features were removed (#368, `ab5aeec`); only `simd-auto` and `simd-avx512` remain.
> - Runtime CPU detection is not implemented by PJS; it is delegated to `sonic-rs`'s own dispatch.
> - No `HybridParser::simd_parser` exists; no production parse path in `pjs-core` selects between serde and sonic-rs. HTTP ingestion uses axum `Json<T>`; `pjs-wasm` parses with `serde_json`.
> - FR5 (benchmark confirming a SIMD speedup) is not satisfied as stated; `crates/pjs-bench/benches/simple_throughput.rs` uses `sonic_rs` directly, not a feature-selected parser.

**Type**: enhancement | **Priority**: P3 | **Status**: obsolete

## Original problem

`HybridParser` declared `simd_parser: Option<SimdBackend>` where `SimdBackend` was a stub (`_placeholder: ()`) and always `None`; `simd-*` features had no effect.

## Functional requirements (final disposition)

| FR | Requirement | Status |
|----|-------------|--------|
| 1 | `SimdBackend` delegates to `sonic-rs` | superseded: struct removed; `sonic-rs` is a direct dependency used for serialization (`infrastructure/http/streaming.rs`) and in `parser/simd/mod.rs` |
| 2 | `simd-auto` performs runtime CPU detection | superseded: handled inside `sonic-rs`; `simd-auto` only sets `cfg(pjs_simd)` in `build.rs` |
| 3 | `HybridParser::simd_parser` is `Some` on supported platforms | superseded: type removed |
| 4 | Existing parser correctness tests keep passing | done |
| 5 | Benchmarks confirm SIMD improvement | open: [NEEDS CLARIFICATION: no bench compares `simd-auto` vs `--no-default-features`] |

## Non-functional requirements

- No new `unsafe`: met (build script and config only).
- WASM/fallback builds compile without SIMD: met; `.cargo/config.toml` applies `target-cpu=native` only to `x86_64`/`aarch64` target cfgs.

## See Also

- [[006-sonic-rs-primary-parser-path/spec|006]] - flag wiring and removal of the dead parser
- [[003-sonic-rs-avx512-feature/spec|003]] - `simd-avx512`
- `docs/architecture/SPECIFICATION.md` for the wire protocol
