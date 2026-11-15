---
aliases:
  - HTTP streaming SSE NDJSON
  - Accept negotiation
tags:
  - sdd
  - spec
  - http
  - streaming
  - sse
  - ndjson
created: 2026-11-15
status: implemented
priority: P2
issue: "#511"
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
  - "[[060-http-rest-api/spec|HTTP REST API]]"
  - "[[015-batch-stream-ndjson-format/spec|BatchFrameStream NDJSON]]"
  - "[[025-streaming-response-content-type-helper/spec|Content-Type helper]]"
  - "[[035-streaming-module-unreachable/spec|Streaming module reachability]]"
  - "[[036-accept-header-negotiation-crate/spec|Accept negotiation crate]]"
code:
  - crates/pjs-core/src/infrastructure/http/streaming.rs
  - crates/pjs-core/src/infrastructure/http/handlers/streams.rs
  - crates/pjs-bench/benches/http_streaming.rs
verified: 2026-11-15
---

# HTTP frame streaming and `Accept` negotiation

## Overview

**Problem.** Clients want frames delivered incrementally in a framing they can parse (SSE for `EventSource`, NDJSON for fetch streams) selected by `Accept`.

**Goal.** `GET /pjs/sessions/{sid}/streams/{stid}/frames/stream` serializes one already-materialised page of frames one frame at a time; `StreamFormat::from_accept_header` selects the format per RFC 9110.

**Out of scope.** Live tail or subscription (needs a `FrameSourceGat`-backed port that does not exist in production); binary wire format; `retry:`/`Last-Event-ID`; WebSocket push ([[066-websocket-server-transport/spec|066]]).

Feature gate: `http-server` (deps `async-stream`, `headers-accept`, `mediatype`).

## User Stories

- As a browser client I get `text/event-stream` and must `.close()` the `EventSource` after the final event.
- As a script client I get `application/x-ndjson` by default.
- As an integrator I can reuse `BatchFrameStream` and `create_streaming_response*` outside the route.

## Functional Requirements

### FR-001 Streaming route

The handler shall run the same query as `GET .../frames` (`since_sequence`, `priority`, `limit`; page capped by `MAX_PAGINATION_LIMIT`; same 400/404 errors), then emit frames through `BatchFrameStream::new(stream::iter(frames), format, 1)` (one frame per chunk). The response carries `X-Total-Count` equal to `FramesResponse.total_count`, `Cache-Control: no-cache`, and the format's `Content-Type`. Code: `stream_stream_frames`.

### FR-002 Format selection

| `Accept` outcome | `StreamFormat` | `Content-Type` of this route |
|---|---|---|
| `text/event-stream` preferred | `ServerSentEvents` | `text/event-stream` (plus `X-Accel-Buffering: no`) |
| `application/x-ndjson` preferred | `NdJson` | `application/x-ndjson` |
| absent, `*/*`, `application/json`, `application/octet-stream`, unknown, unparsable | `Json` (or `Binary` normalised to `Json`) | `application/x-ndjson` |

The route never returns 406. `StreamFormat::Binary` is normalised to `Json` because no binary wire format exists.

### FR-003 Negotiation algorithm (`StreamFormat::from_accept_header`)

- Candidates in tie-break order: `application/json` (Json), `text/event-stream` (SSE), `application/x-ndjson` (NdJson), `application/octet-stream` (Binary).
- Delegated to `headers_accept::Accept::negotiate`: highest `q`, then specificity (exact, `application/*`, `*/*`).
- Wildcards restricted to exactly `*/*` and `application/*` (case-insensitive); other ranges containing `*` are dropped.
- `q=0` drops the entry; non-finite or unparsable `q` drops the entry; finite out-of-range `q` is clamped to [0,1]; a positive `q` is never rounded down to 0.000.
- Each entry is validated as a media type independently; a malformed entry is dropped without discarding the rest.
- At most 16 comma-separated entries (`MAX_ACCEPT_ENTRIES`) are considered; the rest are ignored.
- Missing header, non-UTF-8 value, or no surviving entry yields `Json`.

