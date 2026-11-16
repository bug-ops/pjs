---
aliases:
  - JS client core
  - pjs-js-client transports and reconstruction
tags:
  - sdd
  - spec
  - typescript
  - client
created: 2026-11-15
status: partial
priority: P2
related:
  - "[[074-js-client-wasm-integration/spec]]"
  - "[[020-ts-client-wasm-api-sync/spec]]"
  - "[[022-ci-js-client-enforcement/spec]]"
  - "[[070-wasm-bindings-api/spec]]"
issue: "#178"
code:
  - crates/pjs-js-client/src/core/client.ts
  - crates/pjs-js-client/src/core/frame-processor.ts
  - crates/pjs-js-client/src/core/json-reconstructor.ts
  - crates/pjs-js-client/src/transport/base.ts
  - crates/pjs-js-client/src/transport/http.ts
  - crates/pjs-js-client/src/transport/sse.ts
  - crates/pjs-js-client/src/transport/websocket.ts
  - crates/pjs-js-client/src/types/index.ts
  - crates/pjs-js-client/src/utils/index.ts
  - crates/pjs-js-client/src/index.ts
verified: 2026-11-15
---

# Spec 073: TypeScript client core (client, transports, reconstruction)

Part of [[MOC-specs]]; see [[constitution]].

> [!info] Verification (2026-11-15)
> Partially implemented: HTTP/SSE/WebSocket transports, event-driven `PJSClient`, frame validation and JSON reconstruction exist and are unit/integration tested. Several config options are accepted but unused, and the transports' endpoint paths have no verified counterpart in `pjs-core` routes (see Open Questions). Evidence: `crates/pjs-js-client/src/**`, `tests/core/*`, `tests/integration/full-stream.test.ts`, `tests/utils/utils.test.ts`; issues #178 (API mismatch, closed) and #180 (CI, closed).

## Overview

**Problem.** Browser and Node.js apps need to consume a PJS stream and render partial data as it arrives.

**Goal.** An event-emitting `PJSClient` that connects through a pluggable `Transport`, validates frames, rebuilds JSON progressively from a skeleton plus patches, and reports progress and statistics.

**Out of scope.** WASM-backed local streaming ([[074-js-client-wasm-integration/spec]]); server routes.

## User Stories

- As a front-end developer I call `client.stream(endpoint, { onRender })` and re-render on every frame.
- As a developer I listen to typed events (`skeleton_ready`, `patch_applied`, `stream_complete`, ...).
- As a developer I choose HTTP NDJSON, SSE or WebSocket by config.

## Functional Requirements

### FR-001 Configuration normalisation
`new PJSClient(config)` shall throw `PJSError(ConfigurationError)` when `baseUrl` is empty, strip a trailing slash, and default: `transport` HTTP, `sessionId` `''`, `headers` `{}`, `timeout` 30000 ms, `debug` false, `bufferSize` 1 MiB, `priorityThreshold` `Priority.Background`, `maxConcurrentStreams` 10.
Code: `client.ts::validateAndNormalizeConfig`.
Status: done (but see FR-012).

### FR-002 Transport selection
`TransportType` is `'http' | 'websocket' | 'sse' | 'wasm'`. The client shall instantiate `HttpTransport`, `WebSocketTransport`, `SSETransport` or `WasmBackend` accordingly and throw `ConfigurationError` otherwise. The WASM branch loads `WasmBackend` through a CommonJS `require('../transport/wasm-backend.js')`.
Code: `client.ts::createTransport`.
Status: done; WASM path see [[074-js-client-wasm-integration/spec]].

### FR-003 Connect / disconnect
`connect()` shall reject with `ConnectionError` if already connected, call `transport.connect()`, store `sessionData.sessionId || config.sessionId`, and emit `connected`. `disconnect()` is a no-op when not connected; otherwise it disconnects the transport, clears stream stats, and emits `disconnected`. Both emit `error` with a context string on failure.
Code: `client.ts`.
Status: done.

