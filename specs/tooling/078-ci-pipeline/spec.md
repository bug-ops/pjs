---
aliases:
  - CI pipeline
  - continuous integration
tags:
  - sdd
  - spec
  - ci
  - supply-chain
created: 2026-11-15
status: implemented
priority: P1
related:
  - "[[079-release-pipeline/spec]]"
  - "[[022-ci-js-client-enforcement/spec]]"
  - "[[072-wasm-packaging-targets/spec]]"
  - "[[076-fuzz-targets/spec]]"
  - "[[075-bench-suite/spec]]"
  - "[[077-demo-servers/spec]]"
code:
  - .github/workflows/ci.yml
  - .github/workflows/dependabot-automerge.yml
  - .github/workflows/labeler.yml
  - .github/dependabot.yml
  - .github/codecov.yml
  - .github/labeler.yml
  - deny.toml
  - osv-scanner.toml
  - fuzz/osv-scanner.toml
  - rust-toolchain.toml
  - .cargo/config.toml
verified: 2026-11-15
---

# Spec 078: CI pipeline and supply-chain gates

Part of [[MOC-specs]]; see [[constitution]]. Release is in [[079-release-pipeline/spec]].

> [!info] Verification (2026-11-15)
> Implemented. `.github/workflows/ci.yml` (903 lines), `dependabot-automerge.yml`, `labeler.yml`, `dependabot.yml`, `codecov.yml`, `deny.toml`, `osv-scanner.toml`, `rust-toolchain.toml`, `.cargo/config.toml` read in full. Related history: #180/#383 (JS tests in CI), #327/#344 (wasm native tests), #318 (demo in workspace). Since then: #588 (least-privilege permissions), #615 (per-feature matrix), #608 (MSRV job), #616 (dependabot proposes majors) and #619 (bench A/B procedure) are done.

## Overview

**Problem.** A nightly-only, multi-crate, multi-language repository (Rust, wasm, TypeScript) needs fast, path-aware checks and supply-chain gates that merge safely.

**Goal.** Describe the CI jobs, triggers, required-status aggregator, coverage reporting, security scans and dependency automation.

**Out of scope.** Tag-triggered release ([[079-release-pipeline/spec]]); local pre-commit commands (`CLAUDE.md`).

## Functional Requirements

### FR-001 Triggers and concurrency
CI shall run on push and pull_request to `main`/`develop`, on `merge_group` for `main`, weekly on schedule (`39 17 * * 6`, Saturday), and on manual dispatch; runs in the same workflow/ref cancel in-progress ones. Workflow env: `RUSTFLAGS=-D warnings`, `CARGO_INCREMENTAL=0`, `CARGO_NET_RETRY=10`, `RUST_BACKTRACE=short`.
Status: done.

### FR-002 Change detection
Job `changes` (dorny/paths-filter) shall output `rust` (core, domain, wasm, demo, fuzz, root Cargo files, toolchain, `.cargo`, the workflow), `wasm` (wasm, domain, core, root Cargo files, toolchain, workflow) and `js` (`crates/pjs-js-client/**`, workflow). Downstream jobs run when their output is true, or on push, schedule or manual dispatch.
Status: done.

### FR-003 Rust quality gate
Job `quality` shall run `cargo fmt --all --check` (pinned toolchain) and `cargo clippy --workspace --all-features --all-targets -- -D warnings` (sccache + rust-cache).
Status: done.

### FR-004 Build and test matrix
Job `build` shall build a nextest archive (`--workspace --exclude pjs-wasm --features default[,mimalloc] --lib --bins`) on ubuntu, macos and windows for allocators `system` and `mimalloc`; job `test` (needs `build`, `quality`) shall run each archive in 4 partitions (`--partition count:N/4 --no-fail-fast`). Integration tests under `tests/` are not part of the archive.
Status: done. See Open Questions.

### FR-005 Doctests and docs
Job `doctest` shall run `cargo test --workspace --exclude pjs-wasm --doc` on three OSes; job `docs` shall run `cargo doc --no-deps --all-features -p pjson-rs -p pjson-rs-domain` with `--deny rustdoc::broken_intra_doc_links --deny rustdoc::private_intra_doc_links --deny rustdoc::redundant_explicit_links`.
Status: done.

