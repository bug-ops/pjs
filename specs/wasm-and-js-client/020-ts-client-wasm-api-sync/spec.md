---
aliases:
  - TS Client WASM API Sync
  - JS client API drift gate
tags:
  - sdd
  - spec
  - js-client
  - wasm
  - ci
created: 2026-04-28
status: implemented
priority: P1
issue: "#178"
pr: "#192"
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
  - "[[022-ci-js-client-enforcement/spec|Spec 022]]"
  - "[[027-wasm-backend-node-init/spec|Spec 027]]"
code:
  - crates/pjs-js-client/package.json
  - crates/pjs-js-client/tsconfig.json
  - crates/pjs-js-client/tsconfig.wasm-drift.json
  - crates/pjs-js-client/src/types/pjs-wasm.d.ts
  - crates/pjs-js-client/src/core/json-reconstructor.ts
  - crates/pjs-js-client/src/core/frame-processor.ts
  - .github/workflows/ci.yml
verified: 2026-11-15
---

# Spec 020: TypeScript Client - WASM/Rust API Sync Strategy

> [!info] Verification (2026-11-15)
> **State: implemented** (Tier 1 and the practical part of Tier 2; Tier 3 deliberately deferred). Issue #178 CLOSED 2026-04-28; #143 (tsify) CLOSED.
>
> Evidence:
> - The 71 failing tests were fixed by aligning the implementation to the tests: PR #192 (`dce0e2b`) added `JsonReconstructor.processSkeleton`, `applyPatch(frame)`, `FrameProcessor.validateFrame`. Both still exist in `src/core/`.
> - Tier 1: `tsconfig.json` has `"strict": true`; `npm run typecheck` (`tsc --noEmit`) exists and runs in CI job `js-client-test` (added by PR #351, issue #339).
> - Tier 2: `crates/pjs-wasm` uses `tsify` (`Cargo.toml` `tsify = "0.5"`, PR #199). `tsconfig.wasm-drift.json` excludes the ambient shim `src/types/pjs-wasm.d.ts`; `npm run typecheck:wasm-drift` runs in CI job `wasm-types-drift` against the real built `pjs-wasm` pkg.
>
> Drift from original spec:
> - Gate is a separate `typecheck` CI step, not `"pretest": "tsc --noEmit"` (package.json has no `pretest`).
> - WASM isolation uses an ambient `declare module 'pjs-wasm'` shim, not a Jest `__mocks__/pjs-wasm`; CI also builds and downloads the real `wasm-build-web` artifact for `js-client-test`.
> - Crate is `tsify` 0.5, not `tsify-next`.
> - `package.json` `engines.node` is `>=18.0.0` as the original spec stated.

## Problem

The TypeScript client was authored independently of the Rust/WASM source; tests called methods that did not exist (`processSkeleton`, `validateFrame`, `applyPatch(frame)`), 71 of 113 tests failed, and nothing in CI detected it.

## Functional Requirements

### FR-1 Divergence between implementation and tests is detected on every PR
Status: done. `tsc --noEmit` (`typecheck`) plus `jest` run in `js-client-test`.

### FR-2 No separately maintained API contract document
Status: done. Source of truth is TypeScript types plus generated `pjs_wasm.d.ts`.

### FR-3 Strict compile is the first gate
Status: done. `strict: true`; `npm run typecheck` precedes `npm test` in CI.

### FR-4 Tests run as the second gate
Status: done (`npm test` = `jest`).

### FR-5 WASM optional dependency does not block compile/test without a built pkg
Status: done for typecheck (ambient shim `src/types/pjs-wasm.d.ts`). WASM integration suites skip locally if `crates/pjs-wasm/pkg` is absent and fail loudly under `CI` (`tests/integration/wasm-pkg.ts`, `describeWasmPkg`).

## Strategy tiers (as built)

| Tier | Plan | State |
|------|------|-------|
| 1 | strict `tsc --noEmit` gate | done |
| 2 | wasm-bindgen/tsify generated `.d.ts` as authority | done via `typecheck:wasm-drift` |
| 3 | Pact/OpenAPI contract testing for HTTP | not done; defer until after v1.0.0 |

## Acceptance Criteria

- [x] Missing-method calls fail at compile time.
- [x] `npm test` runs in CI after typecheck.
- [x] Real WASM bindings are type-checked in CI (`wasm-types-drift`).
- [ ] Tier 3 contract testing (explicitly deferred).

## See Also

- [[022-ci-js-client-enforcement/spec|Spec 022]], [[027-wasm-backend-node-init/spec|Spec 027]], [[010-wasm-typescript-type-generation/spec|Spec 010]]
- `docs/architecture/WASM_ROADMAP.md`
