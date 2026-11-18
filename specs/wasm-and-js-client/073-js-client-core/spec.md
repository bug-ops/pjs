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
verified: 2026-11-18
---

# Spec 073: TypeScript client core (client, transports, reconstruction)

Part of [[MOC-specs]]; see [[constitution]].

> [!info] Verification (2026-11-15)
> Partially implemented: HTTP/SSE/WebSocket transports, event-driven `PJSClient`, frame validation and JSON reconstruction exist and are unit/integration tested. Several config options are accepted but unused, and the HTTP/SSE transports speak the shipped REST routes (FR-011, #735). Evidence: `crates/pjs-js-client/src/**`, `tests/core/*`, `tests/integration/full-stream.test.ts`, `tests/utils/utils.test.ts`; issues #178 (API mismatch, closed) and #180 (CI, closed).

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
`new PJSClient(config)` shall throw `PJSError(ConfigurationError)` when `baseUrl` is empty, strip a trailing slash, and default: `transport` HTTP, `sessionId` `''`, `headers` `{}`, `timeout` 30000 ms, `debug` false, `bufferSize` 1 MiB, `priorityThreshold` `Priority.Background`, `maxConcurrentStreams` 10, `maxStreamBytes` 16 MiB, `stateDelivery` `'live'`.
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
`stateDelivery` (#746) selects the per-frame state handed to `onRender` and `skeleton_ready`/`patch_applied` listeners: `'live'` (default) passes the reconstructor's own state without a copy, typed `DeepReadonly<JsonValue>` (read-only, not to be kept across frames); `'snapshot'` passes an independent deep clone per frame. The value `stream()` resolves with, and the `stream_complete`/final `onRender` data, is one clone taken at the Complete frame. `ProgressInfo` has `framesReceived`, `totalFrames?`, `elapsedTime`, `prioritiesReceived`, `completionPercentage?`; `bytesReceived` was removed (#766), and `framesReceived` equals `totalFrames` at completion when the transport sets `total_frames` to skeleton + patch + Complete frames.
Per frame the client shall emit `frame_received`; for a skeleton `skeleton_ready` and `onRender(data, {priority, isComplete:false, progress})`; for a patch frame one `patch_applied` (first patch only; the payload also carries `appliedPatches` and `skippedPatches`) and `onRender`; for the complete frame `stream_complete`, final `onRender` with `isComplete:true`, and `progress_update`/`onProgress`. `PJSEvent.Error` has a default no-op listener so unhandled errors do not crash the process.
Code: `client.ts`, `types/index.ts` (`PJSEvent`, `PJSEventData`).
Status: done.

### FR-006 Statistics
The client shall track per stream: total frames, priority distribution, time to first frame/skeleton/completion, and frames per second. `throughputMbps` and `memoryStats` are initialised to 0 and not updated.
Code: `client.ts`; `getStreamStats()`.
Status: partial.

### FR-007 Frame validation
`FrameProcessor.validateFrame` shall return `{isValid, errors}`: frame must be an object with a known `frame_type` (`Skeleton|Patch|Complete|Error`), numeric `priority` in 0..=100, skeleton requires `payload`, patch requires non-empty `payload.patches` with each patch having a path accepted by the shared path scanner `isValidJsonPath` (grammar below) and `operation` in `set|append|merge|delete`.
Code: `frame-processor.ts`; `utils/index.ts::validateFrame` is a second, looser type guard.
Status: done.

### FR-008 Frame sequencing state machine
`FrameProcessor.processFrame` shall reject (as `ProtocolViolation`) frames after completion, a patch before the skeleton, a duplicate skeleton, and a patch whose priority exceeds the previous patch's. `PJSClient` does not call it.
Code: `frame-processor.ts`.
Status: implemented but unused by the client (tested in `tests/core/frame-processor.test.ts`).

### FR-009 JSON reconstruction
`JsonReconstructor` shall: `processSkeleton` (deep clone payload; reject a second call), `applyPatch` (per patch: path `$` addresses the root, where `set` replaces the state with a deep clone of the value and `merge` shallow-merges a plain-object value into a plain-object root (any other root or value is skipped), while `append` pushes onto an array root (scalar once, array spread; any other root is skipped), `append_str` concatenates a string onto a string root, and `delete` at the root is skipped; every other path must match the grammar below; operations `set`, `append` (push scalar or spread array onto an existing array target; a missing or non-array target is skipped and nothing is created, #764), `merge` (shallow object merge onto an existing object target; a missing, non-object or non-object-valued merge is skipped, #764), `delete` (array index splice or own-property delete), `append_str` (concatenate a string onto an own string field or an existing string array element; any other target is skipped); unknown operations/paths counted as skipped), `getCurrentState()` (deep clone), `getLiveState()` (the state itself, read-only, no copy), `reset`, `markComplete`, `getMetadata`. Path grammar (`core/json-path.ts`, internal, shared with `FrameProcessor` and `parseJsonPath`): `$` followed by any number of `.plain`, `[index]` or `["quoted"]` segments. `plain` is non-empty without `.`, `[`, `]`. `index` is `0` or `[1-9][0-9]*` (ASCII, no sign, no leading zeros) up to `Number.MAX_SAFE_INTEGER`; Rust's bound is `usize` (documented divergence; no real array reaches either). `quoted` is the canonical `JSON.stringify` literal of a key that is not plain and has no lone surrogates; non-canonical escapes, plain keys in quotes and a missing `]` are rejected (the Rust grammar of spec 042). `$` alone addresses the root and `$[0]...` indexes an array root. Resolution is strict: missing intermediate object keys followed by a key are created, but every index must already exist (`n < length`, on an array), so an out-of-range, non-array or under-a-missing-key index skips the patch instead of creating sparse arrays or literal keys. Root `merge` of two plain objects mutates the state in place.
Code: `json-reconstructor.ts`.
Status: done.

### FR-010 Prototype-pollution defence
Keys `__proto__`, `constructor` and `prototype` are ordinary data: the reconstructor stores and reads them as own properties for every operation (`set`, `merge`, `append`, `delete`, intermediate and tail). Every read on the walk and at the tail goes through an own-property check, writes use `Object.defineProperty`, intermediates are `Object.create(null)`, and deep clones use null-prototype objects, so no patch can reach a prototype. The former deny-list (`UNSAFE_KEYS`, `assertSafeKey`) was removed (#712).
Code: `json-reconstructor.ts`; test `tests/core/reconstructor-own-keys-acceptance.test.ts`.
Status: done.

### FR-011 Transports
- `HttpTransport` (#735): `startStream` requires `StreamOptions.data` (`ConfigurationError` otherwise) and ignores `endpoint`. `connect()` posts `POST {base}/pjs/sessions {client_info: string}`; a stream runs `POST /pjs/sessions/{sid}/streams {data}`, `POST .../start`, `POST .../generate-frames {priority_threshold, max_frames: 1000}`, then `GET .../frames/stream?priority=<priorityThreshold>` with `Accept: application/x-ndjson` (line-buffered, abortable; the terminal line `{"event":"shutdown"}` is a shutdown marker (FR-013), not a frame; request timeout via `AbortSignal.timeout(config.timeout)`). The page holds Patch frames only: the client synthesizes the Skeleton (payload `{}`) and the Complete frame with `total_frames` = skeleton + patches + Complete. One stream at a time: a second `startStream` rejects the first with `STREAM_ERROR`; `disconnect()` during a stream rejects it with `ConnectionError`. The page is generated once (not a live tail) and capped at 1000 frames.
- `SSETransport` (#735): same REST sequence as HTTP (session created by `connect()`, returning its `sessionId`); frames come from an `EventSource` on `.../frames/stream`, handling `message`, `frame`, server `error` and the named `shutdown` event (FR-013). Headers apply to the REST calls only; the source is closed at the end of the page; no reconnection; Skeleton and Complete are synthesized as for HTTP.
- `WebSocketTransport`: connects to `{base with http->ws}/ws` and speaks the shipped `WsMessage` protocol with patch framing. `startStream` requires `StreamOptions.data` (`JsonValue`, `null` allowed) and `crypto.randomUUID` (`ConfigurationError` otherwise), ignores `endpoint`, and sends `StreamInit {session_id, data, options: {max_frame_size: 65536, framing: 'patch'}}`. The `StreamInit` is serialized first and the active stream is registered only after `socket.send` succeeds, so a `JSON.stringify` or `send` failure (BigInt, cyclic document) rejects `startStream` with that error and leaves no orphaned stream (#763); a serialization failure leaves the previous stream untouched, a `send` failure still supersedes it. `StreamPatch {frame_id, priority, patches: [[op, [segment...], value]]}` maps to a lazily emitted Skeleton frame (payload `{}`, priority 100) followed by Patch frames whose paths come from `formatJsonPath`; `StreamComplete` maps to a Complete frame with `total_frames` (the Skeleton, the patch frames and the Complete frame itself, #766) and resolves `startStream`. Receiver limits (#746, spec 067): a patch path of more than `MAX_PATCH_PATH_DEPTH` (128) segments, or more than `maxStreamBytes` cumulative UTF-8 bytes of `StreamPatch` messages in one stream, rejects the stream with `ProtocolError` and sends `StreamCancel` before any frame of that message is emitted. A throwing `frame` listener on the Skeleton or a Patch frame sends `StreamCancel` and rejects `startStream` with `StreamError` whose `originalError` is the thrown value (#739); nothing escapes `onmessage`. An `Error {session_id, error, code}` for the active session or a null session id rejects with `ProtocolError` (context `{code, error}`); a malformed `StreamPatch` or a `frame_id` gap rejects with `ProtocolError` and sends `StreamCancel`, as does an `Error` reply with a non-numeric `code` or non-string `error` (`Malformed Error reply`); a close before `StreamComplete` rejects with `ConnectionError`. One stream at a time: a second `startStream` sends `StreamCancel` and rejects the first with `StreamError` (`Stream superseded`), `stopStream()` does the same (`Stream stopped`), and `disconnect()` rejects a pending stream and a pending `connect()` with `ConnectionError` (`Client disconnected`). No automatic reconnection; events from a replaced socket are ignored. A document over the server's inbound cap (default 1 MiB per message) makes the server close the socket, so the stream rejects with `ConnectionError` and no code; 413 comes only from plan limits. A null-session `Error` (undecodable message, e.g. 400) carries no id and is attributed to the active stream, so after a superseded `StreamInit` it can fail the stream that replaced it. A document nested past the serde recursion limit gets 400 instead of the 413 `PathTooDeep` that spec 066 describes (#737).
- `Transport` base class: `connect`, `disconnect`, `startStream`, `stopStream`; emits `frame`, `error`, `disconnect`, `shutdown` (`SHUTDOWN_EVENT`).
Code: `transport/*.ts`.
Status: `WebSocketTransport` works against a stock `pjs-core` WebSocket server (#731; server built with `into_make_service_with_connect_info::<SocketAddr>()`; browsers need `AxumWebSocketTransport::with_allowed_origins`). HTTP and SSE work against the shipped REST routes (#735).
Test: `tests/transport/{http,sse}-shipped-routes.test.ts`, `tests/transport/wire-format.test.ts` (HTTP, SSE), `tests/transport/websocket-protocol.test.ts` (fake socket) and `tests/cross-stack/websocket.cross-stack.test.ts` (compiled bundle against `crates/pjs-core/examples/js_client_ws_fixture.rs`).

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
- NFR-ENV-001: `engines.node >= 22` (#743); CI runs the supported Node versions for tests/type checks.
- NFR-SEC-001: reconstruction must never mutate `Object.prototype` (FR-010).

## Data Model

`Priority` enum (Critical 100, High 75, Medium 50, Low 25, Background 10); `FrameType` string enum; `Frame = SkeletonFrame | PatchFrame | CompleteFrame` (`frame_type`, `priority`, optional `stream_id`, `sequence`, `timestamp`, `metadata`); `PatchOperation { path: string; value: any; operation }`; `PJSClientConfig`; `PJSEvent` and `PJSEventData`; `StreamStats`, `ProgressInfo`, `PerformanceMetrics`; `PJSError(type, message, context?, originalError?)` with `PJSErrorType` (10 members, including `ServerShutdown`); `StreamOptions`.

## Edge Cases and Errors

| Condition | Result |
|-----------|--------|
| Patch before skeleton (`PJSClient`) | `PJSError(ProtocolError)` |
| Patch path not matching the grammar (no `$`, empty segment, bad index incl. leading zeros, missing `]`) | skipped; rejected by `FrameProcessor.validateFrame` |
| Index past the end, on a non-array, or under a missing key | patch skipped, no sparse array or literal key created |
| Root `$` `append`/`delete` | skipped |
| `delete` on missing key | no-op counted as applied; out-of-range index skipped |
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
- Unused configuration: `bufferSize`, `maxConcurrentStreams` and `StreamOptions.priorityStrategy` are accepted and defaulted but never read.
- `calculateHeuristicPriority` in `utils/index.ts` is a third, substring-based priority table that is exported but unused by the client and differs from `compute_priority` in the domain crate.
- Two frame validators (`FrameProcessor.validateFrame` and `utils.validateFrame`) with different rules (priority range, error reporting).
- Endpoint compatibility: the real routes are `POST /pjs/sessions`, `/pjs/sessions/{id}/streams/{stream_id}/frames/stream`, and WebSocket `/ws`; the WebSocket transport speaks the shipped protocol, and HTTP and SSE follow the shipped REST routes (FR-011).
- Version strings disagree: `package.json` `0.3.0-alpha.2`, `src/index.ts` `VERSION` and HTTP `client_info.version` `0.3.0-alpha.1`.
- `ws` is a dependency and optional peer dependency but no source file imports it.
- `createTransport` uses `require()` for the WASM branch inside an ESM-first TypeScript module.
- `PJSClient.stream` timeout uses `||`, so a `0` timeout falls back to the default.
- `PatchApplied` is emitted only for the first patch of a frame.
- `any` is widespread in public types (`Frame.payload`, `PatchOperation.value`, `stream<T = any>`), contrary to the strict-typing rule.

## See Also

[[plan]] | [[074-js-client-wasm-integration/spec]] | [[020-ts-client-wasm-api-sync/spec]] | [[022-ci-js-client-enforcement/spec]] | `crates/pjs-js-client/README.md` | `docs/architecture/SPECIFICATION.md`
