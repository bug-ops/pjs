---
aliases:
  - WASM Loader ESM/CJS Mismatch
tags:
  - sdd
  - spec
  - wasm
  - js-client
  - ci
created: 2026-08-17
status: implemented
priority: P1
issue: "#383"
pr: "#389"
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
  - "[[027-wasm-backend-node-init/spec|027 WasmBackend Node.js init]]"
  - "[[022-ci-js-client-enforcement/spec|022 CI js-client enforcement]]"
code:
  - crates/pjs-js-client/src/utils/wasm-loader.ts
  - crates/pjs-js-client/src/transport/wasm-backend.ts
  - crates/pjs-js-client/tests/integration/wasm-pkg.ts
  - crates/pjs-js-client/tests/integration/wasm-backend.test.ts
  - crates/pjs-js-client/tests/integration/wasm-parser.test.ts
  - .github/workflows/ci.yml
verified: 2026-11-15
---

# Feature: Fix WasmBackend/WasmParser Node.js Initialization (ESM/CJS mismatch after #351)

> [!info] Verification (2026-11-15)
> **State: implemented**, with option 3 (loader-side fix) plus option 4 (CI coverage).
>
> **Evidence**
> - PR #389 (commit `c46c027`, "resolve ESM/CJS mismatch that still broke Node.js WASM init after #351") closed issue #383. CHANGELOG entry cites #383.
> - `loadWasmModule()` in `crates/pjs-js-client/src/utils/wasm-loader.ts`: on Node it resolves `pjs-wasm/package.json`, reads the glue file named by `main`, rewrites `export` declarations and the trailing `export { ... }` list into CommonJS via `evaluateEsmGlueAsCjs` (`new Function` with a `'use strict'` preamble and a `TextEncoder`/`TextDecoder` shim from `util`), reads `*_bg.wasm`, and calls `init({ module_or_path: buffer })`. The in-flight promise is cached in `cachedNodeModulePromise`; failures reset the cache. The browser path is unchanged: `await import('pjs-wasm')` then `default()`.
> - CI (`.github/workflows/ci.yml`): `wasm-build` runs on `changes.wasm`, `changes.js`, push, schedule and dispatch with `needs: [changes]`; `js-client-test` has `needs: [changes, wasm-build]` and downloads artifact `wasm-build-web` into `crates/pjs-wasm/pkg` before `npm ci`, typecheck and `npm test`.
> - `tests/integration/wasm-pkg.ts`: when `pjs-wasm/pkg` is missing the suites `describe.skip` locally but fail loudly when `process.env.CI` is set.
>
> **Drift from original spec**
> - `jest.config.js` is unchanged (`ts-jest/presets/default-esm`, `jsdom`); the dual-target/`bundler` options were not adopted (`wasm-build` does build `web`, `nodejs`, `bundler` for other jobs, but the loader uses the `web` glue).
> - Pure-ESM Node without a CommonJS `require` is still unsupported: the loader throws an explicit error (FR-001 holds for CJS/Jest/`require`-shim contexts only).
> - One test stays skipped: `wasm-parser.test.ts` `test.skip('should stream JSON with priority-based progressive delivery')`, because `onComplete` calls `stream.free()` while `start()` is active (suspected wasm-bindgen reentrant borrow). [NEEDS CLARIFICATION: no open issue found for this follow-up.]
> - The `evaluateEsmGlueAsCjs` approach is a regex rewrite that supports only the two export forms wasm-bindgen emits; a wasm-bindgen output change would throw rather than degrade silently.
> - The original issue count (22 failing tests) is historical; not re-run in this verification.

## 1. Overview

### Problem Statement (historical)

PR #351 fixed `fetch()` of a `file:` URL (#317) but `loadWasmModule()` still used `import('pjs-wasm')`. `pjs-wasm/pkg` from `wasm-pack --target web` is `"type": "module"`; under Jest (no `--experimental-vm-modules`) the import threw `SyntaxError: Unexpected token 'export'`, so `WasmBackend.connect()` and `WasmParser` initialization failed in Node. CI never saw it because `js-client-test` did not fetch the `wasm-build-web` artifact and the integration suites silently `describe.skip`ped.

### Goal

`WasmBackend.connect()` and `WasmParser` initialization succeed in Node.js under Jest/ts-jest and plain Node, verified live in CI.

### Out of Scope

- `pjs-wasm` Rust source or its wasm-bindgen API.
- Browser behaviour.
- Deno, Bun, edge runtimes.

## 2. User Stories