### FR-006 WASM jobs
Jobs `wasm-native-tests` (`cargo nextest run -p pjs-wasm --tests`), `wasm-build` (matrix web/nodejs/bundler), `wasm-bundle-size`, `wasm-node-example`, `wasm-types-drift`, `wasm-package-validation` as specified in [[072-wasm-packaging-targets/spec]].
Status: done.

### FR-007 JS client job
Job `js-client-test` (Node 20, `npm ci`, downloads `wasm-build-web` into `crates/pjs-wasm/pkg`) shall run `npm run typecheck` and `npm test` in `crates/pjs-js-client`. It also runs for JS-only changes because `wasm-build` runs when `js` changed.
Status: done ([[022-ci-js-client-enforcement/spec]]; #180).

### FR-008 Fuzz jobs
Jobs `fuzz` and `fuzz-explore` as specified in [[076-fuzz-targets/spec]].
Status: done.

### FR-009 Coverage
Job `coverage` shall run `cargo llvm-cov nextest --all-features --all-targets --workspace --exclude pjs-demo` and upload `lcov.info` to Codecov with `fail_ci_if_error: false`. `.github/codecov.yml` sets project target 70% (threshold 2%) and patch target 75%, both `informational: true`, flags for core/domain/wasm and components (domain, security, application, infrastructure, parser, wasm). Coverage never fails a PR.
Status: done as reporting; there is no enforced gate.

### FR-010 Supply-chain scans
Job `security-osv` (pull_request, merge_group, schedule) shall run `google/osv-scanner-action` honouring `osv-scanner.toml` (three ignored advisories, each with a stated reason) and `fuzz/osv-scanner.toml`. Job `security-deny` shall run `cargo deny check` on the root and `check advisories bans sources` on `fuzz/Cargo.toml`. `deny.toml`: `yanked = "deny"`, allowed licenses MIT, Apache-2.0 (+LLVM-exception), BSD-2/3-Clause, ISC, Unicode-3.0, 0BSD, Zlib; `ring` license clarified; `multiple-versions = "warn"`; `unknown-registry`/`unknown-git = "warn"`; registry limited to crates.io.
Status: done.

### FR-015 Feature matrix and MSRV
Job `feature-matrix` (nightly, `cargo-hack`) shall run `cargo hack check -p pjson-rs -p pjson-rs-domain --each-feature --no-dev-deps --keep-going`, so every feature compiles on its own and one run reports every broken combination (#615). Job `msrv` shall run `cargo +1.89 check -p pjson-rs-domain -p pjs-wasm --locked` and `cargo +1.89 check -p pjs-wasm --target wasm32-unknown-unknown --locked`; `rust-version` (1.89) is set only on crates that build on stable (`pjson-rs-domain`, `pjs-wasm`), and `pjson-rs` carries none (#608).
Status: done.

### FR-011 Required-status aggregator
Job `ci-success` (`if: always()`) shall fail when any of quality, feature-matrix, msrv, fuzz, build, test, doctest, docs, wasm-native-tests, coverage, wasm-build, wasm-bundle-size, wasm-node-example, wasm-types-drift, wasm-package-validation, security-osv, security-deny, js-client-test finished `failure` or `cancelled`; skipped counts as pass. `fuzz-explore` is not part of it.
Status: done.

### FR-012 Toolchain and compile flags
`rust-toolchain.toml` shall pin a dated nightly channel (`nightly-YYYY-MM-DD`, not floating `nightly`) with `rustfmt`, `clippy` and targets `x86_64-unknown-linux-gnu`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`. `.cargo/config.toml` sets `-C target-cpu=native` for x86_64 and aarch64 hosts (CI `RUSTFLAGS` env overrides it).
Status: done.

### FR-013 Action pinning
All actions shall be pinned to full commit SHAs with a trailing version comment (user rule; observed in every `uses:`).
Status: done.

### FR-014 Dependency automation
`dependabot.yml` shall schedule weekly (Monday 09:00 GMT) updates for cargo (`/`), cargo (`/fuzz`) and github-actions, grouping minor and patch updates, also proposing semver-major updates (#616), commit prefix `chore` (cargo) / `ci` (actions), labels `dependencies` + ecosystem. `dependabot-automerge.yml` (pull_request_target, only when the PR author is `dependabot[bot]`) shall wait for all other checks (`wait-on-check-action`, allowed `success,skipped,neutral`) then `gh pr merge --auto --squash` only when every updated dependency is a patch update or a minor update from a version already >= 1.0.0 (a 0.x minor is breaking), and label other updates `major-update`. `labeler.yml` applies path labels from `.github/labeler.yml` (core, domain, benchmarks, application, infrastructure, rust, documentation, ci/cd, dependencies, security, performance, testing, wasm, pjs-demo, pjs-js-client).
Status: done.

## Non-Functional Requirements

- NFR-PERF-001: `quality` 20 min, `build` 45 min, `test` 30 min, `coverage` 30 min, `fuzz` 30 min timeouts.
- NFR-SEC-001: untrusted-PR safety: `pull_request_target` workflows do not check out PR code (`dependabot-automerge`, `labeler`); `fuzz` jobs use `persist-credentials: false`.
- NFR-SEC-002: workflow-level `permissions` are `contents: read`; each job that needs more (`pull-requests: write`, `security-events: write`, publish `id-token: write`, release `contents: write`) declares it itself (#588).
- NFR-REL-001: a path-skipped job must not block merge (`ci-success` logic).

## Data Model

Job graph: `changes` -> {`quality`, `feature-matrix`, `fuzz`, `build`, `doctest`, `docs`, `wasm-native-tests`, `coverage`, `wasm-build`}; `build`+`quality` -> `test`; `wasm-build` -> {`wasm-bundle-size`, `wasm-node-example`, `wasm-types-drift`, `wasm-package-validation`, `js-client-test`}; independent: `msrv`, `security-osv`, `security-deny`, `fuzz-explore`; all required -> `ci-success`.

## Edge Cases and Errors

| Case | Behaviour |
|------|-----------|
| Docs-only PR | most jobs skipped, `ci-success` passes |
| JS-only PR | `wasm-build`, `js-client-test`, `wasm-types-drift`, `wasm-node-example` run |
| Scheduled run | everything incl. `fuzz-explore`, `security-*` |
| Codecov upload failure | ignored |
| `wasm-bundle-size` over limit | warning only |

## Success Criteria

1. `ci-success` is green on `main`.
2. `cargo deny check` and OSV scan pass without new ignores.

## Agent Boundaries

- Always: pin new actions to SHAs; extend `ci-success` when adding a job; keep the `ci.yml` path filter in sync with workspace layout.
- Ask first: adding required jobs that run on every PR (cost), raising permissions.
- Never: use `pull_request_target` with PR code checkout; add `RUSTFLAGS` that differ from local checks.

## Open Questions

- Root `CLAUDE.md` says "Coverage threshold is enforced in CI"; `codecov.yml` is informational and uploads never fail.
- No benchmark job (see [[075-bench-suite/spec]]); the quiet-machine A/B procedure is documented in `crates/pjs-bench/README.md` (#619); `CLAUDE.md` and `.claude/rules` mention a `jemalloc` feature, but no `Cargo.toml` defines it (the matrix covers `system` and `mimalloc`).
- Dependabot has no npm ecosystem entry for `crates/pjs-js-client` (so its `package-lock.json` is not auto-updated by this file).
- `crates/pjs-js-client/.github/workflows/ci.yml` (Node 18/20/21 matrix) is a nested workflow GitHub never runs.
- Demo integration tests are not exercised in PR CI ([[077-demo-servers/spec]]); the comment near `coverage` ("zero tests") is stale.
- `wasm-pack test` (documented in `.claude/rules/branching.md`) is not run in CI.
- `wasm-pack@latest` and `cargo-fuzz` are installed unpinned.
- `ci.yml` mentions `main, develop`; there is no `develop` branch convention elsewhere in the repo docs. [NEEDS CLARIFICATION]

## See Also

[[plan]] | [[079-release-pipeline/spec]] | [[022-ci-js-client-enforcement/spec]] | `CONTRIBUTING.md` | `SECURITY.md`
