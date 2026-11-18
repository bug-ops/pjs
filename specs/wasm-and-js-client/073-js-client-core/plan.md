---
aliases:
  - JS client core plan
tags:
  - sdd
  - plan
  - typescript
created: 2026-11-15
status: partial
related:
  - "[[073-js-client-core/spec]]"
verified: 2026-11-18
---

# Plan 073: as-built

```mermaid
flowchart LR
  App --> C[PJSClient (EventEmitter)]
  C --> T{Transport}
  T --> H[HttpTransport NDJSON]
  T --> S[SSETransport]
  T --> W[WebSocketTransport]
  T --> B[WasmBackend]
  T -- frame --> C
  C --> FP[FrameProcessor.validateFrame]
  C --> R[JsonReconstructor per stream]
  R --> App
```

## Tooling

- Build: Rollup (`rollup.config.mjs`) emits ESM (`dist/index.esm.js`), CJS (`dist/index.js`), UMD (`dist/index.umd.js`, global `PJS`), with `ws` external.
- Test: Jest 30 + ts-jest ESM preset, `jsdom` environment, `tests/setup.ts` mocks `WebSocket`/`fetch` and replaces `console.log/warn/error/debug` with mocks.
- Lint: ESLint (`npm run lint`); not part of CI.
- Docs: `typedoc src/index.ts` (`npm run docs`).

## Contracts

| Function | Precondition | Postcondition |
|----------|--------------|---------------|
| `PJSClient.stream(endpoint, opts)` | `baseUrl` set | resolves once with final state after `Complete`, or rejects once (settled guard); frame listener removed in both cases |
| `JsonReconstructor.applyPatch(frame)` | skeleton processed | returns counts; never throws on bad patches; never mutates prototypes |
| `JsonReconstructor.getCurrentState()` | - | deep copy, safe to mutate |
| `FrameProcessor.validateFrame(x)` | any | `{isValid, errors}`; never throws |
| `Transport.startStream` | connected | frames delivered via `'frame'` events |

## Testing strategy

`tests/core/{client,frame-processor,json-reconstructor}.test.ts`, `tests/utils/utils.test.ts`, `tests/integration/full-stream.test.ts` (mock transport). Transports themselves (`http.ts`, `sse.ts`, `websocket.ts`) have no dedicated test files. [NEEDS CLARIFICATION: coverage of transports via client tests]

## Security notes

Untrusted frames reach `JsonReconstructor`; prototype-pollution guard FR-010. No size limit on accumulated state or on NDJSON line buffer (`HttpTransport` accumulates until newline).