### FR-004 stream()
`stream<T>(endpoint, options)` shall auto-connect, generate a stream id (`stream_<ms>_<random>`), register a `StreamStats` record, and resolve with the reconstructed JSON on the first `Complete` frame, or reject with a `PJSError`: `ValidationError` for an invalid frame, `ProtocolError` for a patch before a skeleton or processing failure, `TimeoutError` after `options.timeout || config.timeout`, or the transport error. Each call owns a private `JsonReconstructor`.
Code: `client.ts::stream`, `processStream`.
Status: done.

### FR-005 Events and callbacks
Per frame the client shall emit `frame_received`; for a skeleton `skeleton_ready` and `onRender(data, {priority, isComplete:false, progress})`; for a patch frame one `patch_applied` (first patch only) and `onRender`; for the complete frame `stream_complete`, final `onRender` with `isComplete:true`, and `progress_update`/`onProgress`. `PJSEvent.Error` has a default no-op listener so unhandled errors do not crash the process.
Code: `client.ts`, `types/index.ts` (`PJSEvent`, `PJSEventData`).
Status: done.

### FR-006 Statistics
The client shall track per stream: total frames, priority distribution, time to first frame/skeleton/completion, and frames per second. `throughputMbps` and `memoryStats` are initialised to 0 and not updated.
Code: `client.ts`; `getStreamStats()`.
Status: partial.

### FR-007 Frame validation
`FrameProcessor.validateFrame` shall return `{isValid, errors}`: frame must be an object with a known `frame_type` (`Skeleton|Patch|Complete|Error`), numeric `priority` in 0..=100, skeleton requires `payload`, patch requires non-empty `payload.patches` with each patch having a path matching `^\$(\.[^.[\]]+(\[\d+\])?)*$` (key grammar aligned with Rust `JsonPath::is_valid_key`: any non-empty key without `.`, `[`, `]`) and `operation` in `set|append|merge|delete`.
Code: `frame-processor.ts`; `utils/index.ts::validateFrame` is a second, looser type guard.
Status: done.

### FR-008 Frame sequencing state machine
`FrameProcessor.processFrame` shall reject (as `ProtocolViolation`) frames after completion, a patch before the skeleton, a duplicate skeleton, and a patch whose priority exceeds the previous patch's. `PJSClient` does not call it.
Code: `frame-processor.ts`.
Status: implemented but unused by the client (tested in `tests/core/frame-processor.test.ts`).

### FR-009 JSON reconstruction
`JsonReconstructor` shall: `processSkeleton` (deep clone payload; reject a second call), `applyPatch` (per patch: path `$` addresses the root, where `set` replaces the state with a deep clone of the value and `merge` shallow-merges plain objects (otherwise replaces), while `append`/`delete` at the root are skipped; every other path must start with `$.`; operations `set`, `append` (push scalar or spread array), `merge` (shallow object merge, otherwise replace), `delete` (array index splice or own-property delete); unknown operations/paths counted as skipped), `getCurrentState()` (deep clone), `reset`, `markComplete`, `getMetadata`. Path grammar: dot-separated segments with optional `[n]` index. Missing intermediate object segments are created; a missing indexed parent throws and the patch is skipped.
Code: `json-reconstructor.ts`.
Status: done.

### FR-010 Prototype-pollution defence
The reconstructor shall reject path segments `__proto__`, `constructor`, `prototype`, create intermediate objects with `Object.create(null)`, write values with `Object.defineProperty`, and deep-clone with null-prototype objects.
Code: `json-reconstructor.ts` (`UNSAFE_KEYS`, `assertSafeKey`).
Status: done.

