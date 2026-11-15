---
aliases:
  - SIMD Features and Global Allocator
  - Feature Flag Matrix (parser and memory)
tags:
  - sdd
  - spec
  - simd
  - features
  - allocator
  - build
created: 2026-11-15
status: implemented
priority: P3
code:
  - crates/pjs-core/Cargo.toml
  - crates/pjs-core/build.rs
  - .cargo/config.toml
  - crates/pjs-core/src/global_alloc.rs
  - crates/pjs-core/src/parser/simd/mod.rs
  - crates/pjs-core/tests/parser_simd_classifier_comprehensive.rs
  - crates/pjs-core/tests/parser_allocator_comprehensive.rs
related:
  - "[[001-simd-backend-implementation/spec]]"
  - "[[003-sonic-rs-avx512-feature/spec]]"
  - "[[006-sonic-rs-primary-parser-path/spec]]"
  - "[[034-fused-utf8-parse-validation/spec]]"
  - "[[040-buffer-pool-aligned-alloc/spec]]"
  - "[[038-zero-copy-lazy-json-parser/spec]]"
  - "[[039-partial-json-jiter-parser/spec]]"
verified: 2026-11-15
---

# Spec 041: SIMD Features, Feature Matrix and Global Allocator

Part of [[MOC-specs]]. Rules in [[constitution]]. Plan: [[041-simd-features-and-global-allocator/plan|plan]].

