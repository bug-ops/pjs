---
aliases:
  - Testing Strategy
  - Test Pyramid
tags:
  - sdd
  - testing
  - ci
  - pjs
created: 2026-11-15
last-verified: 2026-11-15
verified-head: dcf9b11
status: living
audience: contributors and agents working on PJS
related:
  - "[[constitution]]"
  - "[[architecture-baseline]]"
  - "[[decisions/adr-001-allocator-decision|ADR-001 allocator decision]]"
  - "[[MOC-specs]]"
---

# Testing Strategy

> [!info] Verification (2026-11-15)
> Distilled from the untracked `.local/testing/` notes (`coverage-status.md`, `process-notes.md`, `regressions.md`, journals `ci-034` to `ci-036`) and **re-verified against the repository at HEAD dcf9b11**: `.github/workflows/ci.yml`, `release.yml`, `.config/nextest.toml`, `.github/codecov.yml`, `fuzz/`, `crates/*/tests`, `crates/pjs-bench`, `crates/pjs-js-client`. Test counts are taken from the ci-036 live run (nextest lib+bins 1173/1173, 2477 tests with integration targets, 1 skipped, doctests green); they were not re-run for this document and drift with every PR. The `.local/testing/` live-testing playbooks and scratch harnesses are deliberately not migrated (operational, not durable). Several statements in the `.local` notes were stale and are corrected here (see "Corrections").

This document describes the test pyramid as it exists, the CI gates that enforce it, and the guard tests protecting known regressions. Principles are in [[constitution]]; design patterns in [[architecture-baseline]].

## 1. Test pyramid

| Layer | Tool and location | Scope | Runs in CI |
|---|---|---|---|
| Unit | inline `#[cfg(test)]`/`#[test]`/`#[tokio::test]` in `crates/*/src` (about 1100 test fns in `pjs-core` and `pjs-domain`); `cargo nextest` | modules, aggregates, parsers, codecs, handlers with mock ports | yes: `test` job, `--lib --bins`, 3 OS x 2 allocators x 4 partitions |
| Integration | `crates/pjs-core/tests/*.rs` (26 files), `crates/pjs-domain/tests/*.rs` (8), `crates/pjs-wasm/tests/*.rs` (2), `crates/pjs-demo/tests/*.rs` (2); shared helpers in `pjs-core/tests/common/mod.rs` | cross-module flows: HTTP router + auth + rate limit, WebSocket wire level, GAT repositories, compression pipeline, schema validation, bounded iteration | partially: see "Gate gaps" |
| Property | `proptest` in `crates/pjs-domain/tests/property_tests.rs` and `src/value_objects/json_path.rs` | domain invariants (priority, ids, paths, frames) | yes (part of domain tests) |
| Golden / wire format | `pjs-domain/tests/serde_golden.rs`, `pjs-core/tests/serde_wire_format.rs` | byte-identical re-encoding of `Priority` (bare `u8`), ids (UUID strings), `StreamSession::streams` map keys | yes |
| Doctests | `cargo test --workspace --exclude pjs-wasm --doc` | every `# Examples` block on public API | yes: `doctest` job, 3 OS |
| Fuzz | `fuzz/` (separate workspace, `cargo-fuzz`): `parser_zero_copy`, `parser_partial`, `secure_decompress`, `streaming_decompressor`, `ws_message_decode`; committed seeds in `fuzz/seeds/<target>/` | untrusted-input boundaries | build + seed replay (`-runs=0`) gate; exploration is non-gating |
| Benchmarks | `crates/pjs-bench` (`criterion`): `simple_throughput`, `zero_copy_bench`, `serde_overhead_bench`, `gat_query_benchmarks`, `http_streaming` | perf-critical paths | **no** (manual / continuous-improvement cycle) |
| WASM native | `crates/pjs-wasm/tests/priority_parity.rs`, `streaming_comprehensive.rs` (plain `#[test]`) | WASM crate logic on the host | yes: `wasm-native-tests` job |
| WASM in-runtime | 28 `#[wasm_bindgen_test]` in `pjs-wasm/src/parser.rs` (20) and `streaming.rs` (8) | browser/Node runtime behaviour | **no** (run locally: `wasm-pack test --node` / `--headless --chrome`) |
| WASM artefact | `wasm-build` (web, nodejs, bundler), `wasm-bundle-size`, `wasm-node-example`, `wasm-types-drift`, `wasm-package-validation` | published package shape | yes |
| JS/TS | Jest + ts-jest in `crates/pjs-js-client/tests/{core,parser,integration,utils}`; `npm run typecheck` | client library, WASM backend | yes: `js-client-test` job |
| Live / exploratory | continuous-improvement cycles (HTTP demo servers, WebSocket probes, SecurityConfig probes) | end-to-end, cross-interface | manual, outside CI |

