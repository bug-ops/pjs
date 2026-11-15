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
verified: 2026-11-15
---

# Prometheus `/metrics` endpoint

> [!info] Status
> `partial`: the scrape endpoint and recorder lifecycle are implemented; exactly one application metric (`pjs_frames_total`) is emitted. Broader observability goals live in [[009-realtime-metrics-observability/spec|009]].

## Overview

**Problem.** Operators need a standard scrape target.

**Goal.** With feature `metrics`, `GET /metrics` renders the process-global Prometheus recorder.

**Out of scope.** Per-route latency/status metrics, WebSocket metrics, authentication of `/metrics`.

Feature gate: `metrics` = `http-server` + `metrics` + `metrics-exporter-prometheus`. It also enables `#![feature(once_cell_try)]` in `lib.rs` (nightly).

## User Stories

- As an operator I scrape `/metrics` with Prometheus.
- As an embedder I can call `install_global_recorder()` early to share the recorder.

## Functional Requirements

### FR-001 Route

When `metrics` is enabled, `public_routes` shall register `GET /metrics` -> `metrics_handler`; it is unauthenticated and (when rate limiting is configured) still subject to the common stack.

### FR-002 Rendering

`metrics_handler` shall install the global recorder on first use (idempotent) and return `200` with `Content-Type: text/plain; version=0.0.4; charset=utf-8` and `PrometheusHandle::render()` as the body.

### FR-003 Recorder lifecycle

`install_global_recorder()` shall store the first successful `PrometheusHandle` in a `OnceLock` and return clones thereafter; on failure nothing is cached and the next call retries; the raw error is logged once per process and the caller receives `PjsError::HttpError("failed to install metrics recorder")`.

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
- Never: add session or client identifiers as metric labels.

## Open Questions

- Only one counter exists; `GET /pjs/stats` (`SystemStatsResponse`) is the other observability surface and is not Prometheus-backed.
- No HTTP request duration/status metrics, concurrency gauge or rate-limit counters.
- Gate `once_cell_try` makes the `metrics` feature nightly-only, which is already the project baseline.

## See Also

- [[009-realtime-metrics-observability/spec|009]], [[060-http-rest-api/spec|060]]
