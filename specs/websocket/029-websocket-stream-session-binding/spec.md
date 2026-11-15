---
aliases:
  - WebSocket Session Binding
  - websocket-streaming-server 404 fix
tags:
  - sdd
  - spec
  - pjs-demo
  - websocket
created: 2026-08-17
status: implemented
priority: P2
issue: "#382"
pr: "#386"
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
  - "[[028-pjs-demo-workspace-build/spec|028 pjs-demo workspace build]]"
code:
  - crates/pjs-demo/src/servers/websocket_streaming.rs
  - crates/pjs-demo/src/bin/websocket_streaming_server.rs
  - crates/pjs-demo/tests/ws_upgrade.rs
verified: 2026-11-15
---

# Feature: Fix websocket-streaming-server Session/WebSocket URL Mismatch

> [!info] Verification (2026-11-15)
> **State: implemented** with option (a) (session-scoped route).
>
> **Evidence**
> - `websocket_streaming::app()` registers `.route("/ws/{session_id}", get(ws_upgrade))`; `POST /stream` returns `websocket_url = ws://127.0.0.1:3001/ws/{session_id}`; the startup log advertises the same shape.
> - `ws_upgrade` parses the path id with `SessionId::from_string` (400 on failure), then `AppState::try_bind_session` atomically returns `SessionBind::{Bound, AlreadyBound, NotFound}` (409 / 404 / proceed) in one `Mutex` hold. `handle_websocket(socket, state, session_id)` receives the bound id; it no longer mints `SessionId::new()`.
> - Delivered by PR #386 (commit `fdcb90e`), closing issue #382. CHANGELOG entry cites #382.
> - Tests: `crates/pjs-demo/tests/ws_upgrade.rs` (`valid_session_upgrades_and_streams`, `unknown_session_is_rejected_with_404`, `malformed_session_id_is_rejected_with_400`, `concurrent_connect_to_same_session_is_rejected_with_409`).
>
> **Drift from original spec**
> - Line numbers and the `SessionInfo` shape differ: `SessionInfo` is `{ started_at: Instant, bound: bool, frame_count: usize, total_bytes: u64 }` (the `session_id` field was dropped; the map key is the id).
> - The open question on concurrent connections is resolved: reject with `409 Conflict` (single owner, no fan-out).
> - Router construction extracted to `pub fn app(AppState) -> Router`; `main` became `pub async fn run()` and is reachable through the library target and `src/bin/websocket_streaming_server.rs`.
> - SC-002 (`/metrics` totals attributable to the session) is only partly true: `total_frames_sent` / `total_bytes_sent` are summed over `active_sessions`, so they drop when a session is removed on disconnect. Tracked in open issue #610.
> - Abandoned sessions (created, never connected) still have no TTL or cleanup. [NEEDS CLARIFICATION: no issue found for this gap.]
> - Per-stream config in `POST /stream` is accepted and echoed but not applied (open issue #609).

## 1. Overview

### Problem Statement (historical)

`POST /stream` registered a session via `AppState::add_session` and returned a `websocket_url` of the form `ws://127.0.0.1:3001/ws/{session_id}`, but the router only served the literal `/ws` route and `handle_websocket` generated an unrelated random `SessionId`. Clients following the returned URL received 404; session bookkeeping was decorative.

### Goal

`POST /stream` returns a `websocket_url` that a client can connect to and receive frames for the session just created, with `active_sessions` bookkeeping reflecting that connection.

### Out of Scope

- Other demo servers and the production `pjson-rs` adapters under `crates/pjs-core/src/infrastructure/`.
- Authentication, reconnection or resume.

## 2. User Stories

- US-001: as an integrator, the `websocket_url` from `POST /stream` upgrades (HTTP 101) and streams `pjs_frame` and `stream_complete` messages for that session.
- US-002: as an operator, `GET /metrics` reflects the sessions that are actually streaming.

## 3. Functional Requirements

| ID | Requirement | Priority | Status |
|----|------------|----------|--------|
| FR-001 | WHEN a client sends `POST /stream` THE SYSTEM SHALL return a `websocket_url` that resolves to a registered route | must | done |
| FR-002 | WHEN a client connects to that URL THE SYSTEM SHALL accept the upgrade and stream frames for the created session | must | done |
| FR-003 | WHEN `handle_websocket` runs THE SYSTEM SHALL use the `SessionId` registered by `add_session` | must | done |
| FR-004 | WHEN the server advertises the WebSocket endpoint (response and startup log) THE SYSTEM SHALL advertise only a served shape | must | done |
| FR-005 | IF per-session routing were removed THE SYSTEM SHALL return the bare `/ws` URL | should | superseded (option (a) chosen) |
| FR-006 | WHEN a client connects to an unknown or already-claimed session THE SYSTEM SHALL reject the upgrade | should | done (404 unknown, 409 already bound, 400 malformed) |

## 4. Non-Functional Requirements

| ID | Category | Requirement | Status |
|----|----------|-------------|--------|
| NFR-001 | Correctness | Response URL, startup log and router stay in agreement | done (still two string literals, not derived from one constant) |
| NFR-002 | Consistency | Keep the demo small and readable | done |
| NFR-003 | Compatibility | Pre-v1.0, no deprecation needed | n/a |

## 5. Data Model

| Entity | Description |
|--------|-------------|
| `AppState` | `active_sessions: Arc<Mutex<HashMap<SessionId, SessionInfo>>>`, `compressor: Arc<SchemaCompressor>`, `server_started_at: Instant` |
| `SessionInfo` (private) | `started_at`, `bound`, `frame_count`, `total_bytes` |
| `SessionBind` (private enum) | `Bound`, `AlreadyBound`, `NotFound` |
| `StreamResponse` | `session_id: String`, `websocket_url: String`, `config: StreamConfig` |

`bound: bool` plus map membership encode a three-state lifecycle (created, bound, removed). [NEEDS CLARIFICATION: whether a typestate/enum would be preferred over a boolean; not required for a demo crate.]

## 6. Edge Cases and Error Handling

| Scenario | Behaviour |
|----------|-----------|
| Path id is not a valid `SessionId` | `400 Bad Request` |
| Session never created or already removed | `404 Not Found` |
| Second connection to a bound session | `409 Conflict` |
| Session created, never connected | stays in `active_sessions` forever (no TTL) |
| Concurrent `POST /stream` | independent `SessionId::new()` values, no collision |

## 7. Success Criteria

| ID | Metric | Target | Status |
|----|--------|--------|--------|
| SC-001 | `POST /stream` then connect to returned URL | HTTP 101 and at least one frame | done (`valid_session_upgrades_and_streams`) |
| SC-002 | `/metrics` after a completed session | non-zero totals attributable to the session | not met after disconnect (issue #610) |
| SC-003 | fmt, clippy, nextest | green | done in CI |

## 8. Agent Boundaries

**Always**: run fmt, clippy and nextest; keep startup log and response URL in sync with the router.
**Ask first**: new dependencies in `pjs-demo`; coupling to `pjson-rs` websocket internals; changing the `StreamRequest`/`StreamResponse` JSON shape.
**Never**: edit other demo servers as part of this behaviour; make bookkeeping decorative.

## 9. Open Questions

- Session TTL for abandoned sessions [NEEDS CLARIFICATION: not tracked].
- `/metrics` totals reset on session end (#610); per-stream config not applied (#609).
- `std::sync::Mutex::lock().unwrap()` is used in the state methods (poisoning panics); acceptable for a demo, noted for completeness.

## 10. See Also

- [[constitution]], [[MOC-specs]]
- [[077-demo-servers/spec|077 demo servers]]
- [[028-pjs-demo-workspace-build/spec|028 pjs-demo workspace build]] (made this crate buildable and testable)
- `crates/pjs-demo/src/servers/websocket_streaming.rs`
- Issues #382, #609, #610, #611; PR #386
