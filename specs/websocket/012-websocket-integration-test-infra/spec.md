---
aliases:
  - WebSocket Wire-Level Tests
tags:
  - sdd
  - spec
  - websocket
  - testing
created: 2026-04-28
status: implemented
priority: P2
issue: "#158"
pr: "#165"
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
code:
  - crates/pjs-core/tests/websocket_wire_level.rs
  - crates/pjs-core/Cargo.toml
  - crates/pjs-core/src/infrastructure/websocket/server.rs
verified: 2026-11-15
---

# Feature: WebSocket Integration Test Infrastructure

> [!info] Verification (2026-11-15)
> **State: implemented**, and the harness has grown well past the original baseline.
>
> **Evidence**
> - Issue #158 CLOSED (2026-04-28); PR #165 (`bb6ece1`, "add wire-level integration tests"). Defects it was meant to catch: #147, #149, #122 all CLOSED.
> - `crates/pjs-core/tests/websocket_wire_level.rs` (~1100 lines, ~30 `#[tokio::test]`): binds `127.0.0.1:0`, real HTTP upgrade via `tokio_tungstenite::connect_async`, `spawn_ws_test_server[_with]`, `connect`, `stream_init`, `next_ws_message`, `expect_pong`, `assert_silent`.
> - `Cargo.toml`: `[[test]] name = "websocket_wire_level"` with `required-features = ["http-server", "websocket-client"]`.
> - Later additions (git log of the file): rate limiting (#252), frame-size/write-timeout bounds (#364), supervised streaming tasks (#366), WS client caps (#568), frame routing to session owners (#577).
>
> **Drift from original spec**
> - Helpers are `spawn_ws_test_server`/`connect`, not `spawn_test_server`/`connect_ws`; the router is `create_websocket_router()` served at `/ws` (not the demo router at `/pjs/ws/{session}`); the session id travels in the `StreamInit` message.
> - Gating is `http-server` + `websocket-client` (the spec said `websocket-server`).
> - `axum-test` was not adopted; `tokio-tungstenite` is used directly.
> - Ping test uses a protocol-level `Message::Ping`, not a `WsMessage` ping; close test asserts `active_connection_count() == 0` after `Message::Close`.

## Overview

Struct-level WebSocket tests missed wire-level defects (silent `send_frame`, no streaming on `StreamInit`, no-op `close_stream`). Goal: a harness that exercises a real upgraded connection against the axum router.

## User Stories

- As a developer, I want a real-connection test for `StreamInit` -> frames.
- As a CI maintainer, I want WS regressions caught on every PR.

## Functional Requirements

### FR-001: Harness binding the router on a random port
Status: done. Code: `spawn_ws_test_server_with(AxumWebSocketTransport)`.

### FR-002: Helper utilities
Status: done (names differ, see drift).

### FR-003: Baseline cases
Status: done. `test_wire_upgrade_handshake`, `test_wire_protocol_ping_pong`, `test_wire_stream_init_yields_frame`, `test_wire_clean_close`; plus malformed JSON, nil/duplicate session id (400/409), ownership routing, rate limit, frame-size boundaries, stalled-write timeout, client caps.

### FR-004: Feature gating
Status: done via `required-features` in `Cargo.toml` and `#![cfg(all(feature = "http-server", feature = "websocket-client"))]`.

### FR-005: Runs under nextest with no external services
Status: done (loopback only).

## Non-Functional Requirements

- NFR-001: isolation via port 0 per test. Status: done.
- NFR-002: every `recv` wrapped in `tokio::time::timeout` (5 s). Status: done in the helpers.

## Success Criteria

- `cargo nextest run -p pjson-rs --all-features` includes and passes `websocket_wire_level`.

## Agent Boundaries

- Always: add a wire-level test with any change to `infrastructure/websocket/`.
- Never: use fixed ports.

## Open Questions

- Original Rust tests exercise `AxumWebSocketTransport` only; the demo server's WS path (`/pjs/ws/{session_id}`) is not covered here. [NEEDS CLARIFICATION]

## See Also

- [[029-websocket-stream-session-binding/spec|029 WebSocket stream session binding]]
- `crates/pjs-core/tests/websocket_server_comprehensive.rs` (struct-level tests)