### Conventions

- New public API: at least one unit test plus a runnable doctest. New port/adapter: an integration test wiring the real adapter. Hot-path change: criterion before/after numbers in the PR.
- Time-dependent aggregate logic uses the injectable `TimeProvider` (`StreamSession::with_time_provider`) rather than sleeping.
- Concurrency/atomicity bugs get a deterministic test (for example the `PendingThenReady` adversarial stream harness in `infrastructure/http/streaming.rs` for waker contracts; pause hooks in `infrastructure/adapters/event_publisher.rs`).
- Integration tests that need a feature are gated with `#![cfg(feature = "...")]` or `required-features` (`websocket_security`, `websocket_wire_level` in `pjs-core/Cargo.toml`).
- Live testing is read-only against source: findings become GitHub issues, not in-tree changes. Security findings follow `SECURITY.md` (private advisories), not public issues.

## 2. CI gates (`.github/workflows/ci.yml`)

Triggers: push and pull request on `main`/`develop`, `merge_group`, weekly schedule (Saturday 17:39 UTC), `workflow_dispatch`. A `changes` job (path filter) skips Rust/WASM/JS jobs for unrelated changes. `ci-success` is the single branch-protection gate; it fails on `failure`/`cancelled` of any needed job (skipped counts as pass).

| Tier | Job | What it enforces |
|---|---|---|
| 1 | `quality` | `cargo +nightly fmt --all --check`; `cargo clippy --workspace --all-features --all-targets -- -D warnings` |
| 1 | `fuzz` | fmt and clippy of `fuzz/`; `cargo fuzz build`; replay every committed seed with `-runs=0` |
| 2a | `build` | per OS (ubuntu, macos, windows) x allocator (`system`, `mimalloc`): `cargo nextest archive --workspace --exclude pjs-wasm --features "default[,mimalloc]" --lib --bins` ([[decisions/adr-001-allocator-decision]]) |
| 2b | `test` | runs the archive in 4 partitions per leg (24 jobs), `--no-fail-fast` |
| 2 | `doctest` | `cargo test --workspace --exclude pjs-wasm --doc`, 3 OS |
| 2 | `docs` | `cargo doc --no-deps --all-features -p pjson-rs -p pjson-rs-domain` with `--deny rustdoc::broken_intra_doc_links --deny rustdoc::private_intra_doc_links` |
| 2 | `wasm-native-tests` | `cargo nextest run -p pjs-wasm --tests --no-fail-fast` |
| 2 | `coverage` | `cargo llvm-cov nextest --all-features --all-targets --workspace --exclude pjs-demo`, uploads `lcov.info` to Codecov (`fail_ci_if_error: false`); the test run itself must pass |
| 3 | `wasm-build` | `wasm-pack build --release` for `web`, `nodejs`, `bundler` |
| 4 | `wasm-bundle-size` | reports raw/gzipped `.wasm` size per target and marks WARNING above 200 KB raw or 80 KB gzipped (report and PR comment only; never fails the job) |
| 4 | `wasm-node-example` | runs `crates/pjs-js-client/examples/browser-wasm/example.js` against the built package, grep-checks output |
| 4 | `wasm-types-drift` | `npm run typecheck:wasm-drift` type-checks the JS client against the real generated `pjs-wasm` typings |
| 4 | `wasm-package-validation` | required `package.json` fields; required classes in the generated `.d.ts` |
| 4 | `js-client-test` | `npm ci`, `npm run typecheck`, `npm test`; depends on the `wasm-build` artefact |
| sec | `security-deny` | `cargo deny check` (root) and `check advisories bans sources` for `fuzz/` (PR, merge queue, schedule only) |
| sec | `security-osv` | OSV scanner (PR, merge queue, schedule only) |
| - | `fuzz-explore` | 60 s per target, schedule/manual only, **not** in `ci-success` |

`.config/nextest.toml`: `default` profile has `default-filter = 'not test(/^integration_/)'`; `ci` profile `all()` with JUnit output. No test function is named `integration_*` today, so the default filter excludes nothing, and CI does not select `--profile ci`. `release.yml` runs the full suite before publishing: fmt, clippy `-D warnings`, `cargo nextest run --workspace --all-features --all-targets --no-fail-fast`, `cargo test --workspace --doc --all-features`.