### FR-004 Serialization

`BatchFrameStream::into_stream()` shall yield `Result<Vec<u8>, StreamTransportError>` per batch of `batch_size` frames (final partial batch flushed):

- `Json` and `NdJson`: one JSON object per frame followed by `\n` (identical bytes; only `content_type()` differs; `BatchFrameStream::content_type()` promotes `Json` to `application/x-ndjson`).
- `ServerSentEvents`: `data: <frame json>\n\n` per frame, no `id:`/`event:`/`retry:` fields.
- `Binary`: the whole batch as one JSON array via `sonic_rs::to_vec`.

### FR-005 Response builders

`create_streaming_response(stream, format)` sets `200`, `Content-Type` from the format, `Cache-Control: no-cache`, and `X-Accel-Buffering: no` for SSE; it sets no `Connection` or `Transfer-Encoding` (hyper owns framing; both are illegal on HTTP/2). `create_streaming_response_with_content_type(stream, content_type)` does the same with an explicit type and no `X-Accel-Buffering`. Builder failures map to `StreamTransportError::Io` and surface to clients as a generic 500 (`PjsError::HttpError`).

## Non-Functional Requirements

- NFR-SEC-001: header iteration bounded to 16 entries before parsing.
- NFR-PERF-001: serialization uses `sonic_rs::to_vec`; `crates/pjs-bench/benches/http_streaming.rs` measures `BatchFrameStream`/format throughput [NEEDS CLARIFICATION: no recorded baseline numbers in repo].
- NFR-TEST-001: `tests/http_streaming_comprehensive.rs` (46 tests) and `streaming.rs` unit tests (7) cover negotiation edge cases and framing.

## Data Model

`StreamFormat { Json, NdJson, ServerSentEvents, Binary }` (`Debug, Clone, Copy`; no `PartialEq`), `BatchFrameStream<S>`, `StreamTransportError { Serialization(sonic_rs::Error), Io(String), BufferOverflow, StreamClosed }`.

## Edge Cases and Errors

| Case | Behaviour |
|---|---|
| `Accept: application/json;q=0, */*` | SSE |
| `Accept: application/json;q=0, application/*` | NDJSON |
| `Accept: text/*` | wildcard dropped, falls back to Json (NDJSON on the wire) |
| `q=nan` | entry dropped |
| 17+ entries | entries past the 16th ignored |
| Empty page | empty body, `X-Total-Count` still set |
| Invalid priority | 400 `InvalidPriority` |

## Success Criteria

- Each of the three outcomes in FR-002 is reachable by `Accept` alone.
- Wire bytes for NDJSON and SSE match FR-004 exactly.

## Agent Boundaries

- Always: keep negotiation bounded and fail-soft (no 406).
- Ask first: adding `Last-Event-ID`/`retry:` or a live-tail source.
- Never: set `Connection`/`Transfer-Encoding` manually.

## Open Questions

- The route is not a live stream; `ResponseBodyTimeoutLayer` is near a no-op here because the page is materialised in memory before streaming ([[061-http-middleware-stack/spec|061]]).
- `BatchFrameStream` is wired into production only by this one route; [[035-streaming-module-unreachable/spec|035]] described a zero-caller state that no longer holds for `BatchFrameStream`.
- Negotiation is implemented with `headers-accept` + `mediatype` plus a pre-sanitiser, so [[036-accept-header-negotiation-crate/spec|036]] is realised; the sanitiser still hand-parses `q`.
- Query-string `format` selector: `StreamParams.format` is exported but unused (see [[060-http-rest-api/spec|060]]); `PjsExtension` negotiates separately with substring matching ([[064-http-axum-extension/spec|064]]).
- Batch size is fixed at 1 in the route; the `Binary` array format has no consumer.

## See Also

- [[015-batch-stream-ndjson-format/spec|015]], [[025-streaming-response-content-type-helper/spec|025]], `docs/architecture/SPECIFICATION.md`
