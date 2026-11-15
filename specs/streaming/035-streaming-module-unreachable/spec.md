---
aliases:
  - Streaming Module Unreachable
  - AdaptiveFrameStream Dead Code
tags:
  - sdd
  - spec
  - investigation
  - http
  - architecture
created: 2026-08-18
status: implemented
priority: P2
issue: "#511"
pr: "#512"
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
  - "[[036-accept-header-negotiation-crate/spec|036 Accept negotiation crate]]"
code:
  - crates/pjs-core/src/infrastructure/http/streaming.rs
  - crates/pjs-core/src/infrastructure/http/handlers/streams.rs
  - crates/pjs-core/src/infrastructure/http/axum_adapter.rs
  - crates/pjs-core/tests/http_axum_endpoints.rs
  - crates/pjs-core/tests/http_streaming_comprehensive.rs
verified: 2026-11-15
---

# Feature: Resolve `infrastructure::http::streaming` Module Unreachability

> [!info] Verification (2026-11-15)
> **State: implemented** (resolution path (a), wire-up).
>
> **Evidence**
> - Issue #511 is closed; PR #512 (commit `1d8085a`, `feat(http)!: wire infrastructure::http::streaming into a live route`) delivered it. CHANGELOG records the new route (Added) and the `from_accept_header` fix (Fixed, #518 follow-up).
> - `axum_adapter.rs` registers `GET /pjs/sessions/{session_id}/streams/{stream_id}/frames/stream` with `get(stream_stream_frames::<R, P, S>)`. The handler lives in `handlers/streams.rs` (`pub(crate) async fn stream_stream_frames`): parses ids, validates `priority`, runs the same `GetStreamFramesQuery` as `get_stream_frames`, negotiates a `StreamFormat` from `Accept` (`Binary` normalized to `Json`), builds `BatchFrameStream::new(stream::iter(frames), format, 1)`, and sets `X-Total-Count`.
> - Tests: 12 integration tests `test_stream_frames_*` in `crates/pjs-core/tests/http_axum_endpoints.rs` (default NDJSON, shape parity with the buffered route, SSE, NDJSON, octet-stream fallback, per-frame chunking, no handler-set `Transfer-Encoding`, q-value preference, `X-Total-Count`, 404, invalid priority, empty page); unit coverage in `http_streaming_comprehensive.rs`.
> - `.claude/rules/continuous-improvement.md` Interfaces section now lists the route (cites #511).
>
> **Drift from original spec**
> - `AdaptiveFrameStream` and `PriorityFrameStream` no longer exist: PR #521 (commit `3eb708b`) removed them as dead code (only `BatchFrameStream` is wired). `infrastructure/http/mod.rs` re-exports `BatchFrameStream`, `StreamFormat`, `StreamTransportError`, `create_streaming_response`, `create_streaming_response_with_content_type`. FR-004's "or `AdaptiveFrameStream`, or `PriorityFrameStream`" clause is therefore moot, and the spec's "follow-up for unused `AdaptiveFrameStream`/`PriorityFrameStream`" was resolved by removal.
> - `StreamFormat::from_accept_header` was later re-implemented on top of the `headers-accept` crate (PR #520, see [[036-accept-header-negotiation-crate/spec|036]]); FR-007's "first-occurrence tie-break" description is obsolete, specificity now breaks ties.
> - The route is finite, not a live tail: `stream_stream_frames` fully materializes one page (bounded by the query handler's pagination cap) before streaming; `ResponseBodyTimeoutLayer` in `apply_common_layers` is therefore close to a no-op on it (documented in `axum_adapter.rs`).
> - 1080-test and 92-test counts in the original resolution note are historical and were not re-run.
> - The criterion benchmark follow-up exists: `crates/pjs-bench/benches/http_streaming.rs`.
> - References to `.local/plans/...` and `.local/handoff/...` in the source spec are not tracked in git and are dropped.

## 1. Overview

### Problem Statement (historical)

`infrastructure::http::streaming` (`AdaptiveFrameStream`, `BatchFrameStream`, `PriorityFrameStream`, `StreamFormat`, `create_streaming_response*`) had no production caller; the real frames endpoint `get_stream_frames` returned axum `Json<FramesResponse>`. PR #510 swapped `serde_json` for `sonic_rs` inside that dead module, so the claimed throughput win never reached live traffic.

### Goal

Either wire the module into a reachable route or remove it. The maintainer chose wire-up.

### Out of Scope

- Redesign of the HTTP API beyond frame streaming; WebSocket transport.

## 2. User Stories

- US-001: a maintainer can choose between wire-up and removal from the recorded evidence (done, wire-up chosen).
- US-002: an implementor delivers a genuinely reachable route (done, covered by integration tests).
- US-003: removal path (not selected; retained as history only).

## 3. Functional Requirements

| ID | Requirement | Priority | Status |
|----|------------|----------|--------|
| FR-001 | THE SYSTEM SHALL have exactly one of wire-up or removal applied | must | done (wire-up) |
| FR-002 | THE SYSTEM SHALL correct the stale `#163` cross-reference in `.claude/rules/continuous-improvement.md` | must | done |
| FR-003 | THE SYSTEM SHALL update `CHANGELOG.md` for the outcome | must | done |
| FR-004 | IF wire-up THEN expose at least one axum route that returns a response built via `create_streaming_response` / `create_streaming_response_with_content_type` | must | done (`BatchFrameStream`, `batch_size = 1`) |
| FR-005 | IF wire-up THEN resolve prior incomplete-behaviour issues #226 and #161 on this path | must | done (verified not applicable on this route) |
| FR-006 | IF wire-up THEN add end-to-end integration tests | must | done (12 tests) |
| FR-007 | IF wire-up THEN define content negotiation and document it on the handler | must | done (`Accept` only; permissive fallback to NDJSON; no `406`; no query override) |
| FR-008..011 | removal path | must (alt.) | superseded (not selected) |

## 4. Non-Functional Requirements

| ID | Category | Requirement | Status |
|----|----------|-------------|--------|
| NFR-001 | Correctness | checks green | done in CI |
| NFR-002 | Docs accuracy | no stale endpoint or performance claims | done for the route; `continuous-improvement.md` corrected |
| NFR-003 | Architecture | handler stays transport-only; no domain logic duplicated | done (handler reuses `GetStreamFramesQuery`) |
| NFR-004 | Traceability | issue with one severity label | done (#511) |

## 5. Data Model

| Entity | Description |
|--------|-------------|
| `StreamFormat` | `Json`, `NdJson`, `ServerSentEvents`, `Binary`; `content_type()` maps to `application/json`, `application/x-ndjson`, `text/event-stream`, `application/octet-stream` |
| `BatchFrameStream<S>` | wraps a frame stream with a `StreamFormat` and `batch_size`; `into_stream()` yields `Vec<u8>` chunks |
| `StreamTransportError` | `Serialization(sonic_rs::Error)`, `Io(String)`, `BufferOverflow`, `StreamClosed` |

## 6. Edge Cases and Error Handling

| Scenario | Behaviour |
|----------|-----------|
| Missing, `*/*`, `application/json`, `application/octet-stream`, unknown `Accept` | NDJSON-of-objects, `Content-Type: application/x-ndjson` |
| `Accept: text/event-stream` | SSE (`data: <json>\n\n` per frame) |
| Unknown session/stream, bad ids | same errors as the buffered route (404 / 400 via `PjsError`) |
| Invalid `priority` query | `PjsError::InvalidPriority` |
| Serialization failure while building the response | logged; generic `PjsError::HttpError("failed to build streaming response")` |
| Browser `EventSource` | finite stream makes `EventSource` redial; callers must `.close()` after the last event (route doc) |

## 7. Success Criteria

| ID | Metric | Target | Status |
|----|--------|--------|--------|
| SC-001 | module reachable from a real HTTP request | integration test | done |
| SC-002 | check suite | green | done in CI |
| SC-003 | no stale `#163` claim | zero | done |
| SC-004 | traceability | issue closed by the PR | done (#511 closed) |

## 8. Agent Boundaries

**Always**: run the full check suite; keep `http_streaming_comprehensive.rs` and the route tests; update `[Unreleased]`.
**Ask first**: changing `StreamTransportError`'s public shape; adding a query-parameter format override or `406` behaviour.
**Never**: claim performance wins for code not on a live path; leave a module compiled but unreachable.

## 9. Open Questions

- A live-tail (subscription) endpoint needs a `FrameSourceGat`-backed port with a production implementation; none exists. [NEEDS CLARIFICATION: not tracked]
- `StreamFormat::Binary` has no real binary wire format (`format_batch_owned` serializes a JSON array); the route normalizes it away.

## 10. See Also

- [[constitution]], [[MOC-specs]], [[036-accept-header-negotiation-crate/spec|036]], [[063-http-streaming-negotiation/spec|063 HTTP streaming and negotiation (current behaviour)]]
- `docs/architecture/SPECIFICATION.md`
- Issues #511, #515, #518; PRs #510, #512, #520, #521