### FR-011 Transports
- `HttpTransport`: `POST {base}/pjs/sessions` with `client_info` body to obtain `session_id`; stream via `GET {base}/pjs/stream{endpoint}?session_id&stream_id&...` with `Accept: application/x-ndjson`, line-buffered NDJSON parse, abortable; the terminal line `{"event":"shutdown"}` is a shutdown marker (FR-013), not a frame; request timeout via `AbortSignal.timeout(config.timeout)`.
- `SSETransport`: `connect()` is local only; stream via `EventSource` at `{base}/pjs/stream{endpoint}/sse?...`, handling `message`, `frame`, server `error` and the named `shutdown` event (FR-013).
- `WebSocketTransport`: connects to `{base with http->ws}/pjs/ws`, sends `{type:'start_stream', endpoint, session_id, stream_id, query_params, headers}` / `{type:'stop_stream'}`, expects `{type:'frame', frame}` or `{type:'error', error}`; reconnects with exponential backoff (base 1000 ms, up to 5 attempts) on non-1000 close.
- `Transport` base class: `connect`, `disconnect`, `startStream`, `stopStream`; emits `frame`, `error`, `disconnect`, `shutdown` (`SHUTDOWN_EVENT`).
Code: `transport/*.ts`.
Status: done as code; endpoint compatibility with the server is unverified.

### FR-012 Public exports
`src/index.ts` shall export `PJSClient`, `FrameProcessor`, `JsonReconstructor`, the transports, `WasmBackend`/`createWasmBackend`, `WasmParser`/`createWasmParser`, all types, `createPJSClient`, `validateFrame`, `parseJsonPath`, `VERSION`, `DEFAULT_CONFIG`, `quickStart`.
Status: done.

