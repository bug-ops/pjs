---
aliases:
  - ADR-001
  - Allocator decision
tags:
  - sdd
  - adr
  - decision
  - allocator
  - pjs
created: 2026-04-28
status: accepted
decided: 2026-04-28
verified: 2026-11-15
issue: "#160"
pr: "#171"
code:
  - crates/pjs-core/src/global_alloc.rs
  - crates/pjs-core/src/parser/aligned_alloc.rs
  - crates/pjs-core/src/parser/buffer_pool.rs
  - crates/pjs-core/Cargo.toml
  - .github/workflows/ci.yml
related:
  - "[[constitution]]"
  - "[[architecture-baseline]]"
  - "[[testing-strategy]]"
  - "[[MOC-specs]]"
---

# ADR-001: Allocator architecture (mimalloc only, real `#[global_allocator]`)

> [!info] Verification (2026-11-15)
> Migrated from the untracked `.local/allocator-decision.md` (status "Proposed", 2026-04-28). **Implemented**: merged as PR #171 ("consolidate allocators", closes issue #160) on 2026-04-28, first released in v0.5.2 (`CHANGELOG.md` 0.5.2 section; `git tag --contains 05b0c51` lists v0.5.2). The migration steps in the original document were carried out; current code is verified below. Status changed from Proposed to **Accepted (implemented)**.
>
> **Drift between the proposal and the as-built code:**
> - `global_allocator_name()` is a plain `pub fn`, not the proposed `pub const fn name()` re-exported under an alias.
> - The proposed `mimalloc = ["dep:mimalloc"]` feature is as designed; `libmimalloc-sys` and all `tikv-jemalloc-*` dependencies are gone from the manifests.
> - The original design said the CI `--all-features` step "loses its meaning"; the as-built CI builds with `--features "default${extra_features}"` where `extra_features` is `,mimalloc` for the mimalloc leg (see below). The workspace-wide clippy and the coverage job still use `--all-features`.
> - `README.md` says "The `jemalloc` feature was removed in v0.6.0"; the removal actually shipped in v0.5.2 (CHANGELOG). README wording is stale.
> - `.claude/CLAUDE.md` still lists a `jemalloc` feature ("mutually exclusive" with `mimalloc`) and a `jemalloc` build example; both are stale.
> - The `.github/workflows/ci.yml` comment on the `build` job says "Windows skips mimalloc", but the matrix is `os: [ubuntu, macos, windows] x allocator: [system, mimalloc]` with no exclusion (6 build legs, 24 test jobs with 4 partitions).
> - Open follow-ups 1 to 3 (below) are **not done**.

## Status

