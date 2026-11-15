---
aliases:
  - WasmBackend Node.js Init
  - WASM loader Node fetch file URL
tags:
  - sdd
  - spec
  - js-client
  - wasm
  - nodejs
created: 2026-08-07
status: implemented
priority: P1
issue: "#317"
pr: "#351"
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
  - "[[020-ts-client-wasm-api-sync/spec|Spec 020]]"
  - "[[022-ci-js-client-enforcement/spec|Spec 022]]"
  - "[[030-wasm-loader-esm-cjs-mismatch/spec|Spec 030]]"
code:
  - crates/pjs-js-client/src/utils/wasm-loader.ts
  - crates/pjs-js-client/src/transport/wasm-backend.ts
  - crates/pjs-js-client/src/parser/wasm-parser.ts
  - crates/pjs-js-client/tests/integration/wasm-backend.test.ts
  - crates/pjs-js-client/tests/integration/wasm-pkg.ts
verified: 2026-11-15
---

# Spec 027: WasmBackend.connect() in Node.js - fetch() Cannot Load file: URL

> [!info] Verification (2026-11-15)
> **State: implemented.** Issue #317 CLOSED 2026-08-14; fixed by PR #351 (`eef62e8`, also #338, #339). A follow-up regression (ESM/CJS mismatch in Node and Jest, issue #383) was fixed by PR #389 (`c46c027`); see [[030-wasm-loader-esm-cjs-mismatch/spec|Spec 030]].
>
> Evidence:
> - `src/utils/wasm-loader.ts` exports `loadWasmModule()`; `isNodeRuntime()` uses `typeof process !== 'undefined' && !!process.versions?.node`. Node path (`loadWasmModuleForNode`) reads the `.wasm` binary from the installed `pjs-wasm` package via `fs/promises` and calls `default({ module_or_path: buffer })`; browser path is unchanged (`import('pjs-wasm')` then `default()`).
> - `WasmBackend.connect()` and `WasmParser` both call `loadWasmModule()`; module and binary are cached (in-flight promise cached, cleared on failure).
> - `connect()` failure message is `Failed to initialize WASM backend: <underlying message>` and Node load failures are prefixed `Failed to load the pjs-wasm binary for Node.js:` with the original error as `cause`.
> - `tests/integration/wasm-pkg.ts` `describeWasmPkg` skips only when `pkg/` is absent and not in CI; under `CI` it fails loudly. CI job `js-client-test` downloads the built `wasm-build-web` artifact into `crates/pjs-wasm/pkg` before `npm test`.
>
> Drift from original spec:
> - Mechanism differs from the "possible direction": after #389 the Node path evaluates the generated ESM glue as CommonJS (`evaluateEsmGlueAsCjs`) rather than `import()`, because Jest cannot parse ESM in `node_modules`. It requires a CommonJS `require`; pure-ESM Node contexts without one are explicitly unsupported (clear error).
> - Binary is located via `require.resolve('pjs-wasm/package.json')` and the package `main` field.
> - CI wiring (open question 1) was resolved in favor of building the pkg in CI (see [[022-ci-js-client-enforcement/spec|Spec 022]]).
> - Non-Node, non-browser runtimes (Deno, Bun, workers) remain untested; they follow the browser (`fetch`) branch. Original open questions 2 and 3 stay open.

## Problem

`wasm-pack --target web` glue defaults to `fetch(new URL('pjs_wasm_bg.wasm', import.meta.url))`; Node's `fetch` cannot load `file:` URLs, so `WasmBackend.connect()` always failed under Node.js with a misleading "ensure pjs-wasm is installed" message. The wasm integration suite was silently skipped when `pkg/` was absent, hiding the bug.

## Functional Requirements

### FR-1 connect() succeeds in Node.js with pjs-wasm installed
Status: done (`loadWasmModuleForNode`).

### FR-2 Browser behavior unchanged
Status: done (`import('pjs-wasm')` + `default()`).

### FR-3 Runtime detected reliably, not by `fetch` sniffing
Status: done (`process.versions.node`).

### FR-4 Distinguishable error messages
Status: done. Message now carries the underlying cause instead of the fixed "Ensure pjs-wasm is installed" string.

### FR-5 No new required connect() options
Status: done.

## Non-Functional Requirements

- No new runtime dependency; uses Node built-ins (`fs/promises`, `path`, `url`, `util`).
- `pjs-wasm` stays an optional dependency.
- Regression visibility: suite fails (not skips) in CI when the package is missing.

## Edge Cases

| Scenario | Behavior |
|----------|----------|
| `pjs-wasm` not installed | `connect()` throws `PJSError(InitializationError)` with the underlying resolve error |
| Corrupt/missing `.wasm` | Same, message includes read/instantiate error; failure is not cached, a later call retries |
| Concurrent loads | Share one in-flight promise |
| Already connected | Early return preserved |
| Node without CommonJS `require` | Throws explicit unsupported-environment error |

## Acceptance Criteria

- [x] connect() succeeds in Node >= 18 with a built `pjs-wasm`.
- [x] Browser path unchanged.
- [x] Integration suite runs when `pkg/package.json` exists; fails if absent under CI.
- [x] Missing-package error distinguishable from fetch failure.
- [x] No new hard dependency.
- [ ] Deno/Bun/edge runtime behavior defined. [NEEDS CLARIFICATION: still open]

## See Also

- [[020-ts-client-wasm-api-sync/spec|Spec 020]], [[022-ci-js-client-enforcement/spec|Spec 022]], [[030-wasm-loader-esm-cjs-mismatch/spec|Spec 030]]
- `crates/pjs-js-client/examples/browser-wasm/example.js` (reference Node pattern)
