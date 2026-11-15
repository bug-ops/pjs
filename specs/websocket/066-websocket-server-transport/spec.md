---
aliases:
  - WebSocket server transport
  - AxumWebSocketTransport
tags:
  - sdd
  - spec
  - websocket
  - server
  - security
created: 2026-11-15
status: partial
priority: P2
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
  - "[[067-websocket-wire-protocol/spec|WebSocket wire protocol]]"
  - "[[068-websocket-client/spec|WebSocket client]]"
  - "[[062-http-serve-connection-limits/spec|serve_with_limits]]"
  - "[[012-websocket-integration-test-infra/spec|WebSocket test infrastructure]]"
  - "[[029-websocket-stream-session-binding/spec|Demo session/URL binding]]"
  - "[[037-graceful-shutdown-serve-with-limits/spec|Graceful shutdown]]"
code:
  - crates/pjs-core/src/infrastructure/websocket/controller.rs
  - crates/pjs-core/src/infrastructure/websocket/protocol.rs
  - crates/pjs-core/src/infrastructure/websocket/mod.rs
  - crates/pjs-core/src/infrastructure/websocket/server.rs
  - crates/pjs-core/src/infrastructure/websocket/security.rs
  - crates/pjs-core/src/infrastructure/bounded_channel.rs
  - crates/pjs-core/src/security/rate_limit.rs
verified: 2026-11-15
---

# WebSocket server transport: upgrade, ownership, caps and TTL

