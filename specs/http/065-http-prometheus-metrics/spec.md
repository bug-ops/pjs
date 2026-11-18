---
aliases:
  - Prometheus metrics endpoint
tags:
  - sdd
  - spec
  - http
  - metrics
  - observability
created: 2026-11-15
status: partial
priority: P3
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
  - "[[009-realtime-metrics-observability/spec|Real-time metrics and observability]]"
  - "[[060-http-rest-api/spec|HTTP REST API]]"
code:
  - crates/pjs-core/src/infrastructure/http/metrics.rs
  - crates/pjs-core/src/infrastructure/http/axum_adapter.rs
  - crates/pjs-core/src/application/handlers/command_handlers.rs
verified: 2026-11-18
---

# Prometheus `/metrics` endpoint

> [!info] Status
> `partial`: the scrape endpoint, eager recorder install, HTTP request series, state gauges, WebSocket gauge and rate-limit/bytes counters are implemented; `pjs_frames_total` is the only domain metric. Broader observability goals live in [[009-realtime-metrics-observability/spec|009]].

## Overview

**Problem.** Operators need a standard scrape target.

**Goal.** With feature `metrics`, `GET /metrics` renders the process-global Prometheus recorder.

**Out of scope.** Authentication of `/metrics`, `pjs_frames_total` for WebSocket frames.

Feature gate: `metrics` = `http-server` + `metrics` + `metrics-exporter-prometheus`. It also enables `#![feature(once_cell_try)]` in `lib.rs` (nightly).

## User Stories

- As an operator I scrape `/metrics` with Prometheus.
- As an embedder I can call `install_global_recorder()` early to share the recorder.

## Functional Requirements

### FR-001 Route

When `metrics` is enabled, `public_routes` shall register `GET /metrics` -> `metrics_handler::<R, P, S>` (generic over `PjsAppState`); it is unauthenticated and (when rate limiting is configured) still subject to the common stack.

### FR-002 Rendering

Every router builder that mounts `/metrics` shall call `install_global_recorder()` while being built (eager install; a failure is logged once and building continues). `metrics_handler` shall still install the recorder if that failed (idempotent retry), run the recorder upkeep, and return `200` with `Content-Type: text/plain; version=0.0.4; charset=utf-8` and `PrometheusHandle::render()` as the body.

### FR-003 Recorder lifecycle

`install_global_recorder()` shall store the first successful `PrometheusHandle` in a `OnceLock` and return clones thereafter; on failure nothing is cached and the next call retries; the raw error is logged once per process and the caller receives `PjsError::HttpError("failed to install metrics recorder")`.

### FR-006 Request series

A metrics middleware, applied as the last (outermost) layer of every router builder, shall record for each request `http_requests_total{method,route,status}` (counter) and `http_request_duration_seconds{method,route}` (histogram with explicit buckets, no `status` label). The duration covers the time to the response head only, so SSE/NDJSON streaming responses record near-zero latency (known limitation). `route` is the `MatchedPath` template or `unmatched`, never the raw URI. `method` is one of `GET|HEAD|POST|PUT|PATCH|DELETE|OPTIONS|other`. Routes an embedder adds after the builder returns are not measured.

### FR-007 Upkeep

A background task shall run the recorder upkeep every 5 seconds; it starts the first time `install_global_recorder()` is called with a Tokio runtime current (a router built outside a runtime starts it at the latest on the first scrape, and the exporter buffers samples until then); `metrics_handler` also runs it before rendering.

### FR-008 State, WebSocket and transport series

With `metrics`, the following are emitted:

| Series | Type | Source |
|---|---|---|
| `pjs_active_sessions`, `pjs_active_streams` | gauge | set at scrape time from the `GET /pjs/stats` query, refreshed at most once per second per `PjsAppState` (clones share the throttle; a failed refresh releases the slot so the next scrape retries) |
| `pjs_websocket_connections` | gauge | `websocket-server` only; drop guard around each connection loop |
| `pjs_rate_limited_total{transport}` | counter | HTTP `429` from the rate-limit middleware (`http`); inbound WebSocket frame rejected by the message budget (`websocket`) |
| `pjs_bytes_sent_total{transport}` | counter | bytes of each `BatchFrameStream` chunk (`http`, streaming route only, not the JSON frame endpoints); bytes of each WebSocket text message written (`websocket`) |

`transport` is `http` or `websocket`. WebSocket frame production still does not increment `pjs_frames_total`.

### FR-004 Error exposure

On install failure the handler shall respond `500` with the fixed body `metrics recorder unavailable`; the underlying error text never reaches the client (issue #323).

### FR-005 Emitted metrics

`SessionCommandHandler` shall increment counter `pjs_frames_total` by the number of frames after publishing events, in the `GenerateFramesCommand` and `BatchGenerateFramesCommand` handlers (`command_handlers.rs`, under `#[cfg(feature = "metrics")]`). WebSocket frame production does not increment it ("HTTP throughput only", issue #239).

## Non-Functional Requirements

- NFR-SEC-001: unauthenticated; deploy behind a network boundary or scrape-only listener [NEEDS CLARIFICATION: no guidance in code on isolating `/metrics`].
- NFR-TEST-001: unit test `install_is_idempotent` in `metrics.rs`; route-presence tests in `axum_adapter.rs` (`test_metrics_router_has_metrics_route`).

## Data Model

`RECORDER: OnceLock<PrometheusHandle>` (process global); no custom types.

## Edge Cases and Errors

| Case | Behaviour |
|---|---|
| Another recorder already installed globally | install fails, 500 `metrics recorder unavailable`, retried on next call |
| Feature off | route absent (404) |
| Concurrent first calls | one performs the install |

## Success Criteria

- After a generate-frames request, `/metrics` contains `pjs_frames_total`.

## Agent Boundaries

- Always: keep error text server-side only.
- Ask first: adding labels (cardinality) or metrics on request paths.
- Always: every label value comes from a closed or route-table-bounded set.
- Never: add session or client identifiers as metric labels.

## Open Questions

- `GET /pjs/stats` (`SystemStatsResponse`) is the other observability surface and is not Prometheus-backed.
- Building a router claims the process-global recorder: an embedder that installs its own recorder must do so before building the router.
- Gate `once_cell_try` makes the `metrics` feature nightly-only, which is already the project baseline.

## See Also

- [[009-realtime-metrics-observability/spec|009]], [[060-http-rest-api/spec|060]]
