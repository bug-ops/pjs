---
aliases:
  - Wire sonic-rs as Primary Parser via SIMD Flags
  - Spec 006
tags:
  - sdd
  - spec
  - parser
  - simd
  - build
created: 2026-04-28
status: implemented
priority: P2
issue: "#125"
pr: "#137"
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
  - "[[001-simd-backend-implementation/spec|001 SIMD backend]]"
  - "[[003-sonic-rs-avx512-feature/spec|003 AVX-512 flag]]"
code:
  - crates/pjs-core/build.rs
  - .cargo/config.toml
  - crates/pjs-core/Cargo.toml
verified: 2026-11-15
---

# Spec 006: Wire sonic-rs as the Active Parser Through SIMD Feature Flags

> [!info] Verification (2026-11-15)
> **State: implemented, with the targeted parser code later removed.** Issue #125 closed 2026-04-28 by commit `3356eaa` (#137).
>
> Evidence: `.cargo/config.toml` sets `rustflags = ["-C", "target-cpu=native"]` for `cfg(any(target_arch = "x86_64", target_arch = "aarch64"))`. `crates/pjs-core/build.rs` translates `simd-auto`/`simd-avx512` into `cfg(pjs_simd)` and warns when AVX2/SSE4.2/AVX-512 target features are not exposed. `parser/hybrid.rs` deleted (FR-006a); `Parser::{zero_copy_simd, use_zero_copy}` deleted (FR-007), then `Parser`/`SonicParser`/`SimpleParser` removed in #495 (`fd15815`). #368 (`ab5aeec`) removed `simd-avx2`/`simd-neon`/`simd-sse42` as behaviourally identical to `simd-auto`.
>
> Drift from original spec:
> - Mechanism is `.cargo/config.toml` (`target-cpu=native`), not per-feature `rustflags`; `build.rs` only diagnoses and sets a cfg (Cargo cannot set `-C target-feature` from a build script).
> - `cfg(pjs_simd)` currently has no readers in `pjs-core` (the build.rs warning text says its consumer was removed in #486/#488).
> - `release.yml` sets `RUSTFLAGS="-D warnings"` at job level, overriding `.cargo/config.toml`, so CI/release artifacts are not built with `target-cpu=native`; crates.io publishes source only (comment in `.cargo/config.toml`).
> - FR-002/003/004 (per-ISA flags) superseded: features `simd-avx2`, `simd-neon` removed; AVX-512 handled via `sonic-rs/avx512` ([[003-sonic-rs-avx512-feature/spec|003]]).
> - FR-008 (benchmark confirming speedup) open.

**Type**: enhancement | **Priority**: P2 | **Status**: implemented

## Functional requirements

| FR | Requirement | Status |
|----|-------------|--------|
| 1 | `simd-auto` activates sonic-rs SIMD codegen | done via `.cargo/config.toml` (`target-cpu=native`), host builds only |
| 2 | `simd-avx2` adds `+avx2` | superseded: feature removed (#368) |
| 3 | `simd-avx512` adds AVX-512 flags | partial: forwards `sonic-rs/avx512`; target features come from `target-cpu=native` |
| 4 | `simd-neon` (aarch64) adds `+neon` | superseded: feature removed; NEON is baseline on aarch64 |
| 5 | Gate by target arch | done: `.cargo/config.toml` target cfg; `build.rs` x86_64 branch |
| 6 | Delete or canonicalise `hybrid.rs` | done: deleted |
| 7 | Wire or remove `zero_copy_simd` | done: removed (then whole `Parser` removed) |
| 8 | Benchmark `simd-auto` vs `--no-default-features` | open |

## Non-functional requirements

- No new `unsafe`: met.
- Non-AVX2 / WASM platforms still compile: met (`target-cpu=native` only for x86_64/aarch64 host builds; wasm32 unaffected).

## Open questions

- `pjs_simd` cfg is emitted but unread; either give it a consumer or drop it from `build.rs` (and then `simd-auto` becomes a pure marker feature).
- Cross-compiling to a baseline CPU requires overriding `.cargo/config.toml` rustflags, as the file's comment notes.

## See Also

- [[001-simd-backend-implementation/spec|001]], [[003-sonic-rs-avx512-feature/spec|003]]
- `crates/pjs-bench/benches/simple_throughput.rs`