> [!info] Status
> `partial`: transport hardening (origin check, rate limits, frame caps, write timeout, bounded queues, per-connection session ownership and cleanup) is implemented and covered by wire-level tests. Delivery semantics are minimal: every session is a single-frame plan and only default `StreamOptions` are accepted (real planner: #624). Message formats are in [[067-websocket-wire-protocol/spec|067]].

## Overview

**Problem.** Push streaming over WebSocket must not let one peer observe, ack or exhaust another peer's sessions, nor wedge the server with slow or abusive peers.

**Goal.** `AxumWebSocketTransport` upgrades requests at an axum route, tracks connections, and routes frames only to the owning connection.

**Out of scope.** Priority-ordered multi-frame plans (#624), HTTP session integration (the WebSocket session model is disjoint from `StreamSession`, issue #239).

Feature gates: `websocket-server` (= `http-server`); the server module and `SecureWebSocketHandler` are gated by `websocket-server`, not `http-server` (#598). `websocket-client` adds the client ([[068-websocket-client/spec|068]]).

## User Stories

- As an embedder I mount `create_websocket_router()` with a shared `Arc<AxumWebSocketTransport>`.
- As an operator I deny cross-site browser upgrades by default and cap rates and sizes.
- As a client I only receive frames for sessions I created.

## Functional Requirements

### FR-001 Route

`create_websocket_router()` shall return `Router<Arc<AxumWebSocketTransport>>` with a single `GET /ws` route bound to `AxumWebSocketTransport::upgrade_handler`. The router must be served with `ConnectInfo<SocketAddr>` (via `into_make_service_with_connect_info` or [[062-http-serve-connection-limits/spec|serve_with_limits]]); without it the extractor fails (HTTP 500). The route has no session id segment and no auth layer.

### FR-002 Upgrade admission

On upgrade the handler shall, in order: (1) apply `ClientRateLimiter::check_request(ip)`, rejecting with `429` and the error text; (2) if an `Origin` header is present and not allowed, reject with `403`; (3) configure axum's `max_message_size` and `max_frame_size` from `RateLimitConfig::max_frame_size`; (4) upgrade. A missing `Origin` is always allowed (native clients, not browser CSWSH).

### FR-003 Origin allow-list

`with_allowed_origins(Vec<String>)` shall resolve once into `DenyAll` (empty list, the default; or `*` mixed with explicit), `Any` (`["*"]`; re-enables CSWSH, documented as more dangerous than CORS `Any`) or `Explicit` (case-sensitive byte equality). Entries that cannot match a real origin (no `scheme://`, a path, uppercase) are kept but logged with `warn!`; entries failing `HeaderValue` parsing are dropped; `"null"` is exempt from the shape warning.

### FR-004 Connection lifecycle

`handle_socket(socket, client_ip)` shall: create a `RateLimitGuard` (per-IP concurrent-connection cap; on failure send close code `1008` and return); assign a `ConnectionId` (server-side `Uuid`, never on the wire); register an outgoing channel `byte_bounded_channel<String>(1000 messages, 16 MiB)` with the controller under that id; run one `select!` loop over the outgoing channel, inbound messages and the shutdown signal; on exit detach the connection, abort its sessions' streaming tasks and drop the guard. Frames reach a connection only through its own channel, so there is no shared broadcast. When a `ShutdownSignal` is set via `with_shutdown`, the loop sends close code `1001` and ends, and its task is tracked by the signal ([[037-graceful-shutdown-serve-with-limits/spec|037]]).

### FR-005 Inbound handling

| Inbound | Action |
|---|---|
| Text | `guard.check_message(len)`; on violation send close `1008` and end; parse as `WsMessage`; parse failure answers `Error{session_id: null, code: 400, error: "malformed message"}` and keeps the connection |
| Binary | `check_message(len)` then ignored (debug log only) |
| Ping (protocol) | `Pong` with same payload |
| Pong | ignored |
| `WsMessage::Ping` | `WsMessage::Pong` with the same timestamp |
| `WsMessage::StreamCancel` | ends the connection's own session |
| Close / transport error | end loop |

Message rate: `max_messages_per_second` 30 with burst 5 by default (`RateLimitConfig`). Oversize frames are rejected during frame assembly by the transport cap before the application check.

### FR-006 Session ownership

`StreamInit` shall be handled as: nil UUID answers `Error{400, "nil session id is not allowed"}`; non-default `StreamOptions` answer `Error{400, "unsupported stream options"}` and create no session; otherwise `create_session(session_id, owner = connection, data, options)`. A duplicate id on the same connection answers `Error{session_id, 409, "session id already in use"}` and leaves the existing session untouched; a connection already holding the cap of live sessions (`with_max_sessions_per_connection`, default 64; defence in depth: the default per-IP rate limit (30 msg/s, burst 5) stops a burst at about 35 messages, but a paced client can still reach 64 live sessions, only when its sessions stay live for more than about 2 s (completed sessions stop being live earlier) or the rate limit is raised, #631) answers `Error{session_id, 429, "too many live sessions on this connection"}`. On success the streaming task starts. `FrameAck` and `StreamCancel` act only on a session of the sending connection (unknown ids are ignored). Session ids are peer-chosen and scoped to the connection: two connections may use the same id without interacting (#572, #571). A session stops being live before its `StreamComplete` is queued, so the id can be reused as soon as the peer sees it.

### FR-007 Streaming task

`start_streaming` shall spawn a task that sends each plan frame as `WsMessage::StreamFrame` into the owner's outgoing channel, sleeping `recommended_frame_delay` between frames (10 ms normally; 0.5 x average ack time when average exceeds 100 ms, clamped to 1 s), then sends `StreamComplete` (see 067). A supervising task logs panics; a repeated start is a no-op. A frame that exceeds the 16 MiB outgoing byte budget ends the session and the peer receives `Error{413}`. The plan today is a single frame with `Priority::HIGH` carrying the full `StreamInit.data` (`is_complete: true`); the planner is tracked in #624.

### FR-008 Outgoing path and backpressure

`AxumWebSocketTransport::send_frame(connection, message)` shall serialise once, `try_send` with the byte length against the 16 MiB budget, and return `Ok(())` even when the frame is dropped (full channel, closed channel, over budget, unknown connection), logging a `warn!`. It never blocks, so it is safe to call from the connection's own message-handling path. Every socket write uses `send_with_write_timeout` with `RateLimitConfig::write_timeout` (10 s default, 3 s `low_resource`); a timeout is treated as a send error and the connection ends.

### FR-009 Session TTL sweep

`with_rate_limit_config` shall spawn a task that every 60 s calls `cleanup_expired_sessions(3600 s)`, aborting streaming tasks of sessions older than one hour; it holds only a `Weak<AdaptiveStreamController>` and exits when the controller is dropped. The constructors do not panic outside a Tokio runtime (#602). `AdaptiveStreamController::remove_session(id)` is idempotent and returns whether it removed anything.

### FR-010 Standalone security helper

`SecureWebSocketHandler` (`new`, `with_default_security`, `with_high_traffic_security`, `with_low_resource_security`, `check_upgrade_request`, `create_connection_guard`, `validate_message`, `security_stats`, `force_cleanup`) shall wrap a `ClientRateLimiter` for callers who build their own upgrade handler. It is not used by `AxumWebSocketTransport`, which owns its own limiter.

## Non-Functional Requirements

- NFR-SEC-001: bounded memory per connection: 1000 queued messages and 16 MiB queued bytes (`MAX_QUEUED_OUTGOING_BYTES`), budget held until the write completes (`Envelope::split`).
- NFR-SEC-002: inbound cap default 1 MiB (`DEFAULT_MAX_INBOUND_FRAME_SIZE`); `low_resource` 256 KiB.
- NFR-SEC-003: error replies use fixed text; decode errors are never echoed; client-supplied `Error.error` is logged by length only.
- NFR-REL-001: a stalled peer is disconnected after `write_timeout`.
- NFR-TEST-001: `tests/websocket_wire_level.rs` (real sockets: handshake, close cleanup, malformed 400, nil id, duplicate 409, owner routing, owner-only ack, per-connection cleanup, rate limit, frame-cap boundary exact and plus-one, low-resource cap, stalled write with negative control), `tests/websocket_security.rs`, `tests/websocket_server_comprehensive.rs`, fuzz target `ws_message_decode`.

## Data Model

`ConnectionId(Uuid)`, `WsMessage`, `StreamOptions`, `ClientMetrics` (EMA alpha 0.3), `AdaptiveStreamController` (connections keyed by `ConnectionId`, each holding its outgoing channel and its sessions keyed by `SessionId`), `CreateSessionError { AlreadyExists, TooManySessions }`, `AxumWebSocketTransport`, `SecureWebSocketHandler`. `WebSocketStreamSession` is crate-private and the `WebSocketTransport` trait, `subscribe_frames` and `FrameEvent` were removed (#601, #570).

## Edge Cases and Errors

| Case | Behaviour |
|---|---|
| Upgrade over per-IP request budget | HTTP 429 |
| Disallowed `Origin` | HTTP 403 |
| Per-IP connection cap reached | close frame 1008 |
| Inbound frame over cap | rejected at transport, or close 1008 at application check |
| Malformed text | `Error` 400, connection stays |
| Nil or duplicate session id | `Error` 400 or 409; existing session untouched |
| Non-default `StreamOptions` | `Error` 400, no session |
| Live-session cap reached | `Error` 429 |
| Server shutdown | close frame 1001 |
| Slow/stalled reader | stalls only its own sessions; disconnect after write timeout |
| Connection closes | its sessions and tasks removed |

## Success Criteria

- A second connection never receives, acks or overwrites another connection's session.
- After disconnect `active_connection_count()` and the controller's sessions for that owner drop.

## Agent Boundaries

- Always: route every frame through the owner's channel only; keep replies fixed-text.
- Ask first: changing close codes, defaults, or the wire route.
- Never: let `send_frame` await channel capacity on the connection's own loop (deadlock).

## Open Questions

- #624: single-frame plan; a real priority planner honouring `StreamOptions` is not implemented.
- Rustdoc elsewhere (`serve.rs`, `streams.rs`) names `/pjs/ws/{session_id}`; the crate route is `/ws`, and the demo mounts `/ws/{session_id}` itself ([[029-websocket-stream-session-binding/spec|029]]).

## See Also

- [[067-websocket-wire-protocol/spec|067]], [[068-websocket-client/spec|068]], [[069-transport-features-and-limits/spec|069]], [[012-websocket-integration-test-infra/spec|012]]