> [!info] Relation to older specs
> [[001-simd-backend-implementation/spec|001]] (SIMD backend stub), [[003-sonic-rs-avx512-feature/spec|003]] (AVX-512 flag) and [[006-sonic-rs-primary-parser-path/spec|006]] (sonic-rs as primary parser) describe the pre-removal state. `HybridParser` was deleted (#126), `Parser`/`SonicParser`/`SimpleParser` were deleted afterwards (CHANGELOG `[Unreleased]`, #486/#488), the per-ISA features `simd-avx2`, `simd-neon`, `simd-sse42` were removed (0.6.x), and `jemalloc` was removed (#160). This spec records the surviving behaviour. There is no parser selection logic left: `parser/` has no runtime backend choice.

## Overview

**Problem.** Contributors and downstream users need one place that states what the SIMD, allocator and parser-related Cargo features really do, so flags are not assumed to change behaviour they do not change.

**Goal.** Document, as verified behaviour: the SIMD flags and build script, the `.cargo/config.toml` codegen setting, the `sonic-rs` based `parser::simd` helpers, the `mimalloc` global allocator and its diagnostic function, and the interplay with `partial-parse` and `schema-validation`.

**Out of scope.** Parser implementations ([[038-zero-copy-lazy-json-parser/spec|038]], [[039-partial-json-jiter-parser/spec|039]]); buffer pool ([[040-buffer-pool-aligned-alloc/spec|040]]); compression features and transport features ([[069-transport-features-and-limits/spec|069]]).

## User Stories

- As a downstream user, I enable `mimalloc` and `global_allocator_name()` tells me which allocator is active.
- As a maintainer, I can tell from this spec that `simd-auto` does not select a code path.
- As a builder cross-compiling, I know `.cargo/config.toml` forces `target-cpu=native` on x86_64/aarch64 and that I must override it.

## Functional Requirements

### FR-041-001 Feature matrix

`pjson-rs` shall expose these parser/memory-related features (from `crates/pjs-core/Cargo.toml`):

| Feature | Default | Effect |
|---|---|---|
| `simd-auto` | yes | empty feature; makes `build.rs` emit `cfg(pjs_simd)` and, on x86_64 without AVX2/SSE4.2 target features, a `cargo::warning` |
| `simd-avx512` | no | forwards to `sonic-rs/avx512`; makes `build.rs` emit `cfg(pjs_simd)` and warn if `avx512f` is not a target feature |
| `mimalloc` | no | `dep:mimalloc`; registers `MiMalloc` as `#[global_allocator]` on non-wasm32 targets |
| `partial-parse` | no | `dep:jiter`; compiles `parser::partial` ([[039-partial-json-jiter-parser/spec|039]]) |
| `schema-validation` | yes | `dep:regex`; enables regex `pattern` checks in `ValidationService` |

Status: done.

### FR-041-002 `pjs_simd` cfg has no readers

`build.rs` shall declare `cfg(pjs_simd)` via `cargo::rustc-check-cfg` and emit it when `simd-auto` or `simd-avx512` is enabled. No Rust source in the workspace tests `cfg(pjs_simd)`.
Status: done as built; the cfg is vestigial (see Open Questions).

### FR-041-003 Build-script diagnostics

When `simd-avx512` is enabled on `x86_64` and `avx512f` is absent from `CARGO_CFG_TARGET_FEATURE`, `build.rs` shall print a `cargo::warning` naming `RUSTFLAGS="-C target-cpu=native"`. When `simd-auto` is enabled on `x86_64` and neither `avx2` nor `sse4.2` is present, it shall print a warning that notes `pjs_simd` has no readers in `pjs-core`. On `aarch64` and other architectures no warning is emitted.
Status: done.

### FR-041-004 Native codegen by default

The workspace `.cargo/config.toml` shall set `rustflags = ["-C", "target-cpu=native"]` for `cfg(any(target_arch = "x86_64", target_arch = "aarch64"))`. Artifacts built from the repository are therefore not portable across CPU generations; `crates.io` publishes source only.
Status: done. CI release jobs override with `RUSTFLAGS="-D warnings"` (comment in `.cargo/config.toml`).

### FR-041-005 SIMD value classification helpers

`parser::simd::SimdClassifier` shall provide `classify_value_type(&sonic_rs::Value) -> ValueClass`, `is_numeric_array` (requires length >= 3 and all numbers), `calculate_total_string_length`, and `scan_object_keys -> KeyScanResult` (flags for `timestamp`/`time`, `coordinates`/`coord`, `type`, plus `key_count`). `SimdNumericOps` shall provide `fast_array_sum`, `array_stats -> ArrayStats { min, max, sum, count }` with `mean()`.
These helpers use `sonic-rs` accessors and ordinary iteration; they contain no SIMD intrinsics and do not depend on any feature flag.
Status: done. Tests: `tests/parser_simd_classifier_comprehensive.rs`, unit tests in `parser/simd/mod.rs`.

### FR-041-006 Global allocator registration

Where the `mimalloc` feature is enabled and the target is not `wasm32`, `global_alloc.rs` shall declare `#[global_allocator] static GLOBAL: mimalloc::MiMalloc`. Without the feature, or on `wasm32`, the system allocator shall be used.
Status: done (#160, #171).

### FR-041-007 Allocator diagnostic

`pjson_rs::global_allocator_name() -> &'static str` shall return `"mimalloc"` when FR-041-006 applies and `"system"` otherwise.
Status: done. Test: `tests/parser_allocator_comprehensive.rs`; doc test in `global_alloc.rs`.

### FR-041-008 Mutually exclusive allocators removed

There shall be exactly one optional allocator feature (`mimalloc`). A second `#[global_allocator]` in a downstream binary conflicts and fails at link/compile time; this is documented in the module docs.
Status: done. `jemalloc` was removed (CHANGELOG, #160).

### FR-041-009 Schema validation interplay

Where `schema-validation` is disabled, `ValidationService` shall still validate type, range, length, enum, array and object constraints but shall ignore a string schema's `pattern` (the pattern is bound to `_` and no error is produced). Where enabled, patterns longer than 1024 bytes (default; hard ceiling 8 KiB) shall yield `SchemaValidationError::PatternTooLong`, compiled regexes shall be cached per service instance (at most 64 entries, flushed when full), and compile limits shall be 1 MiB program size and 64 KiB DFA cache.
Code: `crates/pjs-core/src/domain/services/validation_service.rs`. The `Schema` data type itself lives in [[043-domain-schema-depth-guard-wire-encoding/spec|043]].
Status: done.

### FR-041-010 `sonic-rs` is an unconditional dependency

`sonic-rs` shall be a non-optional dependency of `pjson-rs`. Production use is `sonic_rs::to_vec` in `infrastructure/http/streaming.rs` (NDJSON/SSE/binary frame serialisation) and `parser::simd`. No feature selects between `sonic-rs` and `serde_json` for parsing.
Status: done.

## Non-Functional Requirements

- NFR-PERF-041-001: SIMD acceleration comes from `sonic-rs` runtime dispatch plus `target-cpu=native`; this repository sets no throughput target for the flags themselves. Benchmarks: `crates/pjs-bench/benches/simple_throughput.rs` (`sonic_rs_parsing` group), `http_streaming.rs`.
- NFR-COMPAT-041-001: `mimalloc` is built in CI on all three OS (matrix `system` x `mimalloc`) with `--features "default,mimalloc"`; the coverage job builds with `--all-features`.
- NFR-COMPAT-041-002: `wasm32` builds must not register mimalloc (FR-041-006).

## Data Model

| Item | Type |
|---|---|
| `ValueClass` | enum: `Object`, `Array`, `String`, `Integer`, `UnsignedInteger`, `Float`, `Boolean`, `Null` |
| `KeyScanResult` | struct, `Default`: three `bool` flags plus `key_count: usize` |
| `ArrayStats` | struct: `min`, `max`, `sum`, `count` |
| `SimdClassifier`, `SimdNumericOps` | unit structs with associated functions |
| `SimdType` | enum in `parser::buffer_pool`: `Sse`, `Avx2`, `Avx512`, `Neon` (alignment queries only) |

## Edge Cases and Errors

| Case | Behaviour |
|---|---|
| `fast_array_sum` on an array with a non-number | `None` |
| `fast_array_sum` / `array_stats` on empty or all-non-`f64` | `None` |
| `is_numeric_array` with fewer than 3 elements | `false` |
| `simd-avx512` on a CPU without AVX-512 | build succeeds; `sonic-rs` falls back at its own runtime dispatch `[NEEDS CLARIFICATION: confirm sonic-rs 0.5 avx512 feature performs runtime detection rather than assuming CPU support]` |
| enabling `mimalloc` in a binary that already has a global allocator | compile error (duplicate `#[global_allocator]`) |

## Success Criteria

- `cargo build -p pjson-rs --features mimalloc` compiles (exercised by the CI `mimalloc` matrix leg).
- `global_allocator_name()` returns `"mimalloc"` in the mimalloc CI variant and `"system"` otherwise.
- No new Cargo feature is added that has no code or dependency effect.

## Agent Boundaries

- Always: gate platform-specific allocator code on `not(target_arch = "wasm32")`; keep `build.rs` check-cfg declarations in sync with any cfg it emits.
- Ask first: removing `simd-auto` (it is a default feature, so removal is breaking); raising `target-cpu=native` scope.
- Never: reintroduce per-ISA feature flags that map to the same behaviour (removed for exactly that reason); add a second allocator feature without an exclusivity guard.

## Open Questions

- `simd-auto` and the `pjs_simd` cfg do nothing at compile time except trigger a warning; the dependency `sonic-rs` is built with its default features regardless. Either remove the feature (breaking) or give it a reader. The `build.rs` doc itself says its consumer was removed in #486/#488.
- `SimdClassifier`, `SimdNumericOps`, `ValueClass`, `KeyScanResult`, `ArrayStats` have no caller outside tests; `vectorized_*` helpers are loop-based "chunking" with no measurable difference from the scalar branch, despite names and comments referring to SIMD/AVX2. Candidate for removal following the #261 precedent.
- `KeyScanResult` hard-codes domain-specific keys (`timestamp`, `coordinates`, `type`) in a generic scanner.
- `fast_array_sum` returns `None` on the first non-number but silently skips numbers that do not fit `f64`; `ValueClass::Null` is returned for any value that is not number/string/array/object/bool.
- `README.md` lists `simd-avx512` as requiring `-C target-cpu=native`; because `.cargo/config.toml` already applies it, the statement is only relevant outside this repository.
- Older specs [[001-simd-backend-implementation/spec|001]], [[003-sonic-rs-avx512-feature/spec|003]], [[006-sonic-rs-primary-parser-path/spec|006]], [[034-fused-utf8-parse-validation/spec|034]] should be marked obsolete or superseded by the migrators; this spec does not edit them.
- `mimalloc` registers as the allocator from a library crate, so the choice is made by feature unification across the dependency graph, not by the final binary author alone.

## See Also

- [[041-simd-features-and-global-allocator/plan|Plan (as built)]]
- [[040-buffer-pool-aligned-alloc/spec|040 Buffer pool and aligned allocator]]
- [[078-ci-pipeline/spec|078 CI pipeline]]
- `README.md` (feature table), `CHANGELOG.md` (#125, #126, #160, #171, #486, #488)
