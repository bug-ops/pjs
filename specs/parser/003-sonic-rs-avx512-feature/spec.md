---
aliases:
  - Expose sonic-rs AVX-512 Feature Flag
  - Spec 003
tags:
  - sdd
  - spec
  - parser
  - simd
created: 2026-04-28
status: implemented
priority: P3
issue: "#116"
pr: "#157"
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
  - "[[001-simd-backend-implementation/spec|001 SIMD backend]]"
  - "[[006-sonic-rs-primary-parser-path/spec|006 primary parser path]]"
code:
  - crates/pjs-core/Cargo.toml
  - crates/pjs-core/build.rs
verified: 2026-11-15
---

# Spec 003: Expose sonic-rs AVX-512 Feature Flag

> [!info] Verification (2026-11-15)
> **State: implemented.** Issue #116 closed 2026-04-28 by PR #157 (`1af0eb6`); CHANGELOG: "`simd-avx512` Cargo feature now forwards to `sonic-rs/avx512` (#116)".
>
> Evidence: `crates/pjs-core/Cargo.toml` has `simd-avx512 = ["sonic-rs/avx512"]`; workspace pins `sonic-rs = "0.5"`. `crates/pjs-core/build.rs` reads `CARGO_FEATURE_SIMD_AVX512` and, on x86_64 without `avx512f` in `CARGO_CFG_TARGET_FEATURE`, emits `cargo::warning` that sonic-rs falls back to scalar; it also sets `cfg(pjs_simd)`.
>
> Drift from original spec:
> - The dependency on Spec 001 is moot: the SIMD backend struct was removed; the flag acts directly on `sonic-rs`.
> - The "graceful fallback or clear compile-time diagnostic" is a build-time warning only (not an error). Cross-target safety relies on `sonic-rs` and on the architecture check in `build.rs` (`target_arch == "x86_64"`).
> - README marks the flag "x86_64 only; needs `-C target-cpu=native`".
> - No benchmark compares AVX2 vs AVX-512 (user story 2 open).

**Type**: enhancement | **Priority**: P3 | **Status**: implemented

## Functional requirements

### FR-001: Forward to sonic-rs
When `simd-avx512` is enabled, the build shall enable the `sonic-rs/avx512` feature. Status: done. Code: `crates/pjs-core/Cargo.toml`.

### FR-002: Depends on Spec 001
Status: superseded (see Verification).

### FR-003: Hardware without AVX-512
When `simd-avx512` is enabled but rustc is not given AVX-512 target features, the build shall warn. Status: done (warning, not hard error). Code: `crates/pjs-core/build.rs`.

## Non-functional requirements

- Cross-compilation (aarch64, wasm32) must not break: no cfg or dependency is gated on the target other than the x86_64 warning branch. [NEEDS CLARIFICATION: `sonic-rs/avx512` on non-x86 targets is not exercised by CI in this repo].

## Open questions

- No AVX-512 benchmark exists in `crates/pjs-bench/benches/`.

## See Also

- [[001-simd-backend-implementation/spec|001]], [[006-sonic-rs-primary-parser-path/spec|006]]
- sonic-rs releases: `https://github.com/cloudwego/sonic-rs/releases`
