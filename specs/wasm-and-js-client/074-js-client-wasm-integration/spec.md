---
aliases:
  - JS client WASM integration
  - WasmBackend WasmParser wasm-loader
tags:
  - sdd
  - spec
  - typescript
  - wasm
created: 2026-11-15
status: implemented
priority: P2
related:
  - "[[070-wasm-bindings-api/spec]]"
  - "[[072-wasm-packaging-targets/spec]]"
  - "[[073-js-client-core/spec]]"
  - "[[027-wasm-backend-node-init/spec]]"
  - "[[030-wasm-loader-esm-cjs-mismatch/spec]]"
  - "[[020-ts-client-wasm-api-sync/spec]]"
issue: "#383"
code:
  - crates/pjs-js-client/src/utils/wasm-loader.ts
  - crates/pjs-js-client/src/transport/wasm-backend.ts
  - crates/pjs-js-client/src/parser/wasm-parser.ts
  - crates/pjs-js-client/src/types/pjs-wasm.d.ts
  - crates/pjs-js-client/tests/integration/wasm-backend.test.ts
  - crates/pjs-js-client/tests/integration/wasm-parser.test.ts
  - crates/pjs-js-client/tests/integration/wasm-pkg.ts
  - crates/pjs-js-client/tests/parser/wasm-parser.test.ts
verified: 2026-11-15
---

# Spec 074: TypeScript client WASM integration

Part of [[MOC-specs]]; see [[constitution]].

> [!info] Verification (2026-11-15)
> Implemented. Evidence: `wasm-loader.ts`, `wasm-backend.ts`, `wasm-parser.ts`; CHANGELOG entries for #338 (module never assigned) and #383 (Node ESM/CJS loader, CI artifact download, loud CI failure when `pkg` missing). Bug-fix specs [[027-wasm-backend-node-init/spec]] and [[030-wasm-loader-esm-cjs-mismatch/spec]] cover the history.

## Overview

**Problem.** The client should stream and parse locally through `pjs-wasm` in browsers and Node.js, with graceful fallback when the optional package is absent.

**Goal.** Describe the loader, the `WasmBackend` transport, the `WasmParser`, and how they are tested against the real package.

**Out of scope.** The Rust API ([[070-wasm-bindings-api/spec]]); package build ([[072-wasm-packaging-targets/spec]]).

## Functional Requirements

### FR-001 Module loading
`loadWasmModule()` shall: in Node (`process.versions.node`) read `pjs-wasm/package.json` via `require.resolve`, read the `main` glue file, evaluate it as CommonJS (`evaluateEsmGlueAsCjs`: strips `export` statements, rewrites `import.meta.url`, injects `TextEncoder`/`TextDecoder` from `util`, runs under `'use strict'` via `new Function`), read `<main without .js>_bg.wasm` into a cached `Buffer`, and call `init({ module_or_path: buffer })`; in browsers `await import('pjs-wasm')` then `default()`. The in-flight promise is cached so concurrent callers share one instance; a failure clears the cache and rethrows with `cause`.
Code: `wasm-loader.ts`.
Status: done.

### FR-002 Loader failure modes
The loader shall throw explicit errors when `require` is unavailable (pure ESM Node), `main` is missing or not a string, or the glue file lacks a trailing `export { ... }` statement.
Status: done.

### FR-003 WasmBackend transport
`WasmBackend extends Transport` shall: `connect()` load the module and return `sessionId: 'wasm-local'` with features `['wasm','local-streaming','priority-streaming']` (idempotent when connected; failure becomes `PJSError(InitializationError)`); `startStream(endpoint, {jsonData, minPriority?})` require a connected backend, non-empty `jsonData`, and no active stream, then build a `PriorityStream`, `setMinPriority(minPriority ?? 1)`, wire callbacks and call `start(jsonData)`; `stopStream()` and `disconnect()` `free()` the active stream.
Code: `wasm-backend.ts`.
Status: done.

### FR-004 Frame conversion
`convertWasmFrame` shall map `FrameData` to the client `Frame`: type via case-insensitive `skeleton|patch|complete` (anything else, including `error`, throws `ValidationError`), `payload` JSON-parsed (failure -> `ParseError`), `sequence` coerced with `Number`, skeleton `complete:false`, patch `payload: { patches: payload.patches || [] }`, `metadata.source = 'wasm'`.
Status: done.

### FR-005 Completion
On `onComplete(stats)` the backend shall emit an additional `Complete` frame with `priority: Priority.Background` and `total_frames: stats.totalFrames`; on `onError(msg)` it shall emit `PJSError(StreamError)`.
Status: done. See Open Questions (duplicate Complete frame).