Accepted and implemented (PR #171, v0.5.2).

## Context

Before the change, the crate shipped `jemalloc` and `mimalloc` features that linked the native libraries (`extern crate libmimalloc_sys; extern crate tikv_jemalloc_sys;`) but never registered a `#[global_allocator]`; every Rust allocation still used the system allocator. The custom `SimdAllocator` in `parser/allocator.rs` was a separate explicit aligned allocator (used by `parser/buffer_pool.rs`) whose accessor was misleadingly named `global_allocator()`. The `Cargo.toml` comment claimed the two features were mutually exclusive but nothing enforced it, CI multiplied three allocator dimensions across three operating systems, and `jemalloc` needed a permanent Windows exclusion. No allocator benchmark existed, so the choice was made on workload reasoning.

Workload profile (priority queue with many small `Frame` allocations, tokio tasks and per-connection state across many threads; arena and aligned buffers bypass the global allocator; WASM uses the host allocator) favours a thread-local-heap small-object allocator. The 1.82x GAT performance claim is unrelated to the allocator (it comes from removing `async_trait` virtual dispatch).

## Decision

1. Keep exactly one non-default allocator for native targets: **mimalloc**, behind the `mimalloc` feature, registered as `#[global_allocator]`. Opt-in, not in `default`.
2. Remove the `jemalloc` feature and all `tikv-jemalloc-*` dependencies outright (pre-1.0, no deprecation cycle).
3. WASM (`target_arch = "wasm32"`) never uses mimalloc: the registration is guarded with `not(target_arch = "wasm32")`.
4. Decouple and rename the explicit aligned allocator: `SimdAllocator` -> `AlignedAllocator`, `parser::allocator` -> `parser::aligned_alloc`, `global_allocator()` -> `aligned_allocator()`. It uses only `std::alloc::{alloc, dealloc, realloc}`, so it routes through whichever global allocator is registered.
5. Drop `AllocatorBackend`, `AllocatorStats`, the jemalloc `stats()` API and `initialize_global_allocator`.
6. Collapse the CI allocator matrix to `system` and `mimalloc` on all three operating systems.

Rejected alternative: keep all three allocators behind features. Reasons: the features did not change the global allocator, CI cost was real, and removing obsolete constructs is the pre-1.0 policy. jemalloc can return as a single new feature if measurements justify it.

| Criterion | jemalloc (`tikv-jemallocator`) | mimalloc |
|---|---|---|
| Small-object, multi-thread throughput | very good | excellent |
| Windows MSVC | broken (was excluded in CI) | works |
| macOS | works | works |
| WASM | unsupported | unsupported |
| Introspection | rich | minimal |

The deciding factor was uniform platform support. No project benchmark backs either choice.

## As-built state (verified 2026-11-15)

| Aspect | Evidence |
|---|---|
| Global allocator registration | `crates/pjs-core/src/global_alloc.rs`: `#[cfg(all(feature = "mimalloc", not(target_arch = "wasm32")))] #[global_allocator] static GLOBAL: mimalloc::MiMalloc` |
| Diagnostics | `pjson_rs::global_allocator_name() -> &'static str` returns `"mimalloc"` or `"system"`; re-exported in `crates/pjs-core/src/lib.rs`; tested in `crates/pjs-core/tests/parser_allocator_comprehensive.rs` (`test_global_allocator_name`) |
| Feature | `mimalloc = ["dep:mimalloc"]` in `crates/pjs-core/Cargo.toml`; not in `default` (default = `simd-auto`, `schema-validation`, `compression`, `http-server`, `websocket-server`, `websocket-client`); `cargo-machete` ignore entry for `mimalloc` |
| Removed | no `jemalloc`, `tikv-jemalloc*`, `libmimalloc-sys` in any `Cargo.toml`; no `extern crate` linkage in `lib.rs` |
| Aligned allocator | `crates/pjs-core/src/parser/aligned_alloc.rs`: zero-sized `AlignedAllocator`, `aligned_allocator()` singleton accessor; used three times in `parser/buffer_pool.rs`; re-exported from `parser/mod.rs` |
| Library-consumer caveat | `global_alloc.rs` module docs: enabling `mimalloc` in a binary that declares its own `#[global_allocator]` causes a link-time conflict |
| WASM | `pjs-wasm` depends only on `pjson-rs-domain`; the feature is also cfg-guarded |
| CI | `.github/workflows/ci.yml`: `build` matrix `os x [system, mimalloc]`; `extra_features: ",mimalloc"` for the mimalloc leg; `cargo nextest archive --workspace --exclude pjs-wasm --features "default${{ matrix.extra_features }}" --lib --bins`; `test` matrix `os x allocator x partition 1..4` (24 jobs) |
| Release | `release.yml` has no allocator dimension |

## Consequences

- Positive: `mimalloc` now has a real runtime effect; one non-default allocator to maintain; no Windows asterisk; `--all-features` builds no longer enable two competing allocator features.
- Negative: no allocator-specific introspection (jemalloc stats removed); binaries using the `mimalloc` feature cannot declare their own `#[global_allocator]`; mimalloc adds a C build step for consumers who enable it.
- Breaking changes (documented in CHANGELOG 0.5.2): removal of the `jemalloc` feature, `SimdAllocator`/`parser::allocator`, `AllocatorBackend`, `AllocatorStats`, `initialize_global_allocator`, `global_allocator()`.

## Open follow-ups (from the original proposal, still open)

1. **Allocator benchmark.** `crates/pjs-bench/benches/` contains `simple_throughput`, `zero_copy_bench`, `serde_overhead_bench`, `gat_query_benchmarks`, `http_streaming`; the proposed `allocator_compare.rs` (priority queue and reconstruction under system vs mimalloc, multi-threaded) does not exist, so the choice is still unmeasured. [NEEDS CLARIFICATION: no GitHub issue found for this follow-up; the 2026-11-15 bench A/B was inconclusive due to machine load, see #619.]
2. **mimalloc as default.** Still opt-in; deferred until benchmark data exists (risk: forces a C build on every downstream consumer).
3. **Allocator name in startup logs.** `global_allocator_name()` has no caller outside its own tests and doc-test (`rg global_allocator_name` shows no use in `pjs-demo` or `pjs-bench`).

## See also

- [[architecture-baseline]] sections 4 (GAT pattern, 1.82x claim) and 12 (feature flags)
- [[testing-strategy]] (CI allocator matrix, `parser_allocator_comprehensive`)
- `CHANGELOG.md`, v0.5.2 (#160, #171)
