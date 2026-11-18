---
aliases:
  - WASM packaging
  - pjs-wasm npm package and targets
tags:
  - sdd
  - spec
  - wasm
  - npm
  - build
created: 2026-11-15
status: implemented
priority: P2
related:
  - "[[070-wasm-bindings-api/spec]]"
  - "[[074-js-client-wasm-integration/spec]]"
  - "[[078-ci-pipeline/spec]]"
  - "[[079-release-pipeline/spec]]"
  - "[[010-wasm-typescript-type-generation/spec]]"
issue: "#344"
code:
  - crates/pjs-wasm/Cargo.toml
  - crates/pjs-wasm/README.md
  - crates/pjs-wasm/demo
  - .github/workflows/ci.yml
  - .github/workflows/release.yml
verified: 2026-11-15
---

# Spec 072: pjs-wasm build targets and npm package

Part of [[MOC-specs]]; see [[constitution]].

> [!info] Verification (2026-11-15)
> Implemented. Evidence: `crates/pjs-wasm/Cargo.toml`, CI jobs `wasm-build`, `wasm-bundle-size`, `wasm-package-validation`, `wasm-node-example`, `wasm-types-drift`; release job `publish-wasm`. Related fixes: #344 (Windows argv limit), #327 (native tests in PR CI), #531 (tsify attribute removal).

## Overview

**Problem.** The Rust crate must be distributed as an npm package usable from browsers, bundlers and Node.js, with TypeScript types, at a small size.

**Goal.** Describe how the crate is built per target, what the package contains, which checks guard it, and the naming under which it is published.

**Out of scope.** The public API ([[070-wasm-bindings-api/spec]]); the TypeScript client ([[074-js-client-wasm-integration/spec]]); roadmap narrative (`docs/architecture/WASM_ROADMAP.md`).

## Functional Requirements

### FR-001 Crate type and features
The crate shall build as `cdylib` and `rlib`, with default feature `console_error_panic_hook` (optional dependency). No other features exist.
Code: `crates/pjs-wasm/Cargo.toml`.
Status: done.

### FR-002 Build targets
The system shall build with `wasm-pack build` for three targets, `web`, `nodejs` and `bundler`. CI builds all three in release mode into `pkg-<target>/` and uploads each as artifact `wasm-build-<target>` (7-day retention). The release pipeline builds only `--target web --release --scope pjson`.
Code: `ci.yml` job `wasm-build` (matrix), `release.yml` job `publish-wasm`.
Status: done.

### FR-003 Size optimisation
Release builds shall run `wasm-opt -Os` (`[package.metadata.wasm-pack.profile.release]`). The workspace `[profile.release]` is `opt-level = 3`, `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`.
Code: `crates/pjs-wasm/Cargo.toml`, root `Cargo.toml`.
Status: done. The README states `opt-level = "s"`, which is not what the manifest sets.

### FR-004 Bundle size report
When the `wasm` paths changed (or on push, schedule, manual run), CI shall compute raw and gzipped size of `pjs_wasm_bg.wasm` for each target, publish `bundle-report.md` as an artifact (30 days), and mark a target `WARNING` above 200 KB raw or 80 KB gzipped. On pull requests it posts the report as a comment (`continue-on-error`). The status never fails the job.
Code: `ci.yml` job `wasm-bundle-size`.
Status: done. The 70 KB gzipped figure in the crate docs (`lib.rs`) is a claim, not asserted by CI.

### FR-005 Package validation
CI shall check the `web` package `package.json` has `name`, `version`, `files`, `main`, `types`, and that `pjs_wasm.d.ts` contains `class PjsParser`, `class PriorityConstants`, `class PriorityConfigBuilder` and `export function version`.
Code: `ci.yml` job `wasm-package-validation`.
Status: done. `PriorityStream`, `SecurityConfig` and the callback types are not checked there (they are covered indirectly by FR-006).