### FR-006 WasmParser
`WasmParser` shall: `initialize()` (returns `false` and stays native when `preferNative`; on load failure records `initError`, returns `false`); `parse(json)` use `PjsParser.parse` when WASM is available, else `JSON.parse`, wrapping failures in `PJSError(ParseError)`; `getImplementation()` return `'wasm'|'native'`; `stream(json, callbacks, minPriority=1)` require WASM (else `InitializationError`) and resolve on `onComplete`, reject on error; `dispose()` free the parser.
Code: `wasm-parser.ts`; `createWasmParser(options)`.
Status: done.

### FR-007 Fallback declarations
`src/types/pjs-wasm.d.ts` shall declare module `pjs-wasm` (FrameData, StreamStats, PjsParser, PriorityStream, `version`, `init`) for builds where the optional package is absent; the real generated `.d.ts` takes precedence when present. The hand-written subset must be kept in sync manually; `typecheck:wasm-drift` checks against the real bindings.
Status: done.

### FR-008 Test guard
Integration suites shall use `describeWasmPkg` (`tests/integration/wasm-pkg.ts`): when `crates/pjs-wasm/pkg` (or `../pjs-wasm/pkg`) exists run normally; otherwise `describe.skip` locally, and under `process.env.CI` register a failing test that explains how to build the package.
Status: done.

## Non-Functional Requirements

- NFR-COMPAT-001: loader must work in Jest (jsdom, no `--experimental-vm-modules`) and plain Node 22+.
- NFR-REL-001: a missing `pkg` in CI must fail, not skip (#383).
- NFR-CI-001: `js-client-test` depends on the `wasm-build-web` artifact (see [[078-ci-pipeline/spec]]).

## Data Model

`WasmStreamOptions extends StreamOptions { jsonData: string; minPriority?: number }`; `WasmParserOptions { debug?, preferNative? }`; `StreamingCallbacks { onFrame?, onComplete?, onError? }`.

## Edge Cases and Errors

| Condition | Error type |
|-----------|-----------|
| `startStream` before `connect` | `ConnectionError` |
| empty `jsonData` | `ValidationError` |
| second `startStream` while one is active | `StreamError` |
| unknown frame type from WASM | `ValidationError` |
| payload not JSON | `ParseError` |
| WASM missing for `WasmParser.stream` | `InitializationError` |

## Success Criteria

1. `npm test` in `crates/pjs-js-client` with `crates/pjs-wasm/pkg` built runs `wasm-backend.test.ts` (6 describe groups: Initialization, Streaming, Error Handling, Memory Management, Frame Conversion, Debug Mode) and `wasm-parser.test.ts` against the real module.
2. The mocked-module suite `tests/parser/wasm-parser.test.ts` passes without the package.

## Agent Boundaries

- Always: change `pjs-wasm.d.ts` together with the Rust JS API; keep `{ module_or_path }` init form.
- Ask first: moving the Node loader to native ESM.
- Never: call `free()` on a stream from inside its own `onComplete` before verifying the reentrancy issue (see Open Questions).

## Open Questions

- `PJSClient` configured with `TransportType.WASM` cannot stream: `PJSClient` calls `transport.startStream(endpoint, {sessionId, streamId, queryParams, headers})` and never supplies `jsonData`, so `WasmBackend` throws `ValidationError`. The WASM transport is usable only when `WasmBackend` is driven directly.
- `WasmBackend.onFrame` converts the Rust `complete` frame to a `Complete` frame and `onComplete` emits a second `Complete` frame, so consumers can receive two.
- `currentStream` is cleared only by `stopStream`/`disconnect`; after a normal completion the next `startStream` fails with "A stream is already active" until `stopStream()` is called.
- `mapFrameType` has no `error` case although Rust `FrameData.type` can be `"error"`.
- The CHANGELOG (#383) records a skipped test for `WasmParser.stream()` where `stream.free()` is called inside `onComplete` while `start()` is still on the WASM stack; `wasm-parser.ts` still does that. [NEEDS CLARIFICATION: current status of the skipped test and the hang]
- `WasmParser.parser: any` and `Frame.payload: any` weaken typing.
- Node loader evaluates the glue with `new Function`; it trusts the installed `pjs-wasm` package.
- Priority numbers produced by WASM (`HIGH = 80`) differ from TS `Priority.High = 75`.

## See Also

[[plan]] | [[070-wasm-bindings-api/spec]] | [[073-js-client-core/spec]] | [[027-wasm-backend-node-init/spec]] | [[030-wasm-loader-esm-cjs-mismatch/spec]] | `crates/pjs-js-client/examples/wasm-backend-demo.ts`
