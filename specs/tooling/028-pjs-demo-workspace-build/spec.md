---
aliases:
  - pjs-demo Unbuildable
  - Demo Servers Build Regression
tags:
  - sdd
  - spec
  - build-system
  - workspace
  - regression
created: 2026-08-07
status: implemented
priority: P2
issue: "#318"
pr: "#356"
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
code:
  - Cargo.toml
  - crates/pjs-demo/Cargo.toml
  - .github/workflows/ci.yml
  - .claude/rules/continuous-improvement.md
verified: 2026-11-15
---

# Feature: Restore Buildability of `pjs-demo` Demo Servers

> [!info] Verification (2026-11-15)
> **State: implemented** (option (a) of #112 applied).
>
> **Evidence**
> - Root `Cargo.toml`: `members = ["crates/*"]`, `default-members = [pjs-core, pjs-domain, pjs-wasm]`, `exclude = ["crates/pjs-js-client", "fuzz"]` - `crates/pjs-demo` is no longer excluded. `cargo pkgid -p pjs-demo` resolves to `crates/pjs-demo#0.8.0`.
> - Delivered by PR #356 (commit `3b9b426`, "restore pjs-demo as a buildable workspace member"), which closed issue #318 ("pjs-demo remains completely unbuildable ... regression of closed #112"). CHANGELOG entry cites #318.
> - The same PR fixed two latent compile defects (stale `rand::Rng` imports in `crates/pjs-demo/src/data/*.rs`, a clippy `unnecessary_sort_by` in `websocket_streaming.rs`).
> - `.github/workflows/ci.yml`: the `rust` paths-filter includes `crates/pjs-demo/**`; `cargo clippy --workspace --all-features --all-targets -- -D warnings` covers the demo crate; the coverage job runs `--exclude pjs-demo`.
> - Documented commands in `.claude/rules/continuous-improvement.md` use the `pjson-rs/<feature>` prefix (the demo crate declares no `[features]`).
>
> **Drift from original spec**
> - Original spec assumed `-p pjs-demo` could never work; it now works (workspace member). Option (a) chosen, not (b)/(c).
> - Source spec quoted `exclude = ["crates/pjs-demo", "crates/pjs-js-client"]`; the list is now `["crates/pjs-js-client", "fuzz"]`.
> - Feature flags in the spec's example command (`--features "simd-auto,..."`) were wrong for this crate; the working form is `--features "pjson-rs/simd-auto,..."`.
> - `.claude/rules/continuous-improvement.md` (the note "`pjs-demo` is excluded from the workspace ... Use `--manifest-path`" and the Testing Notes line) and `.claude/CLAUDE.md` (workspace table: "excluded from default build") still describe the old exclusion. The `--manifest-path` form still works, but the note is stale. Those files are outside `specs/`; left as-is here.
> - NFR-004 (a CI step that fails if the demo manifest breaks again) is satisfied implicitly by the workspace-wide clippy step; no dedicated step exists.

## 1. Overview

### Problem Statement (historical)

`crates/pjs-demo` was excluded from the Cargo workspace while its manifest used `{ workspace = true }` inheritance for every `[package]` field. Cargo cannot resolve inheritance for an excluded crate, so `cargo run -p pjs-demo`, `cargo build --manifest-path crates/pjs-demo/Cargo.toml` and `cd crates/pjs-demo && cargo build` all failed. Issue #112 (closed 2026-04-28 as `documentation`) proposed three options but none was applied; the defect was filed again as #318.

### Goal

At least one documented command reliably builds and runs each of the three `pjs-demo` binaries (`interactive-demo-server`, `simple-demo-server`, `websocket-streaming-server`).

### Out of Scope

- New demo functionality, endpoints or binaries.
- Publishing `pjs-demo` (`publish = false` is inherited from `[workspace.package]`).
- `pjs-js-client` (npm package, not a Cargo member).
- Restructuring `src/servers/*.rs`.

## 2. User Stories

- US-001: a live tester runs the documented command and gets `interactive-demo-server` listening on port 3000.
- US-002: a developer builds all three demo binaries with one documented command set to catch breakage after a `pjs-core` change.
- US-003: a maintainer can see which of the three #112 options was applied and why (option (a), mirroring `crates/pjs-bench` in PR #190).

## 3. Functional Requirements

Each FR carries a `Status:` line (done | open | superseded).

| ID | Requirement | Priority | Status |
|----|------------|----------|--------|
| FR-001 | WHEN a contributor runs the documented build/run command for `pjs-demo` THE SYSTEM SHALL compile the requested binary without manifest-parsing errors | must | done |
| FR-002 | WHEN `cargo build`/`cargo run` targets `crates/pjs-demo` THE SYSTEM SHALL resolve all `[package]` fields | must | done (member of the workspace) |
| FR-003 | WHEN package metadata changes THE SYSTEM SHALL keep `crates/pjs-demo/Cargo.toml` consistent with `[workspace.package]` | must | done (inherited via `workspace = true`) |
| FR-004 | WHEN the fix is applied THE SYSTEM SHALL update docs referencing the command | must | partial: `.claude/rules/continuous-improvement.md` commands fixed (feature prefix); stale "excluded" notes remain there and in `.claude/CLAUDE.md` |
| FR-005 | IF option (c) is chosen THE SYSTEM SHALL document the demos as unbuildable | must | superseded (option (a) chosen) |
| FR-006 | WHEN verified THE SYSTEM SHALL build all three binaries with `simd-auto,schema-validation,compression,http-server,websocket-server` (via `pjson-rs/` prefix) | must | done |
| FR-007 | WHEN workspace membership changes THE SYSTEM SHALL leave default-member behaviour for `pjs-core`, `pjs-domain`, `pjs-wasm` unchanged | must | done (`default-members` unchanged) |
| FR-008 | WHEN workspace membership changes THE SYSTEM SHALL keep fmt/clippy green on `pjs-demo` | should | done (clippy workspace step in `ci.yml`) |

## 4. Non-Functional Requirements

| ID | Category | Requirement | Status |
|----|----------|-------------|--------|
| NFR-001 | Maintainability | Do not reintroduce the cause of the exclusion | done |
| NFR-002 | Build performance | Default `cargo build` unaffected | done (not in `default-members`) |
| NFR-003 | Documentation accuracy | All references to the demo build command are consistent | partial (see FR-004) |
| NFR-004 | Regression prevention | CI fails if the manifest breaks | done implicitly (workspace-wide clippy) |

## 5. Data Model

Not applicable (build configuration). Relevant files: root `Cargo.toml` (`members`, `default-members`, `exclude`, `[workspace.package]`) and `crates/pjs-demo/Cargo.toml` with three `[[bin]]` entries: `interactive-demo-server` (`src/servers/interactive_demo.rs`), `simple-demo-server` (`src/servers/simple_demo.rs`), `websocket-streaming-server` (`src/bin/websocket_streaming_server.rs`).

## 6. Edge Cases and Error Handling

| Scenario | Behaviour |
|----------|-----------|
| `cargo run -p pjs-demo` | Works (workspace member) |
| `--features "simd-auto,..."` without prefix | Fails: `pjs-demo` declares no `[features]`; use `pjson-rs/simd-auto` |
| Bare `cargo build` / `cargo test` | Does not build `pjs-demo` (not in `default-members`) |
| Coverage job | `--exclude pjs-demo`; also ignored in `.github/codecov.yml` |

## 7. Success Criteria

| ID | Metric | Target | Status |
|----|--------|--------|--------|
| SC-001..003 | each of the three binaries builds from a clean checkout | exit 0 | done (workspace clippy gate) |
| SC-004 | documented command works | verified | done for commands; stale notes remain |
| SC-005 | `pjs-demo` no longer `Blocked` in `.local/testing/coverage-status.md` | row updated | [NEEDS CLARIFICATION: `.local/` is untracked and was not inspected] |

## 8. Agent Boundaries

**Always**: run fmt/clippy/nextest after touching `Cargo.toml`; add a CHANGELOG entry under `[Unreleased]`.
**Ask first**: changing `default-members`; adding `pjs-demo` to CI test/build jobs.
**Never**: make `pjs-demo` publishable; modify unrelated crates as part of manifest fixes.

## 9. Open Questions

- Refresh the stale exclusion notes in `.claude/rules/continuous-improvement.md` and `.claude/CLAUDE.md` (outside the scope of this migration).
- The original reason for excluding `pjs-demo` was never recorded [NEEDS CLARIFICATION: moot after the fix].

## 10. See Also

- [[constitution]], [[MOC-specs]]
- [[029-websocket-stream-session-binding/spec|029 websocket session binding]], [[077-demo-servers/spec|077 demo servers]]
- Issues #112, #318; PR #356
