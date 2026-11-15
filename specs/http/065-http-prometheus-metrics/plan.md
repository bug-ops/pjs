---
aliases:
  - Prometheus metrics plan
tags:
  - sdd
  - plan
  - metrics
created: 2026-11-15
status: partial
related:
  - "[[065-http-prometheus-metrics/spec|065 spec]]"
  - "[[constitution]]"
code:
  - crates/pjs-core/src/infrastructure/http/metrics.rs
verified: 2026-11-15
---

# Plan (as-built): Prometheus metrics

## Architecture

```mermaid
flowchart LR
  CH[SessionCommandHandler] -->|metrics::counter pjs_frames_total| REC[(global PrometheusRecorder)]
  SCRAPE[GET /metrics] --> MH[metrics_handler] --> IGR[install_global_recorder] --> REC
  IGR --> OL[OnceLock PrometheusHandle]
```

## Design decisions

- Process-global recorder behind `OnceLock::get_or_try_init` (nightly `once_cell_try`): success cached, failure retried.
- Lazy install inside the handler so a router without a pre-installed recorder still serves `/metrics`; counters emitted before the first install are lost [NEEDS CLARIFICATION: confirm no early-install call in demo or examples].
- Error log guarded by `Once` to avoid ERROR flooding on an unauthenticated route.

## Public API surface

`infrastructure::http::metrics::{install_global_recorder, metrics_handler}` (module `pub`, feature `metrics`).

## Contracts

| Function | Precondition | Postcondition | Undefined / notes |
|---|---|---|---|
| `install_global_recorder()` | none | `Ok(handle)` same recorder each call | Fails if a different global `metrics` recorder is installed |
| `metrics_handler()` | none | 200 text exposition, or 500 fixed text | Never exposes install error text |

## Testing strategy

`install_is_idempotent` (unit); `axum_adapter.rs` route tests under `#[cfg(feature = "metrics")]`.

## Security notes

Unauthenticated by design (public route group); relies on deployment isolation.
