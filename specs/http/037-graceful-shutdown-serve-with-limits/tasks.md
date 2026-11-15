---
aliases:
  - Graceful Shutdown Tasks
tags:
  - sdd
  - tasks
  - http
  - websocket
created: 2026-11-15
status: implemented
issue: "#590"
related:
  - "[[037-graceful-shutdown-serve-with-limits/spec|spec]]"
  - "[[037-graceful-shutdown-serve-with-limits/plan|plan]]"
verified: 2026-11-15
---

# Tasks: Graceful Shutdown for serve_with_limits

> [!info] Status (2026-11-15)
> All tasks done in #590, #618 and #611 (see spec deviations DV1, DV2). T3 added `tokio-util` only; T6 and T7 use the shared signal instead of `child()`; T8 wires Ctrl-C in all demos (WebSocket `with_shutdown` in `interactive-demo` only).

- [x] T1 Resolve spec open questions (terminal frame format, `DrainTimeout` default and cap, signature break, dropped-trigger semantics, standalone WebSocket shutdown, `tokio-util` vs `watch` + `JoinSet`)
- [x] T2 Add `infrastructure/http/shutdown.rs`: `ShutdownSignal`, `DrainTimeout` (`TryFrom<Duration>`, error enum), `ShutdownConfig`, `ShutdownOutcome` with unit tests and `///` docs with doc-tests (FR-006, FR-010)
- [x] T3 Add dependencies after verifying current versions: `tokio-util` (if approved) and hyper-util feature `server-graceful`; check `cargo deny`
- [x] T4 Modify `serve_with_limits`: race semaphore acquire and `accept_with_retry` against the signal; wrap each connection with `GracefulShutdown::watch`; keep `max_connection_duration` and `header_read_timeout` active while draining (FR-001, FR-002, FR-007, FR-009)
- [x] T5 Drain phase: wait bounded by `DrainTimeout`, abort remaining tracked tasks, return `ShutdownOutcome` (FR-004, FR-005, NFR-REL-001)
- [x] T6 Insert `ShutdownSignal` as a per-connection request extension next to `ConnectInfo` (FR-008)
- [x] T7 `AxumWebSocketTransport`: accept a clone of the shared `ShutdownSignal` (`with_shutdown`), make `handle_socket` select on it, send Close 1001, register tasks in the shared tracker (FR-003)
- [x] T8 Update `pjs-demo` servers (`interactive_demo.rs`, `simple_demo.rs`, `websocket_streaming.rs`) to the new signature and wire Ctrl-C
- [x] T9 Tests: new `crates/pjs-core/tests/http_graceful_shutdown.rs` (idle drain, in-flight NDJSON completes, stalled client forced, post-signal connect not served, pre-fired signal, WebSocket close 1001, zero leaked tasks, `tokio::time::pause` for deadlines); update `http_serve_connection_limits.rs` call sites
- [x] T10 Benchmark: `cargo bench -p pjs-bench` accept-path delta below 2% (NFR-PERF-001)
- [x] T11 Docs and changelog: rewrite `serve_with_limits` rustdoc (remove "runs until the process is torn down"), CHANGELOG `[Unreleased]` breaking entry with PR link
- [x] T12 Run the full check suite (`fmt`, `clippy -D warnings`, `nextest`, doc-tests, rustdoc gate)
