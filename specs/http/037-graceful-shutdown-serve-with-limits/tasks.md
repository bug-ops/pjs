---
aliases:
  - Graceful Shutdown Tasks
tags:
  - sdd
  - tasks
  - http
  - websocket
created: 2026-11-15
status: draft
issue: "#590"
related:
  - "[[037-graceful-shutdown-serve-with-limits/spec|spec]]"
  - "[[037-graceful-shutdown-serve-with-limits/plan|plan]]"
verified: 2026-11-15
---

# Tasks: Graceful Shutdown for serve_with_limits

> [!info] Status (2026-11-15)
> Nothing started; issue #590 open. Tasks derive from the [[037-graceful-shutdown-serve-with-limits/plan|plan]]. Decide the open questions in the spec (drain defaults, signature break vs sibling, dropped-trigger semantics, `tokio-util` approval) before T2.

- [ ] T1 Resolve spec open questions (terminal frame format, `DrainTimeout` default and cap, signature break, dropped-trigger semantics, standalone WebSocket shutdown, `tokio-util` vs `watch` + `JoinSet`)
- [ ] T2 Add `infrastructure/http/shutdown.rs`: `ShutdownSignal`, `DrainTimeout` (`TryFrom<Duration>`, error enum), `ShutdownConfig`, `ShutdownOutcome` with unit tests and `///` docs with doc-tests (FR-006, FR-010)
- [ ] T3 Add dependencies after verifying current versions: `tokio-util` (if approved) and hyper-util feature `server-graceful`; check `cargo deny`
- [ ] T4 Modify `serve_with_limits`: race semaphore acquire and `accept_with_retry` against the signal; wrap each connection with `GracefulShutdown::watch`; keep `max_connection_duration` and `header_read_timeout` active while draining (FR-001, FR-002, FR-007, FR-009)
- [ ] T5 Drain phase: wait bounded by `DrainTimeout`, abort remaining tracked tasks, return `ShutdownOutcome` (FR-004, FR-005, NFR-REL-001)
- [ ] T6 Insert `ShutdownSignal` as a per-connection request extension next to `ConnectInfo` (FR-008)
- [ ] T7 `AxumWebSocketTransport`: accept a child `ShutdownSignal` (builder), make `handle_socket` select on it, send Close 1001, register tasks in the shared tracker (FR-003)
- [ ] T8 Update `pjs-demo` servers (`interactive_demo.rs`, `simple_demo.rs`, `websocket_streaming.rs`) to the new signature and wire Ctrl-C
- [ ] T9 Tests: new `crates/pjs-core/tests/http_graceful_shutdown.rs` (idle drain, in-flight NDJSON completes, stalled client forced, post-signal connect not served, pre-fired signal, WebSocket close 1001, zero leaked tasks, `tokio::time::pause` for deadlines); update `http_serve_connection_limits.rs` call sites
- [ ] T10 Benchmark: `cargo bench -p pjs-bench` accept-path delta below 2% (NFR-PERF-001)
- [ ] T11 Docs and changelog: rewrite `serve_with_limits` rustdoc (remove "runs until the process is torn down"), CHANGELOG `[Unreleased]` breaking entry with PR link
- [ ] T12 Run the full check suite (`fmt`, `clippy -D warnings`, `nextest`, doc-tests, rustdoc gate)
