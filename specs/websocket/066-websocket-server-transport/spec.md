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
status: implemented
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
  - crates/pjs-core/src/infrastructure/websocket/planner.rs
  - crates/pjs-core/src/infrastructure/websocket/mod.rs
  - crates/pjs-core/src/infrastructure/websocket/server.rs
  - crates/pjs-core/src/infrastructure/websocket/security.rs
  - crates/pjs-core/src/infrastructure/bounded_channel.rs
  - crates/pjs-core/src/security/rate_limit.rs
verified: 2026-11-15
---

# WebSocket server transport: upgrade, ownership, caps and TTL

> [!info] Status
> `implemented`: transport hardening (origin check, rate limits, frame caps, write timeout, bounded queues, per-connection session ownership and cleanup) and the stream planner (`planner.rs`: priority-ordered multi-frame plans honouring `StreamOptions`, per-connection plan budget, pacing and stream deadlines; #624) are in place and covered by tests. Message formats are in [[067-websocket-wire-protocol/spec|067]].

## Overview

**Problem.** Push streaming over WebSocket must not let one peer observe, ack or exhaust another peer's sessions, nor wedge the server with slow or abusive peers.

**Goal.** `AxumWebSocketTransport` upgrades requests at an axum route, tracks connections, and routes frames only to the owning connection.

**Out of scope.** Heuristic priorities for keys without a mapping (they get `Priority::HIGH`), HTTP session integration (the WebSocket session model is disjoint from `StreamSession`, issue #239).

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

`StreamInit` shall be handled as: nil UUID answers `Error{400, "nil session id is not allowed"}`; otherwise `create_session(session_id, owner = connection, data, options)`, which validates the options, plans the stream and registers the session. Invalid options (`max_frame_size` below 1024, `client_fps` 0) answer `Error{400, "invalid stream options"}`; a rejected plan answers `Error{429, "connection plan budget exhausted"}` (`PlanError::ConnectionBudgetExhausted`, transient: retry once other sessions finish), `Error{413, "frame exceeds the outgoing byte budget"}` (`UnitExceedsBudget`) or `Error{413, "stream plan too large"}` (`TooManyFrames`, `PlanBytesExceeded`, `PathTooDeep`). No session is created on any of these. A duplicate id on the same connection answers `Error{session_id, 409, "session id already in use"}` and leaves the existing session untouched; a connection already holding the cap of live sessions (`with_max_sessions_per_connection`, default 64; defence in depth: the default per-IP rate limit (30 msg/s, burst 5) stops a burst at about 35 messages, but a paced client can still reach 64 live sessions, only when its sessions stay live for more than about 2 s (completed sessions stop being live earlier) or the rate limit is raised, #631) answers `Error{session_id, 429, "too many live sessions on this connection"}`. On success the streaming task starts. `FrameAck` and `StreamCancel` act only on a session of the sending connection (unknown ids are ignored). Session ids are peer-chosen and scoped to the connection: two connections may use the same id without interacting (#572, #571). A session stops being live before its `StreamComplete` is queued, so the id can be reused as soon as the peer sees it.

### FR-007 Streaming task

`start_streaming` shall spawn a task that sends the session's pre-serialised plan messages (a `StreamFrame` per frame under merge framing, a `StreamPatch` per frame under patch framing) into the owner's outgoing channel in plan order, then sends `StreamComplete` (see 067). A supervising task logs panics; a repeated start is a no-op.

- Delay after each frame: `min(max(adaptive, 1 / client_fps), 30 s / frames)`, where `adaptive` is `recommended_frame_delay` (10 ms normally; 0.5 x average ack time when the average exceeds 100 ms, clamped to 1 s). Total pacing is therefore at most 30 s; `client_fps` is a floor only while the plan has at most `30 * fps` frames.
- A single enqueue that waits longer than 30 s for channel room (`STREAM_STALL_TIMEOUT`) ends the session and sends a best-effort `Error{408, "stream stalled"}`. The timeout is per enqueue, so many sessions sharing one channel on a slow link can see false positives while still progressing; the write timeout (FR-008) is what detects a dead peer.
- The whole stream has an absolute deadline of `120 s + plan_bytes / 64 KiB/s`; exceeding it ends the session with a best-effort `Error{408, "stream deadline exceeded"}`.
- Both 408 paths remove the session and no `StreamComplete` follows. A frame above the 16 MiB outgoing byte budget ends the session with `Error{413}`.

The planner is specified in FR-011.

### FR-008 Outgoing path and backpressure

`AxumWebSocketTransport::send_frame(connection, message)` shall serialise once, `try_send` with the byte length against the 16 MiB budget, and return `Ok(())` even when the frame is dropped (full channel, closed channel, over budget, unknown connection), logging a `warn!`. It never blocks, so it is safe to call from the connection's own message-handling path. Replies to inbound messages (`Error`, application `Pong`) are written by the connection loop directly to the socket with `send_with_write_timeout`, so a full frame queue cannot drop them (#689); a peer that does not read its replies is disconnected after `write_timeout`, like a peer that does not read its frames (NFR-REL-001). `send_frame` stays fire-and-forget for frames and for `handle_message` called outside the loop. Every socket write uses `send_with_write_timeout` with `RateLimitConfig::write_timeout` (10 s default, 3 s `low_resource`); a timeout is treated as a send error and the connection ends.

### FR-009 Session TTL sweep

`with_rate_limit_config` shall spawn a task that every 60 s calls `cleanup_expired_sessions(3600 s)`, aborting streaming tasks of sessions older than one hour (in practice streaming sessions end earlier through the FR-007 deadline, so the sweep is a backstop); it holds only a `Weak<AdaptiveStreamController>` and exits when the controller is dropped. The constructors do not panic outside a Tokio runtime (#602). `AdaptiveStreamController::remove_session(id)` is idempotent and returns whether it removed anything.

### FR-010 Standalone security helper

`SecureWebSocketHandler` (`new`, `with_default_security`, `with_high_traffic_security`, `with_low_resource_security`, `check_upgrade_request`, `create_connection_guard`, `validate_message`, `security_stats`, `force_cleanup`) shall wrap a `ClientRateLimiter` for callers who build their own upgrade handler. It is not used by `AxumWebSocketTransport`, which owns its own limiter.

### FR-011 Stream planner and plan budget

At session creation the server shall plan the stream (`planner.rs`, `plan_stream`) into pre-serialised messages.

- Merge framing: an object root is split by top-level key into frames holding entries of one priority; an entry above the limit travels alone; a scalar, array or `{}` root is one frame.
- Patch framing: an object root becomes `merge([], {...})` batches (no root `set`); `{}` and scalar roots are one `set([], v)`. A value too big for a frame becomes `set(path, empty)` plus `merge`/`append`/`append_str` batches, recursing into children that do not fit. Values nested deeper than the envelope allows (123 levels) are decomposed further; a document needing paths over 128 segments is rejected (`PlanError::PathTooDeep`).
- Priority of a unit is `priority_mapping[top-level key]`, else `Priority::HIGH`. Frames are ordered by priority descending, stable within a priority, and each frame holds one priority.
- Limits: frame limit `L = min(max_frame_size, 16 MiB) - 256` (envelope reserve); at most `MAX_PLAN_FRAMES` = 4096 frames; plan bytes at most `2 * |data| + 4096 * 256`; a single message above the 16 MiB outgoing budget is `PlanError::UnitExceedsBudget`.
- Plan budget: each live session reserves its whole plan size (`unsent_plan_bytes`, summed per connection in `live_plan_bytes`). The reservation is released frame by frame as frames are handed to the outgoing channel, and fully on every removal path (complete, cancel, detach, stall, deadline, sweep). The budget is `max(16 MiB, 2 * inbound max_frame_size + 4096 * 256)` for the transport and 16 MiB for a default controller; a plan that does not fit is `ConnectionBudgetExhausted`. Per-connection memory is bounded by the budget of live plans plus 16 MiB queued.

- Off-thread planning (#658, #669): planning is O(payload) (about 330-370 MiB/s: 45 us at 16 KiB, 90 us at 32 KiB, 2.7 ms at 1 MiB; indicative, measured with the `websocket_controller` bench, baseline in spec 075 FR-008). The offload gate `planner::exceeds_plan_cost` estimates worker time, not size: it sums byte-equivalents of planning time and compares them with `INLINE_PLAN_MAX_BYTES` = 32 KiB (the smallest power of two whose planning reaches about 100 us; a `spawn_blocking` round trip costs 6-10 us; the 16 KiB mixed reference stays inline and the 32 KiB one is offloaded). Strings, keys, containers and separators weigh their exact serialized bytes (escapes counted), `null`/`true` 4, `false` 5, an integer its digits plus `INT_COST` (3), and every float a fixed 7 (`FLOAT_COST`, digits included, never formatted). Every array item and object entry adds `ELEMENT_COST` (3) at any depth, and every entry of an object root adds `ROOT_ENTRY_COST` (38) on top, because the planner frames those entries one by one (#683). A scalar string, `[]` and `{}` cost exactly their size; any other float-free document costs at least its size and a non-empty container strictly more, so `exceeds_size(d, n)` implies `exceeds_plan_cost(d, n)` for float-free `d`. For the calibration shapes of the `websocket_controller` bench (spec 075 FR-008) under default (Merge) framing, the largest document kept inline is estimated to plan no slower than the 32 KiB reference did inline before #683 (indicative: about 90 us on a quiet machine); the 32 KiB mixed reference itself now plans on the blocking pool. Patch framing and shapes outside the bench are not calibrated; the gate remains an estimate, not a bound. When the cost passes the threshold and a Tokio runtime is present, `create_session` plans on the blocking pool so the async worker keeps running; other documents, and calls without a runtime, plan inline. The gate is an iterative early-exit walk, O(threshold) in time and pending stack, stack-safe at any depth; it assumes the default `serde_json::Number`. Results and errors are identical; a planner panic is propagated to the caller.
- Bound on off-thread plans (#671, #673): each controller holds one semaphore (one permit per CPU). A plan on the blocking pool takes a permit first and the permit moves into the blocking task, so it is released only when the plan has finished, also when the caller was dropped meanwhile; further large plans wait for a permit instead of filling Tokio's blocking pool (512 threads). The wait suspends only the calling connection's message loop, as an inline plan did before. A server normally shares one controller across its connections (`create_websocket_router` takes the caller's), so the cap is server-wide. The transient memory of one plan is proportional to the inbound frame cap (plan at most 2x the document plus the envelope reserve); retained plans stay capped by the connection plan budget.
- Pre-plan budget reject (#671): when a connection has at most 32 KiB of plan budget left (`room = budget - live`), `create_session` runs the lower-bound probe `planner::exceeds_size(data, room)` (certain: strings and keys exact with escapes, floats at least 3 bytes) before it takes a permit and refuses a document over `room` with `ConnectionBudgetExhausted { live, requested: room + 1, max }`. Soundness relies on a plan never holding fewer bytes than the document serializes to. Larger rooms plan first and use the exact check under the write lock. `requested` has one meaning: a lower bound on the bytes the new plan needs (its planned size, or `room + 1` when refused before planning). Because the reject runs first, a document that planning would refuse permanently (`413`: `PathTooDeep`, `TooManyFrames`, `PlanBytesExceeded`) on a nearly full connection gets the transient `429` while the room stays at or below 32 KiB; the permanent `413` appears only once the connection has more room, so a retry on a still-full connection gets `429` again.
- Pacing clamp (#663): the total pacing time is deliberately bounded, so the per-frame delay is capped at `30 s / frames` (about 7.3 ms at `MAX_PLAN_FRAMES`). For plans above `30 s / delay` frames the budget wins over both the adaptive delay (up to 1 s for a slow client) and the `client_fps` floor. Back-pressure for such plans comes from the outgoing channel's byte budget and `STREAM_STALL_TIMEOUT`, not from pacing. Kept as a trade-off between total streaming time and per-frame delay; scaling the budget with the plan size or pacing per byte are rejected for now.

**Behaviour change (breaking).** Under merge framing with default options an object payload above about 64 KiB now arrives in several `StreamFrame`s and only the last has `is_complete: true`; clients must shallow-merge top-level keys. Previously the whole payload was one frame.

## Non-Functional Requirements

- NFR-SEC-001: bounded memory per connection: 1000 queued messages and 16 MiB queued bytes (`MAX_QUEUED_OUTGOING_BYTES`), budget held until the write completes (`Envelope::split`); live plans are additionally bounded by the plan budget (FR-011).
- NFR-SEC-002: inbound cap default 1 MiB (`DEFAULT_MAX_INBOUND_FRAME_SIZE`); `low_resource` 256 KiB.
- NFR-SEC-003: error replies use fixed text; decode errors are never echoed; client-supplied `Error.error` is logged by length only.
- NFR-REL-001: a stalled peer is disconnected after `write_timeout`.
- NFR-PERF-001: the per-frame bookkeeping (`release_sent`) takes the controller's global write lock once per sent frame (#664). Measured with `cargo bench -p pjs-bench --bench websocket_controller` (`handle_frame_ack` as proxy: same lock, lookup and O(1) update) on 500 connections: 17.4 M ops/s from 1 task, 11.7 M from 4, 10.3 M from 8 and 10.3 M from 16 tasks (8 worker threads). Sizing rule: sessions supported = 10 % lock utilisation / (2 lock operations per frame x 137 frames/s, the fastest pacing `paced_delay` allows for a 4096-frame plan) = ops/s / 2740, so about 3700 concurrently streaming sessions at 10.3 M ops/s, well above the 1000 threshold. Decision: keep the single lock; no atomics redesign.
- NFR-TEST-001: `tests/websocket_wire_level.rs` (real sockets: handshake, close cleanup, malformed 400, nil id, duplicate 409, owner routing, owner-only ack, per-connection cleanup, rate limit, frame-cap boundary exact and plus-one, low-resource cap, stalled write with negative control), `tests/websocket_security.rs`, `tests/websocket_server_comprehensive.rs`, fuzz target `ws_message_decode`.

## Data Model

`ConnectionId(Uuid)`, `WsMessage`, `StreamOptions`, `ClientMetrics` (EMA alpha 0.3), `AdaptiveStreamController` (connections keyed by `ConnectionId`, each holding its outgoing channel and its sessions keyed by `SessionId`), `CreateSessionError { AlreadyExists, TooManySessions, InvalidOptions(StreamOptionsError), PlanRejected(PlanError) }` (`#[non_exhaustive]`; converts into `Error::SessionAlreadyExists`, `TooManySessions`, `InvalidStreamOptions`, `PlanRejected`), `AxumWebSocketTransport`, `SecureWebSocketHandler`. `WebSocketStreamSession` is crate-private and the `WebSocketTransport` trait, `subscribe_frames` and `FrameEvent` were removed (#601, #570).

## Edge Cases and Errors

| Case | Behaviour |
|---|---|
| Upgrade over per-IP request budget | HTTP 429 |
| Disallowed `Origin` | HTTP 403 |
| Per-IP connection cap reached | close frame 1008 |
| Inbound frame over cap | rejected at transport, or close 1008 at application check |
| Malformed text | `Error` 400, connection stays |
| Nil or duplicate session id | `Error` 400 or 409; existing session untouched |
| Invalid `StreamOptions` | `Error` 400 `invalid stream options`, no session |
| Plan over the connection plan budget | `Error` 429 `connection plan budget exhausted`, no session |
| Plan or frame too large / too deep | `Error` 413 (`stream plan too large` or `frame exceeds the outgoing byte budget`), no session |
| Planned message could not be serialized (server fault, not expected) | `Error` 500 `stream plan could not be built`, no session |
| Live-session cap reached | `Error` 429 `too many live sessions on this connection` |
| Stream stalled or past deadline | best-effort `Error` 408, session removed, no `StreamComplete` |
| Server-to-client message kind sent by a client | ignored, debug log, no reply |
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

- Heuristic priorities for keys without a `priority_mapping` entry are not computed (`Priority::HIGH`).
- Client-side `StreamPatch` handling is specified by the client change ([[068-websocket-client/spec|068]]), not here.
- Rustdoc elsewhere (`serve.rs`, `streams.rs`) names `/pjs/ws/{session_id}`; the crate route is `/ws`, and the demo mounts `/ws/{session_id}` itself ([[029-websocket-stream-session-binding/spec|029]]).

## See Also

- [[067-websocket-wire-protocol/spec|067]], [[068-websocket-client/spec|068]], [[069-transport-features-and-limits/spec|069]], [[012-websocket-integration-test-infra/spec|012]]
