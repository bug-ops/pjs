---
aliases:
  - pjs-demo
  - demo servers and datasets
tags:
  - sdd
  - spec
  - demo
created: 2026-11-15
status: implemented
priority: P3
related:
  - "[[028-pjs-demo-workspace-build/spec]]"
  - "[[029-websocket-stream-session-binding/spec]]"
  - "[[078-ci-pipeline/spec]]"
issue: "#406"
code:
  - crates/pjs-demo/Cargo.toml
  - crates/pjs-demo/src/lib.rs
  - crates/pjs-demo/src/servers
  - crates/pjs-demo/src/data
  - crates/pjs-demo/src/utils/mod.rs
  - crates/pjs-demo/src/bin/websocket_streaming_server.rs
  - crates/pjs-demo/tests/ws_upgrade.rs
  - crates/pjs-demo/tests/ws_pause_resume.rs
verified: 2026-11-15
---

# Spec 077: pjs-demo servers, datasets and tests

Part of [[MOC-specs]]; see [[constitution]].

> [!info] Verification (2026-11-15)
> Implemented. `pjs-demo` is a workspace member (not in `default-members`), contrary to the root `CLAUDE.md` and `.claude/rules/continuous-improvement.md`, which say it is excluded and must be run through `--manifest-path`; the build history is in [[028-pjs-demo-workspace-build/spec]] (CHANGELOG #318). Open demo bugs: #609, #610, #611.

## Overview

**Problem.** Users and maintainers need runnable servers that show progressive priority streaming versus a traditional full-payload response, and a test bed for the WebSocket transport.

**Goal.** Three binaries, four synthetic datasets, shared utilities, two integration test files.

**Out of scope.** Production serving (`pjs-core` HTTP adapter), the TypeScript client.

## Functional Requirements

### FR-001 Binaries
The crate shall define three binaries (`Cargo.toml`): `interactive-demo-server` (`src/servers/interactive_demo.rs`), `simple-demo-server` (`src/servers/simple_demo.rs`), `websocket-streaming-server` (`src/bin/websocket_streaming_server.rs`, logic in `servers::websocket_streaming`, exposed from the library for tests). The crate declares no features; pjson-rs features are passed as `pjson-rs/<feature>`.
Status: done.

### FR-002 interactive-demo-server
It shall serve `GET /` (HTML page `static/performance_comparison.html`), `/traditional`, `/pjs-streaming`, `/performance` (query `dataset_type`, `dataset_size`, `network_type`) and `/api/info`, with CLI flags `--port` (default 3000), `--host` (default 127.0.0.1) and `--verbose`, using `serve_with_limits(ConnectionLimits::default(), ShutdownConfig)`. The server binds `(host, port)`, mounts `create_websocket_router()` at `/ws` (allowing its own origins), and `/api/info` lists only routes that exist.
Status: done (#611).

### FR-010 Graceful shutdown (DV2 of [[037-graceful-shutdown-serve-with-limits/spec|037]])
In all three demo servers Ctrl-C shall trigger the `ShutdownSignal` passed to `serve_with_limits` (`utils::trigger_on_ctrl_c`), so the process exits after the HTTP drain. Only `interactive-demo-server`, which mounts `AxumWebSocketTransport`, also passes the same signal to `with_shutdown`, so its `/ws` clients receive Close 1001. `websocket-streaming-server` keeps its own upgrade handler, which is not drained.
Status: done (#611).

### FR-003 simple-demo-server
It shall serve `/`, `/traditional`, `/pjs`, `/performance`, `/api/info` (query `size`, `latency`), flag `--port` (default 3000). The `/pjs` endpoint returns a skeleton from `pjson_rs::PriorityStreamer::analyze` (#399).
Status: done.

### FR-004 websocket-streaming-server
It shall listen on the fixed address `127.0.0.1:3001` and serve `GET /`, `POST /stream` (create a session from `{data_type, config?}`, response `{session_id, websocket_url, config}`), `GET /ws/{session_id}` (WebSocket upgrade), `GET /health`, `GET /metrics`. It runs with `max_connection_duration = None`.
Code: `servers/websocket_streaming.rs` (`app`, `run`).
Status: done.

### FR-005 Session binding
`/ws/{session_id}` shall return `400` for a malformed id, `404` for an unknown id, `409` when another connection already claimed the session (atomic `try_bind_session`), and upgrade otherwise; sessions are single-owner.
Status: done (#382, [[029-websocket-stream-session-binding/spec]]); tests `tests/ws_upgrade.rs`.

### FR-006 Pause/resume protocol
The server shall parse incoming text frames as `ClientCommand` (`{"command":"pause"}` / `{"command":"resume"}`, `deny_unknown_fields`, struct variants so extras are rejected), deliver them to the frame-sending task through a `tokio::sync::watch::channel(StreamControl { paused })`, and block frame and completion delivery while paused. Malformed or unknown commands are logged and ignored; a resume without a pause is a no-op.
Status: done (#406, #428); tests `tests/ws_pause_resume.rs` (5 tests).

### FR-007 Per-stream configuration
`StreamConfig` (`priority_threshold`, `enable_compression`, `compression_strategy`, `frame_delay_ms`, `max_frames`; defaults 50 / true / "adaptive" / 10 / 1000) shall be accepted and echoed.
`priority_threshold` filters frames, `max_frames` caps them, `frame_delay_ms` (bounded) paces them and `enable_compression` selects compression; omitted fields echo the defaults, and an unknown field, a zero `max_frames` or an over-limit delay is rejected.
Status: done (#609); tests `tests/ws_streaming_config_acceptance.rs`.

### FR-008 Metrics
`/metrics` shall report `active_sessions`, `total_frames_sent`, `total_bytes_sent`, `average_session_age_seconds`, `server_uptime_seconds`, `timestamp`, `active_sessions`, `average_session_age_seconds` come from live sessions; `total_frames_sent` and `total_bytes_sent` are lifetime counters that survive session end.
Status: done (#610).

### FR-009 Dataset generation
The library shall generate synthetic JSON by `DatasetType` (`ECommerce`, `SocialMedia`, `Analytics`, `RealTime`) and `DatasetSize` (`Small` 10 items; `Medium` 50/25/30; `Large` 200/100/150; `Huge` 1000/500/750 for e-commerce/social/others), parseable from strings (`ecommerce|e|store`, `social|s|feed`, `analytics|a|dashboard`, `realtime|r|live`; `small|s`, `medium|m`, `large|l`, `huge|h`), via `generate_dataset` and `generate_metadata` (type, size, bytes, size_kb, generated_at, items). `RealTime` reuses `generate_realtime_data` from `analytics.rs`.
Code: `src/data/{mod,ecommerce,social,analytics,realtime}.rs`.
Status: done.

### FR-010 Utilities
`utils` shall provide `NetworkType` (`Fiber` 5 ms, `Cable` 20, `Wifi` 50, `Mobile4G` 100, `Mobile3G` 300, `Satellite` 600) with `simulate_network_latency`, `PerfTimer`, `format_bytes`, `format_duration`.
Status: done.

## Non-Functional Requirements

- NFR-DOC-001: documented run commands use `cargo run -p pjs-demo --bin <name> --features "pjson-rs/..."` or `--manifest-path crates/pjs-demo/Cargo.toml` (the latter still works as the crate is in the workspace).
- NFR-TEST-001: both integration test files must pass (`cargo nextest run -p pjs-demo`).

## Data Model

`AppState { active_sessions: Arc<Mutex<HashMap<SessionId, SessionInfo>>>, compressor: Arc<SchemaCompressor>, server_started_at }`; `SessionInfo { started_at, bound, frame_count, total_bytes }`; `SessionBind { Bound, AlreadyBound, NotFound }`; `StreamControl { paused: bool }` (a struct so later control fields cannot silently un-pause a `watch` latest-wins channel).

## Edge Cases and Errors

| Condition | Response |
|-----------|----------|
| `/ws/{id}` malformed | 400 |
| unknown id | 404 |
| already bound | 409 |
| `{"command":"pause","extra":true}` | rejected, logged, stream continues |
| `["pause"]` | rejected |
| unknown dataset string | `Err("Unknown dataset type: ...")` from `FromStr` |

## Success Criteria

1. Each binary starts and answers its health/index route.
2. `tests/ws_upgrade.rs` and `tests/ws_pause_resume.rs` pass.

## Agent Boundaries

- Always: keep the demo out of `default-members`; keep routes in the HTML page and banners in sync with the router.
- Ask first: turning the demo into a supported example crate.
- Never: use demo code as a hardening reference (no auth, fixed bind addresses).

## Open Questions

- Demo integration tests are not run in PR CI: the nextest archive uses `--lib --bins`, the coverage job excludes `pjs-demo`, and no job runs `cargo nextest -p pjs-demo`; they run only in the release `test-check` (`--all-targets`). The comment in `ci.yml` above the coverage job ("zero tests") is stale. See [[078-ci-pipeline/spec]].
- `src/clients/websocket_demo.rs` (442 lines) is not referenced by `lib.rs`, `Cargo.toml` or any module, so it is not compiled (orphan file).
- Banners use emoji in console output; `data::mod` leaves `pub use realtime::*` commented as TODO.
- `.claude/rules/continuous-improvement.md` lists `ws://localhost:3000/pjs/ws/{session_id}`, which no demo binary serves (`interactive-demo-server` serves `/ws`).

## See Also

[[plan]] | [[028-pjs-demo-workspace-build/spec]] | [[029-websocket-stream-session-binding/spec]] | `crates/pjs-core/src/infrastructure/http/serve.rs` (`serve_with_limits`)