### Local pre-commit checklist (from `CLAUDE.md` and `.claude/rules/branching.md`)

```bash
cargo +nightly fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --workspace --all-features --lib --bins
cargo test --workspace --doc --all-features
RUSTDOCFLAGS="--deny rustdoc::broken_intra_doc_links" cargo doc --no-deps -p pjson-rs -p pjson-rs-domain
# if crates/pjs-wasm changed:
wasm-pack build crates/pjs-wasm --target web
wasm-pack test crates/pjs-wasm --headless --chrome
```

Allocator variants are tested separately (`--features mimalloc`); `jemalloc` no longer exists. `pjs-demo` is built with `--manifest-path crates/pjs-demo/Cargo.toml` and the `pjson-rs/`-prefixed feature names.

## 3. Coverage

- Collected only by the `coverage` job (`cargo llvm-cov nextest`, all features, all targets, `pjs-demo` excluded).
- `.github/codecov.yml`: project target 70% with 2% threshold, patch target 75%; both **informational** (never block a PR). Ignored: `tests/`, `benches/`, `examples/`, `crates/pjs-demo/`, `crates/pjs-bench/`, `**/tests.rs`, `**/test_*.rs`. Per-crate flags (`pjs-core`, `pjs-domain`, `pjs-wasm`) and per-layer components (domain, security, application, infrastructure, parser, wasm). The comments in that file ("Security modules - 100% required", "Domain - 80% minimum") are aspirational: no job enforces them.
- `CLAUDE.md` says "Coverage threshold is enforced in CI"; in practice no coverage threshold fails a build. [NEEDS CLARIFICATION: current workspace line coverage; the last recorded figure in the notes (about 72%) dates from 2026-05 and is stale.]
- `pjs-js-client` Jest config defines an 80% global threshold (branches, functions, lines, statements) that applies only when run with `--coverage`; CI runs `npm test` without it.

## 4. Gate gaps (verified)

