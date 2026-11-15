---
aliases:
  - Real-time Metrics and Observability
  - Spec 009
tags:
  - sdd
  - spec
  - http
  - observability
created: 2026-04-28
status: partial
priority: P2
issue: "#142"
pr: "#169"
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
  - "[[007-http-auth-middleware/spec|007 auth]]"
code:
  - crates/pjs-core/src/infrastructure/http/metrics.rs
  - crates/pjs-core/src/application/handlers/query_handlers.rs
  - crates/pjs-core/src/application/handlers/command_handlers.rs
  - crates/pjs-core/src/infrastructure/http/axum_adapter.rs
  - crates/pjs-core/Cargo.toml
verified: 2026-11-15
---

# Spec 009: Real-time Metrics and Observability

> [!info] Verification (2026-11-15)
> **State: partial.** Real uptime and a feature-gated Prometheus endpoint exist; rolling rates, most metrics, and Tokio runtime metrics do not.
>
> Evidence: issues #139 and #142 closed 2026-04-28. Uptime fix: PR #146 (`4093718`), `SystemQueryHandler::{new, with_start_time}` store `Instant` and compute `started_at.elapsed().as_secs().max(1)`. Prometheus: PR #169 (`e5f5fa2`), feature `metrics = ["http-server", "dep:metrics", "dep:metrics-exporter-prometheus"]` (workspace `metrics = "0.24"`, `metrics-exporter-prometheus = "0.18"`); `GET /metrics` mounted in `public_routes` (unauthenticated) in `axum_adapter.rs`; `infrastructure/http/metrics.rs` has `install_global_recorder()` (idempotent `OnceLock`) and `metrics_handler`. `GET /pjs/stats` is routed to `get_system_stats`.
>
> Drift from original spec:
> - `frames_per_second`/`bytes_per_second` are lifetime averages (`total / uptime`), not rolling 1-minute rates (FR-2 open).
> - Only one counter exists: `pjs_frames_total` (no `session_id` label), incremented in `GenerateFramesCommand` and `BatchGenerateFramesCommand` handlers under `cfg(feature = "metrics")`; WebSocket frames are not counted (#239 comment in code). `pjs_bytes_total`, `pjs_active_sessions` (in the `/metrics` registry), `pjs_uptime_seconds`, and `pjs_http_request_duration_seconds` are not emitted by the `metrics` facade. (`infrastructure/adapters/metrics_collector.rs` renders a separate `pjs_active_sessions` text format that is not wired to `/metrics`.)
> - The global recorder is installed lazily by the first `/metrics` request (no non-test caller installs it at startup), so `metrics::counter!` calls before the first scrape are dropped (`rg install_global_recorder crates` shows only `metrics.rs` and one test).
> - No `axum-prometheus`, no `tokio-metrics`/`tokio_unstable` support.
> - `PjsAppState` does not hold `start_time`; `SystemQueryHandler` does (`started_at`).
> - `/metrics` is outside auth by design; its recorder-failure body is a fixed generic message (CHANGELOG security fix).

**Type**: enhancement | **Priority**: P2 | **Status**: partial

## Functional requirements

| FR | Requirement | Status |
|----|-------------|--------|
| 1 | `uptime_seconds` is real elapsed time | done (floored to 1 s) |
| 2 | Rolling 1-minute frame/byte rates | open (lifetime average) |
| 3a | `GET /metrics` Prometheus text endpoint | done, behind `metrics` feature; `Content-Type: text/plain; version=0.0.4; charset=utf-8` |
| 3b | `pjs_uptime_seconds` | open |
| 3c | `pjs_frames_total{session_id}` | partial: unlabelled aggregate |
| 3d | `pjs_bytes_total{session_id}` | open |
| 3e | `pjs_active_sessions` | open (on `/metrics`) |
| 3f | `pjs_http_request_duration_seconds{method,path,status}` | open |
| 4 | Tokio runtime metrics with `--cfg tokio_unstable` | open |

## Non-functional requirements

- Overhead below 1% of request latency: [NEEDS CLARIFICATION: not measured].
- Disabled by default: met (`metrics` not in `default`).
- No external service dependency: met (pull-based Prometheus only).
- axum 0.8 / tokio 1: met.

## Open questions

- Install the recorder at router construction so early counters are not lost.
- Decide on per-session labels (cardinality risk with unbounded session ids).
- Rolling-window rates vs delegating to `rate()` in Prometheus; `/pjs/stats` still needs an in-process answer.
- `/metrics` is unauthenticated and not rate limited (documented in `metrics.rs`).

## See Also

- [[007-http-auth-middleware/spec|007]]
- `crates/pjs-core/src/infrastructure/http/axum_adapter.rs` test `test_metrics_endpoint_returns_prometheus_format`
