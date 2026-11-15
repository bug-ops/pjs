---
aliases:
  - CI JS Client Enforcement
  - js-client-test job
tags:
  - sdd
  - spec
  - ci
  - js-client
created: 2026-04-28
status: implemented
priority: P2
issue: "#180"
pr: "#192"
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
  - "[[020-ts-client-wasm-api-sync/spec|Spec 020]]"
  - "[[027-wasm-backend-node-init/spec|Spec 027]]"
code:
  - .github/workflows/ci.yml
  - crates/pjs-js-client/package.json
verified: 2026-11-15
---

# Spec 022: CI Enforcement for pjs-js-client npm Tests

> [!info] Verification (2026-11-15)
> **State: implemented.** Issue #180 CLOSED 2026-04-28; job added by PR #192 (`dce0e2b`), extended by PR #351 (typecheck step, `wasm-types-drift` job) and #389.
>
> Evidence: `.github/workflows/ci.yml` job `js-client-test` (runs `npm ci`, `npm run typecheck`, `npm test` in `crates/pjs-js-client`), listed in `ci-success` `needs` and in its `check` script.
>
> Drift from original spec:
> - Node version is 20 (spec said 22); actions are pinned to commit SHAs.
> - `needs: [changes, wasm-build]`: the job downloads the real `wasm-build-web` artifact into `crates/pjs-wasm/pkg` instead of mocking `pjs-wasm` in Jest (spec Option 1). Gate condition adds `needs.changes.outputs.wasm == 'true'` to the `js` filter, so WASM-only PRs also run it.
> - No `continue-on-error` on the job; #178 was fixed in the same PR #192.
> - `tsc --noEmit` runs as `npm run typecheck`, not a bare `npx` call.
> - Node 18 remains pinned in the `wasm-node-example` and `wasm-package-validation` jobs (spec's follow-up to bump them is not done).

## Problem

`npm test` for `pjs-js-client` was never run in CI, so 71 failing tests and later regressions merged undetected.

## Functional Requirements

### FR-1 A `js-client-test` job runs `npm ci && npm test`
Status: done.

### FR-2 Job is gated on the `js` path filter
Status: done (plus `wasm` filter, push, schedule, workflow_dispatch).

### FR-3 TypeScript typecheck runs before Jest
Status: done (`npm run typecheck`).

### FR-4 Job reports to `ci-success`
Status: done.

### FR-5 Single LTS Node version
Status: done with Node 20 instead of 22. Open: bump 18-pinned jobs. [NEEDS CLARIFICATION: target Node version policy]

## Non-Functional Requirements

- Job timeout is 10 minutes (spec target under 3 minutes of test time).
- WASM-dependent suites use `describeWasmPkg` (`tests/integration/wasm-pkg.ts`): skip locally when `pkg/` is absent, fail loudly when `CI` is set.

## Acceptance Criteria

- [x] PRs touching `crates/pjs-js-client/**` run the job.
- [x] Job failure blocks `ci-success`.
- [x] Rust-only PRs skip the job.
- [ ] All jobs on a single current LTS Node.

## See Also

- [[020-ts-client-wasm-api-sync/spec|Spec 020]], [[027-wasm-backend-node-init/spec|Spec 027]]
- [[023-implementation-roadmap/spec|Spec 023]]