1. Integration tests (`crates/*/tests/`) are **not** run by the `test` job (`--lib --bins`). They run in the `coverage` job (`--all-targets`, ubuntu, all features, system allocator), in `wasm-native-tests` (for `pjs-wasm`) and in `release.yml`. Windows and macOS therefore only execute unit tests and doctests.
2. `wasm_bindgen_test` tests (28) are not executed anywhere in CI; the `wasm-build` job only builds.
3. No per-feature build matrix: `websocket-client` alone does not compile (open #598) and `pjson-rs-domain` `no_std` fails (open #606); proposed gate is open #615 (`cargo hack --each-feature --keep-going`).
4. Benchmarks are never run or compared in CI; the A/B of 914fc72 vs dcf9b11 was contaminated by machine load and needs a quiet-machine rerun with documented `--bench <name>` flags (open #619).
5. `cargo audit` is not run in CI (deny advisories and OSV cover advisories); `security-*` jobs do not run on plain pushes.
6. Fuzz targets do not cover `ByteCodec::ZstdDict` (TODO in `fuzz/fuzz_targets/secure_decompress.rs`).
7. Windows leg compiles the `mimalloc` variant like the others, but the CI comment says Windows skips it (stale comment, no exclusion in the matrix).

## 5. Regression guard tests

Tests that pin a previously fixed defect or a cross-crate invariant. Names are real symbols or files; each should fail loudly if the invariant is reintroduced.

| Guard | Location | Protects |
|---|---|---|
| Cross-engine priority parity | `crates/pjs-wasm/tests/priority_parity.rs` | WASM `PriorityAssigner` and `Stream::extract_patches` both delegate to `pjson_rs_domain::services::compute_priority` (#242) |
| Depth-limit drift | `pjs-core/src/config/security.rs` (`test_max_deserialize_depth_matches_domain_guard_defaults`, `test_jiter_config_default_max_depth_matches_domain_guard`); `pjs-wasm/src/security.rs` (`test_default_max_depth_matches_domain_guard`) | `MAX_DESERIALIZE_DEPTH` (64) stays consistent across `pjs-domain`, `pjs-core` jiter config and `pjs-wasm` (#473/#474) |
| Wire-format golden tests | `pjs-domain/tests/serde_golden.rs`, `pjs-core/tests/serde_wire_format.rs` | serialization of `Priority`, `SessionId`/`StreamId`, `DomainEvent`, `Frame`, `Stream`, `StreamSession::streams` |
| Event drain before persist | `application/handlers/command_handlers.rs` tests (regression test for #466) | `save_and_publish` drains events before cloning the session |
| Waker contract | `infrastructure/http/streaming.rs` tests using `PendingThenReady` | stream bodies are correct under pending-then-ready polling (#166/#168) |
| Round-trip and array duplication | `stream/priority.rs` tests ("Regression tests for object-nested array duplication", #394; `test_round_trip_mixed_payload_no_regression`) | analyze -> reconstruct is lossless, no array duplication |
| WebSocket session binding | `crates/pjs-demo/tests/ws_upgrade.rs` (#382), `ws_pause_resume.rs`; `pjs-core/tests/websocket_security.rs`, `websocket_wire_level.rs` (#158) | session claim/ownership, origin allow-list, real protocol upgrade |
| HTTP connection limits | `pjs-core/tests/http_serve_connection_limits.rs` | `serve_with_limits` timeouts and per-IP caps (#523/#527) |
| Bounded iteration | `pjs-core/tests/security_bounded_iteration_integration.rs` | pagination/scan limits propagate (`MAX_SCAN_LIMIT`, `MAX_RESULTS_LIMIT`) |
| Aligned allocator | `pjs-core/tests/parser_allocator_comprehensive.rs` | alignment, realloc integrity, error paths, `global_allocator_name` |
| WASM typings drift | CI `wasm-types-drift` | JS client types stay compatible with generated `pjs-wasm` `.d.ts` |
| WASM package presence | `pjs-js-client/tests/integration/wasm-pkg.ts` (`describeWasmPkg`) | under `CI=true` a missing `pjs-wasm` package fails instead of silently skipping (blind spot behind #383) |
| Dictionary store | `infrastructure/repositories/dictionary_store.rs` tests (guard release, concurrent training) | bomb-detector gates on register and post-training, single training under concurrency |
| Domain golden/property | `pjs-domain/tests/property_tests.rs`, `json_data_comprehensive.rs` (NaN/Infinity rejection), `frame_comprehensive.rs` | value-object invariants |

`.local/testing/regressions.md` is currently only a list of scratch replay scenarios for the ci-036 WebSocket/config live probes (not tracked, not durable). Durable guards are the tests above.

## 6. Performance testing

`crates/pjs-bench` benches are criterion binaries (`harness = false`); run with `cargo bench -p pjs-bench --bench <name>` (criterion flags need the explicit `--bench` target). Expected process (from the continuous-improvement rules, not CI-enforced): compare against the previous cycle, treat a regression above 10% as P1. Numbers are only trustworthy on an idle machine (open #619). `scripts/profile.sh` offers cpu/heap/bench profiling. The `pjs-bench/README.md` still carries stale result tables and emoji for removed benchmarks. [NEEDS CLARIFICATION: current benchmark baseline; none is committed.]

## 7. Corrections to the source notes

- `coverage-status.md` rows still describe `parser/hybrid.rs`, `parser/sonic.rs`, `parser/simple.rs`, `memory/`, `infrastructure/integration/` as live or "Reviewed"; they are deleted (`Removed` rows exist for some). `Parser::parse_partial` no longer exists (the `Parser` type was removed, #486/#488/#495).
- `process-notes.md` states `pjs-demo` is in the workspace members and that `integration_` filtering is by test name: both true today, but the filter matches no tests. `CLAUDE.md` still says `pjs-demo` is excluded from the workspace; it is a member (not a default member). Only `crates/pjs-js-client` and `fuzz/` are excluded.
- The "JS: npm test CI job (#180)" claim stands (`js-client-test`); the Chrome-dependent `wasm-pack test` run is local only.
- Old coverage figure (about 72%, round 13) is obsolete.

## 8. Open items

| Item | Issue |
|---|---|
| Per-feature CI matrix | #615 |
| `websocket-client` alone does not compile | #598 |
| `pjson-rs-domain` `no_std` build fails | #606 |
| Benchmark A/B procedure and quiet-machine rerun | #619 |
| WebSocket shared broadcast can drop frames; transport never prioritizes | #570, #600 |
| `SecurityConfig` limits unenforced and zero accepted | #613, #614 |
| Run `wasm_bindgen_test` suite in CI | [NEEDS CLARIFICATION: no issue found] |
| Run integration tests on all OSes (switch `test` job to include `--tests`, or add a job) | [NEEDS CLARIFICATION: no issue found] |

## See also

- [[constitution]], [[architecture-baseline]] section 15
- `CONTRIBUTING.md` (Testing Requirements), `SECURITY.md`, `docs/architecture/WASM_ROADMAP.md`
- `.github/workflows/ci.yml`, `.github/workflows/release.yml`, `.github/codecov.yml`, `.config/nextest.toml`
