---
aliases:
  - pjs-bench Workspace Isolation
  - Bench crate build fix
tags:
  - sdd
  - spec
  - build-system
  - workspace
  - bench
created: 2026-04-28
status: implemented
priority: P2
issue: "#179"
pr: "#190"
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
  - "[[028-pjs-demo-workspace-build/spec|Spec 028]]"
code:
  - Cargo.toml
  - crates/pjs-bench/Cargo.toml
verified: 2026-11-15
---

# Spec 021: pjs-bench - Workspace Isolation Pattern

> [!info] Verification (2026-11-15)
> **State: implemented** (Option A). Issue #179 CLOSED 2026-04-28; PR #190 (`a0a8d83`, "restore pjs-bench as a workspace member"); CHANGELOG: "`pjs-bench` benchmark crate restored as a workspace member".
>
> Evidence: root `Cargo.toml` has `members = ["crates/*"]` and `default-members = ["crates/pjs-core", "crates/pjs-domain", "crates/pjs-wasm"]`; `exclude = ["crates/pjs-js-client", "fuzz"]`. `crates/pjs-bench/Cargo.toml` uses `{ workspace = true }` inheritance and builds as a member. Five benches exist in `crates/pjs-bench/benches/`.
>
> Drift from original spec:
> - Spec proposed removing both `pjs-bench` and `pjs-demo` from `exclude`. #190 restored only `pjs-bench`; `pjs-demo` followed later in #318 (see [[028-pjs-demo-workspace-build/spec|Spec 028]]). Today neither is excluded.
> - Project docs (`CLAUDE.md`, `.claude/rules/continuous-improvement.md`) still describe `pjs-demo` as excluded; the root `Cargo.toml` does not. [NEEDS CLARIFICATION: doc cleanup owner]
> - The `fallback` module, `BenchSuite`, `BenchMetrics` and dead features in `pjs-bench` were later removed (#419, #324).

## Problem

`pjs-bench` was in `[workspace] exclude` while using `{ workspace = true }` for all fields and dependencies; Cargo cannot resolve inheritance for a non-member, so the crate did not compile.

## Functional Requirements

### FR-1 `cargo bench -p pjs-bench` succeeds
Status: done.

### FR-2 Bench crate stays out of default workspace builds
Status: done via `default-members` (bare `cargo build`/`cargo test` build only core, domain, wasm).

### FR-3 No other crate is broken
Status: done (CI green on #190).

## Decision record

| Option | Verdict |
|--------|---------|
| A: member + `default-members` | chosen, idiomatic |
| B: explicit versions in bench crate | rejected, drift |
| C: nested workspace | rejected, unsupported/complex |

## Notes

`cargo build --workspace` includes `pjs-bench` and `pjs-demo`; CI jobs account for this (for example `--exclude pjs-demo` in `ci.yml`).

## See Also

- [Cargo reference: default-members](https://doc.rust-lang.org/cargo/reference/workspaces.html#the-default-members-field)
- [[023-implementation-roadmap/spec|Spec 023]]
