---
aliases:
  - Project Principles
  - PJS Constitution
tags:
  - sdd
  - constitution
  - pjs
created: 2026-11-15
status: permanent
related:
  - "[[MOC-specs]]"
  - "[[architecture-baseline]]"
  - "[[testing-strategy]]"
---

# Project Constitution

> [!important]
> Non-negotiable principles governing all development in this repository. Every specification, plan and task under `specs/` must comply. Change this file only by explicit maintainer decision. Detailed, code-anchored inventories live in [[architecture-baseline]] and [[testing-strategy]]; the process rules below are mirrored from `CLAUDE.md`, `CONTRIBUTING.md` and `.claude/rules/`.

## I. Architecture

- Clean / Hexagonal architecture with strict inward dependency direction: `pjs-domain` -> `pjs-core::domain` -> `application` -> `infrastructure`.
- `pjs-domain` stays free of runtime and I/O dependencies (no `tokio`, `axum`, `sonic-rs`, compression, HTTP clients), WASM-compatible, with `unsafe_code` forbidden; it is `std` only and declares no features (the `no_std` mode was removed, #606, see [[042-domain-crate-contract-and-value-objects/spec|042]]).
- Ports (`domain/ports`) are trait-only and contain no infrastructure types; adapters live under `infrastructure/`. One adapter implements one port.
- Async ports use GAT traits (`impl Trait` in associated types, nightly) instead of `async_trait` virtual dispatch; regressing a port to boxed/dynamic dispatch is a defect.
- CQRS boundary: command/query DTOs in `application/` never leak HTTP, axum or storage types; mapping happens at the adapter edge.
- Application handlers orchestrate through ports and do not re-implement domain rules.
- `pjs-wasm` depends on `pjs-domain` plus explicitly exposed APIs only.

## II. Technology Stack

- Rust edition 2024, resolver 3, MSRV 1.89 (declared and checked in CI only for `pjson-rs-domain` and `pjs-wasm`, #608), **nightly toolchain pinned** in `rust-toolchain.toml` (GAT zero-cost ports, SIMD work).
- Primary parser backend: `sonic-rs` (SIMD) selected at runtime in `parser/hybrid.rs`; zero-copy and simple parsers are fallbacks; `jiter` for partial parsing behind `partial-parse`.
- Transport: `axum` 0.8 / `hyper` / `tokio-tungstenite`; compression via `brotli`, `flate2`, `zstd` behind `compression`.
- The `mimalloc` allocator is opt-in and must not conflict with a consumer's `#[global_allocator]`; see [[decisions/adr-001-allocator-decision]].
- YAML is validated with `fy` (fast-yaml); `serde_yaml` and `serde_yml` are not used.
- Published artifacts: crates `pjson-rs`, `pjson-rs-domain` (crates.io) and npm package `pjs-wasm`.

## III. Type Safety (NON-NEGOTIABLE)

- Illegal states are made unrepresentable: newtypes (`SessionId`, `StreamId`, `Priority`, `JsonPath`, bounded size/depth types), enums and typestate instead of booleans, raw primitives or stringly-typed data.
- Value objects with invariants expose validating constructors (`TryFrom` / `new() -> Result`); infallible `From` that can produce invalid data is not allowed.
- No `any` / untyped maps where a concrete type expresses the same information; no silent unchecked casts, and lossy `as` casts on untrusted sizes are replaced by checked conversions.
- No `unwrap` / `expect` / `panic!` on reachable error conditions outside tests.
- Every `unsafe` block carries a one-line invariant comment and honours `unsafe_op_in_unsafe_fn`.

## IV. Testing (NON-NEGOTIABLE)

- All behaviour changes ship with tests; a defect fix ships with a regression test that fails without the fix.
- Prefer integration tests with real dependencies over mocks.
- Local gate equals CI gate (see VIII). Coverage threshold is enforced in CI.
- Untrusted-input parsers and decoders have a fuzz target (`fuzz/`); see [[testing-strategy]].
- WASM and JS client changes run `wasm-pack test` and the `pjs-js-client` npm tests.

## V. Security

- All iteration, buffering and recursion over untrusted input is bounded (size, depth, count, time); limits come from validated config types in `config/security.rs` and `security/`.
- Decompression always goes through the bomb-guarded paths (`compression/secure.rs`, `security/compression_bomb.rs`).
- Secrets and API keys never appear in code, logs or `Debug` output (redacting `Debug` for credential-bearing configs); secrets are not read from environment variables or `.env` files.
- Vulnerabilities are reported through `SECURITY.md` private advisories, never as public issues.
- GitHub Actions are pinned to full commit SHAs with a trailing version comment.

## VI. Performance

- Performance is a core value: SIMD parsing, GAT zero-overhead ports, zero-copy where possible.
- Every change touching `parser/`, `stream/priority.rs` or `domain/ports/gat/` is benchmarked (`cargo bench -p pjs-bench`); a regression above 10 percent is P1.
- No avoidable allocations or lost SIMD eligibility on hot paths.

## VII. Simplicity and MVP discipline

- Before v1.0.0 implement the minimum necessary, add no premature abstraction, and do not keep backward-compatibility shims; document every breaking change in `CHANGELOG.md`.
- Follow DRY: search for an existing implementation before adding one.
- New dependencies need justification and a version check against current releases.
- Comments are minimal: only for genuine complexity, one line. Public API items (`pub` types, traits, functions) always carry `///` docs, with runnable `# Examples` for non-trivial APIs.

## VIII. Git, CI and Documentation Workflow

- Branches: `feat/issue-{n}/{slug}`, `fix/{n}-{slug}`, `hotfix/{n}-{slug}`. Commits follow Conventional Commits with the project scopes (`pjson-rs`, `pjson-rs-domain`, `wasm`, `parser`, `stream`, `http`, `security`, `bench`, `ci`), no emoji.
- Pre-commit gate: `cargo +nightly fmt --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`; `cargo nextest run --workspace --all-features --lib --bins`; `cargo test --workspace --doc --all-features`; rustdoc with `--deny rustdoc::broken_intra_doc_links --deny rustdoc::private_intra_doc_links --deny rustdoc::redundant_explicit_links` when public API changed; WASM build and tests when `pjs-wasm` changed.
- `CHANGELOG.md` is updated per PR under `[Unreleased]`: one line per change ending with the PR number.
- Issues carry exactly one `P0`-`P4` label; a PR closing several issues lists each `Closes #N` on its own line.
- Documentation, comments, specs and plans are written in English.

## See Also

- [[MOC-specs]] - all specifications
- [[architecture-baseline]] - verified design-pattern inventory
- [[testing-strategy]] - test layers and CI gates