- US-001: a Node consumer calls `WasmBackend.connect()` and `isConnected` becomes true.
- US-002: a Node consumer uses `WasmParser` streaming without an init error.
- US-003: a maintainer sees the WASM integration suites executed (not skipped) in CI.

## 3. Functional Requirements

| ID | Requirement | Priority | Status |
|----|------------|----------|--------|
| FR-001 | WHEN `loadWasmModule()` runs on Node with `pjs-wasm/pkg` installed THE SYSTEM SHALL load and initialize without an ESM/CJS error | must | done (CJS-capable contexts; explicit error in pure ESM without `require`) |
| FR-002 | WHEN it runs under Jest ts-jest ESM preset with `jsdom` THE SYSTEM SHALL load and initialize | must | done |
| FR-003 | WHEN `isNodeRuntime()` is false THE SYSTEM SHALL keep the fetch-based path | must | done |
| FR-004 | WHEN the fix changes the build THE SYSTEM SHALL NOT require callers to change `connect()`/`WasmParser` usage | must | done (no build change, no API change) |
| FR-005 | WHEN `js-client-test` runs THE SYSTEM SHALL have `crates/pjs-wasm/pkg` present before `npm test` | must | done |
| FR-006 | WHEN `pjs-wasm/pkg` is present THE SYSTEM SHALL execute and pass `wasm-backend.test.ts` and `wasm-parser.test.ts` | must | partial: executed in CI; one test `test.skip` (see drift) |
| FR-007 | WHEN init fails for unrelated reasons THE SYSTEM SHALL surface a distinguishable error | should | done (`Failed to load the pjs-wasm binary for Node.js: ...` with `cause`) |

## 4. Non-Functional Requirements

| ID | Category | Requirement | Status |
|----|----------|-------------|--------|
| NFR-001 | Compatibility | Browser behaviour unchanged | done |
| NFR-002 | Compatibility | `pjs-wasm` stays optional (`file:../pjs-wasm/pkg`) | done |
| NFR-003 | Testability | Integration suites run unskipped locally after `wasm-pack build` and in CI | done |
| NFR-004 | CI coverage | No silent skip in CI | done (`process.env.CI` guard in `wasm-pkg.ts`) |
| NFR-005 | Docs | Document any build-target change | n/a (no change) |

## 5. Data Model

No domain entities. Module state: `cachedWasmBuffer: Buffer | null`, `cachedNodeModulePromise: Promise<typeof import('pjs-wasm')> | null`.

## 6. Edge Cases and Error Handling

| Scenario | Behaviour |
|----------|-----------|
| `typeof require !== 'function'` on Node | throws "Cannot load pjs-wasm in this Node.js environment: no CommonJS `require` ..." |
| Glue file has no trailing `export { ... }` | throws "unexpected shape" error |
| `package.json` has no string `main` | throws with the `main` value |
| Concurrent `loadWasmModule()` calls | share one in-flight promise and one wasm instance |
| Load failure | cache reset so a later call can retry |
| Package not installed | wrapped `Failed to load ...` error |

## 7. Success Criteria

| ID | Metric | Target | Status |
|----|--------|--------|--------|
| SC-001 | `wasm-backend.test.ts` with pkg present | 100% | done in CI (not re-run here) |
| SC-002 | `wasm-parser.test.ts` | 100% of non-skipped | done in CI (not re-run here) |
| SC-004 | integration suites executed in CI | true | done |
| SC-005 | no browser-path regression | 0 failures | done in CI |

## 8. Agent Boundaries

**Always**: run `npm test`, `npm run typecheck`, `npm run lint` after loader changes; build `pjs-wasm/pkg` first.
**Ask first**: changing the wasm-pack target strategy; `js-client-test` job dependencies; the `pjs-wasm` dependency declaration.
**Never**: change browser-path initialization; add a required dependency; remove the CI hard-fail guard.

## 9. Open Questions

- Follow-up for the skipped `WasmParser.stream()` `onComplete` test [NEEDS CLARIFICATION: no tracking issue].
- Replace regex-based glue rewriting with a dual-target build (`nodejs` artifact is already built in `wasm-build`) [NEEDS CLARIFICATION: maintainer decision].
- Spec 027 example `examples/browser-wasm/example.js` was never confirmed as affected [NEEDS CLARIFICATION].

## 10. See Also

- [[constitution]], [[MOC-specs]]
- [[027-wasm-backend-node-init/spec|027]], [[022-ci-js-client-enforcement/spec|022]]
- `docs/architecture/WASM_ROADMAP.md`
- Issues #317, #383; PRs #351, #389