### FR-006 Type drift guard
CI shall type-check `crates/pjs-js-client` against the real generated bindings with `npm run typecheck:wasm-drift` (`tsc --noEmit -p tsconfig.wasm-drift.json`, which excludes the hand-written fallback `src/types/pjs-wasm.d.ts`).
Code: `ci.yml` job `wasm-types-drift`; `crates/pjs-js-client/tsconfig.wasm-drift.json`.
Status: done.

### FR-007 Node example smoke test
CI shall run `crates/pjs-js-client/examples/browser-wasm/example.js` under Node 24 against the `web` artifact and require output lines `Example 1: Basic Usage` and `Generated`.
Code: `ci.yml` job `wasm-node-example`.
Status: done.

### FR-008 Published names
The release pipeline shall rename the wasm-pack package to `@pjson/wasm` (`jq` over `pkg/package.json`) and publish it with provenance; `@pjson/node` is the published name of `crates/pjs-js-client` (its in-repo name is `@pjs/client`). The in-repo `optionalDependencies` entry is `"pjs-wasm": "file:../pjs-wasm/pkg"`, rewritten at release to `npm:@pjson/wasm@^<version>`. See [[079-release-pipeline/spec]].
Code: `release.yml` jobs `publish-wasm`, `publish-node`.
Status: done. Names disagree across repo docs (see Open Questions).

### FR-009 Browser demo
The repository shall ship a static demo (`crates/pjs-wasm/demo/{index.html,app.js,styles.css}`) and a browser example (`crates/pjs-js-client/examples/browser-wasm/`, with `serve.sh`, `example.js`, `index.html`).
Status: done; not exercised by CI except `example.js` (FR-007).

## Non-Functional Requirements

- NFR-SIZE-001: wasm raw size <= 200 KB and gzipped <= 80 KB (soft, warning only).
- NFR-PORT-001: native tests of the crate must run on Linux/macOS/Windows hosts; the crate is excluded from the main nextest archive and doctests, and run separately in `wasm-native-tests` (Ubuntu only).

## Data Model

Artifacts: `crates/pjs-wasm/pkg*/{pjs_wasm.js, pjs_wasm_bg.wasm, pjs_wasm.d.ts, package.json}` (wasm-pack output). Target names are strings in the CI matrix `[web, nodejs, bundler]`.

## Edge Cases and Errors

| Case | Behaviour |
|------|-----------|
| Size over threshold | `WARNING` row, job passes |
| `d.ts` missing a class | `wasm-package-validation` fails |
| `@pjson/wasm@<version>` already on npm | publish step skipped |
| Windows host build of `pjs-wasm` in nextest archive | avoided by `--exclude pjs-wasm` (#344) |

## Success Criteria

1. All three `wasm-build` matrix legs succeed.
2. `wasm-types-drift`, `wasm-package-validation`, `wasm-node-example`, `js-client-test` pass.

## Agent Boundaries

- Always: update `src/types/pjs-wasm.d.ts` in the JS client and the validation grep list when the JS API changes.
- Ask first: renaming published packages or changing the wasm-pack targets.
- Never: publish from a non-tag run; skip `--provenance`.

## Open Questions

- Package name is inconsistent across docs: `CLAUDE.md` and the brief say `pjs-wasm`; npm publishes `@pjson/wasm`; `SECURITY.md` and README use `@pjson/wasm`; `docs/architecture/WASM_ROADMAP.md` uses `@pjs/wasm`; the client's dependency key is `pjs-wasm`.
- README says `opt-level = "s"`; actual is `wasm-opt -Os` over an `opt-level = 3` build.
- `wasm-pack test --headless --chrome` is a documented pre-PR step (`.claude/rules/branching.md`) but no CI job runs it.
- `wasm-pack` is pinned through `WASM_PACK_VERSION` in `ci.yml` and `release.yml` (#744); refresh it with the Actions bump.
- `pjs-wasm` has no `engines`/`exports` map control (wasm-pack generated).

## See Also

[[plan]] | [[078-ci-pipeline/spec]] | [[079-release-pipeline/spec]] | `crates/pjs-wasm/README.md`