### FR-013 Server shutdown notification
A stopping server (spec 037 FR-008) ends SSE and NDJSON streams with a marker: SSE `event: shutdown` with data `{"event":"shutdown"}`, NDJSON the terminal line `{"event":"shutdown"}`. `isShutdownMarker(value)` (`transport/base.ts`) accepts exactly an object whose only key is `event` with value `"shutdown"`; any other object (extra keys, other value) is an ordinary frame. A transport shall emit `shutdown` once per stream, never as a frame or an error: `SSETransport` closes the `EventSource` so it cannot auto-reconnect, `HttpTransport` cancels the reader and ignores everything after the marker. `PJSClient` shall emit `PJSEvent.ServerShutdown` (`server_shutdown`) and then `disconnected` with reason `server shutdown`, set the client disconnected, and reject a pending `stream()` that has not settled with `PJSError(PJSErrorType.ServerShutdown)` (`'SERVER_SHUTDOWN'`); an already settled promise is untouched. The client never reconnects on its own.
Code: `transport/base.ts`, `transport/sse.ts`, `transport/http.ts`, `core/client.ts`.
Status: done (#644). Tests: `tests/transport/{sse,http,shutdown-marker,client-shutdown}.test.ts`, `tests/core/client-shutdown-rejection.test.ts`.

## Non-Functional Requirements

- NFR-TYPE-001: `tsconfig.json` has `strict`, `noUnusedLocals`, `noUnusedParameters`; `npm run typecheck` is a CI step (`js-client-test`).
- NFR-TEST-001: Jest coverage threshold 80% global (branches/functions/lines/statements) is configured in `jest.config.js` but `npm test` runs without `--coverage`, so it is not enforced in CI.
- NFR-ENV-001: `engines.node >= 18`; CI runs Node 20 for tests/type checks.
- NFR-SEC-001: reconstruction must never mutate `Object.prototype` (FR-010).

## Data Model

`Priority` enum (Critical 100, High 75, Medium 50, Low 25, Background 10); `FrameType` string enum; `Frame = SkeletonFrame | PatchFrame | CompleteFrame` (`frame_type`, `priority`, optional `stream_id`, `sequence`, `timestamp`, `metadata`); `PatchOperation { path: string; value: any; operation }`; `PJSClientConfig`; `PJSEvent` and `PJSEventData`; `StreamStats`, `ProgressInfo`, `PerformanceMetrics`; `PJSError(type, message, context?, originalError?)` with `PJSErrorType` (10 members, including `ServerShutdown`); `StreamOptions`.

## Edge Cases and Errors

| Condition | Result |
|-----------|--------|
| Patch before skeleton (`PJSClient`) | `PJSError(ProtocolError)` |
| Patch path neither `$` nor `$.`-prefixed | skipped |
| Root `$` `append`/`delete` | skipped |
| Unsafe key in path | patch skipped (error caught) |
| `delete` on missing key | no-op counted as applied |
| NDJSON line unparsable | `ParseError` emitted, stream continues |
| WebSocket closes with code != 1000 | up to 5 reconnects |
| No frames within timeout | `TimeoutError` |
| Server shutdown marker (SSE event or NDJSON line) | `server_shutdown` then `disconnected`; pending `stream()` rejects with `ServerShutdown`; no reconnect |

## Success Criteria

1. `npm run typecheck && npm test` pass in `crates/pjs-js-client` (CI `js-client-test`).
2. A mocked transport emitting skeleton, patches and complete yields the final merged object and the documented events (`tests/integration/full-stream.test.ts`).

## Agent Boundaries

- Always: keep `Priority` values and frame/patch field names aligned with the Rust domain (`FramePatch { path, operation, value }`, lowercase operations).
- Ask first: changing endpoint paths or the event names.
- Never: use `any`-typed new public surface where a union exists; touch `Object.prototype`-reachable keys in the reconstructor.

## Open Questions

- Priority mismatch: TS `Priority.High = 75`; Rust `Priority::HIGH = 80`. FrameProcessor accepts 0..=100 while Rust priorities are 1..=255.
- Unused configuration: `bufferSize`, `priorityThreshold`, `maxConcurrentStreams` and `StreamOptions.priorityStrategy` are accepted and defaulted but never read.
- `calculateHeuristicPriority` in `utils/index.ts` is a third, substring-based priority table that is exported but unused by the client and differs from `compute_priority` in the domain crate.
- No test asserts the prototype-pollution defence (FR-010): `tests/core/json-reconstructor.test.ts` has no `__proto__`/`prototype` case.
- Two frame validators (`FrameProcessor.validateFrame` and `utils.validateFrame`) with different rules (priority range, error reporting).
- Endpoint compatibility: no route `/pjs/stream{endpoint}`, `/pjs/stream{endpoint}/sse`, or `/pjs/ws` was found in `crates/pjs-core/src/infrastructure/http`; the server routes are `/pjs/sessions/{id}/streams/{stream_id}/frames/stream` and `/pjs/ws/{session_id}` (per `.claude/rules/continuous-improvement.md`). [NEEDS CLARIFICATION: which server the client targets]
- Version strings disagree: `package.json` `0.3.0-alpha.2`, `src/index.ts` `VERSION` and HTTP `client_info.version` `0.3.0-alpha.1`.
- `ws` is a dependency and optional peer dependency but no source file imports it.
- `createTransport` uses `require()` for the WASM branch inside an ESM-first TypeScript module.
- `PJSClient.stream` timeout uses `||`, so a `0` timeout falls back to the default.
- `PatchApplied` is emitted only for the first patch of a frame.
- `any` is widespread in public types (`Frame.payload`, `PatchOperation.value`, `stream<T = any>`), contrary to the strict-typing rule.
- A nested `.github/workflows/ci.yml` inside `crates/pjs-js-client/` is inert (GitHub only reads the root `.github`); see [[078-ci-pipeline/spec]].

## See Also

[[plan]] | [[074-js-client-wasm-integration/spec]] | [[020-ts-client-wasm-api-sync/spec]] | [[022-ci-js-client-enforcement/spec]] | `crates/pjs-js-client/README.md` | `docs/architecture/SPECIFICATION.md`
