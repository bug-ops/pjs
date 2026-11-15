---
aliases:
  - PjsWebSocketClient
  - WebSocket client
tags:
  - sdd
  - spec
  - websocket
  - client
created: 2026-11-15
status: partial
priority: P2
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
  - "[[067-websocket-wire-protocol/spec|WebSocket wire protocol]]"
  - "[[066-websocket-server-transport/spec|WebSocket server transport]]"
  - "[[069-transport-features-and-limits/spec|Features and limits]]"
code:
  - crates/pjs-core/src/infrastructure/websocket/client.rs
  - crates/pjs-core/src/infrastructure/websocket/mod.rs
  - crates/pjs-core/tests/websocket_wire_level.rs
verified: 2026-11-15
---

# `PjsWebSocketClient`: client sessions, caps and TTL

> [!info] Status
> `partial`: the client works against the in-tree server with session limit, idle TTL and explicit close. `connect` is documented as blocking for the connection lifetime, keeps both halves inside its future, reports write failures and can be called again after it returns (#597, #612); `websocket-client` builds on its own (#598). Remaining gap: no reconnect and no server-side session close notification.

## Overview

**Problem.** A Rust consumer of a PJS WebSocket stream must reconstruct data from frames without unbounded memory from an untrusted server.

**Goal.** `PjsWebSocketClient` connects, sends `StreamInit`, applies frames into per-session JSON, acknowledges them and exposes the result.

**Out of scope.** Reconnect, TLS configuration beyond `tokio-tungstenite` defaults, server-side session close notification, `http-client` (no such feature exists, see [[069-transport-features-and-limits/spec|069]]).

Feature gate: `websocket-client` (deps `tokio-tungstenite`, `url`); builds without `http-server` (#598).

## User Stories

- As a consumer I call `request_stream(data, options)`, `connect()`, then read `get_current_data` and `is_stream_complete`.
- As an operator I bound session count, idle time and per-session bytes.

## Functional Requirements

### FR-001 Construction and config

`PjsWebSocketClient::new(url)` shall parse the URL (`PjsError::InvalidUrl(url::ParseError)` on failure). `with_config(WebSocketClientConfig)` and `with_write_timeout(Duration)` are `#[must_use]` builders. Defaults:

| Field | Default |
|---|---|
| `max_message_size` | 1 MiB (`DEFAULT_MAX_INBOUND_FRAME_SIZE`) |
| `max_frame_size` | 1 MiB |
| `max_sessions` (`NonZeroUsize`) | 1024 |
| `session_idle_ttl` (`SessionIdleTtl`, always positive) | 300 s |
| `max_session_bytes` (`NonZeroUsize`, serialized JSON size) | 16 MiB |
| write timeout | 10 s (`WRITE_TIMEOUT`), unvalidated |

The message/frame caps equal the server's inbound cap so the client does not sever a connection to the in-tree server, which splits a payload into frames of at most the requested `max_frame_size` (default 64 KiB; one top-level entry above it travels alone) and never sends a message above the 16 MiB outgoing budget. `SessionIdleTtl::new(Duration::ZERO)` returns `ConfigError::MustBePositive`.

### FR-002 Connect

`connect()` shall open the socket with `connect_async_with_config` using the caps, lease the outgoing receiver (a concurrent second call fails with `Error::AlreadyConnected`), then drive the send loop (bounded `byte_bounded_channel`, 1000 messages / 16 MiB, writes through `send_with_write_timeout`) and the receive loop inside the `connect` future, returning when either ends. It blocks for the connection lifetime, which the rustdoc states; nothing outlives the future, the lease is released on return, and a later `connect` sends messages queued in between (#597, #612). A write failure or timeout is returned as an error. Handshake failure maps to `PjsError::ConnectionFailed { url (redacted), source: tungstenite::Error }`; a read or write transport error (including an oversize message or frame) ends `connect` with `Error::Transport`; a write deadline gives `Error::WriteTimeout { timeout }`.

### FR-003 Request stream

`request_stream(data, options)` shall generate a fresh `SessionId`, build `StreamInit` (default `StreamOptions` when `None`), reserve a session slot, queue the serialised message, and return the id. A guard (`SessionSlotGuard`) removes the slot if the call is cancelled before queueing. Errors: `Serialization` (serialise failure), `ChannelClosed`, `SessionLimitReached { limit }`.

### FR-004 Session limit

When `max_sessions` is reached a new request shall first drop expired sessions, then evict the incomplete session idle longest; completed sessions are never evicted, so a full set of completed sessions yields `SessionLimitReached` until `close_session` frees one.

### FR-005 Frame application

On `StreamFrame` the client shall drop the session if expired; if the session exists and is incomplete, merge the payload (object payloads merge key by key; any other payload replaces the data) provided the projected size stays within `max_session_bytes` (otherwise the session is removed and an error returned); mark complete on `is_complete`; refresh `last_activity`; record per-frame processing time; and queue a best-effort `FrameAck` (`try_send`, dropped with a warning if the channel is full). Frames for unknown, expired, closed or completed sessions are ignored without an ack.

### FR-006 Completion and error messages

`StreamComplete` marks the session complete. A server `Error` with a `session_id` removes that session. `Ping` is answered with `Pong` (same timestamp). Completed sessions are frozen: later frames do not refresh them, so they expire after the idle TTL.

### FR-007 Reading and closing

`get_current_data(id)` returns `Ok(Option<Value>)`; `is_stream_complete(id)`, `get_stream_stats(id)` return `false`/`None` for unknown or expired sessions; `close_session(id)` removes the session and returns `ClosedSession { data, stats }` (or `None` if unknown or expired). The server is not told and may keep streaming. `StreamStats`: `session_id`, `total_frames`, `is_complete`, `duration`, `average_processing_time`.

## Non-Functional Requirements

- NFR-SEC-001: retained session data is bounded by `max_sessions * max_session_bytes` (serialized JSON bytes) plus 16 MiB of queued outgoing bytes.
- NFR-SEC-002: inbound caps enforced by the transport (`WebSocketConfig`).
- NFR-REL-001: a stalled write ends the send task after `write_timeout`.
- NFR-TEST-001: 30 unit tests in `client.rs`; wire tests `test_client_max_message_size_cuts_oversize_message`, `..._max_frame_size_cuts_oversize_frame_below_message_cap`, `..._within_caps_message_is_accepted` in `tests/websocket_wire_level.rs`.

## Data Model

`PjsWebSocketClient { url, config, sessions, message_tx, message_rx, write_timeout }`, `WebSocketClientConfig` (`Copy`), `SessionIdleTtl`, `StreamStats`, `ClosedSession`, private `ClientStreamSession`.

## Edge Cases and Errors

| Case | Behaviour |
|---|---|
| Invalid URL | `InvalidUrl` |
| Handshake fails | `ConnectionFailed { url, source }` |
| Concurrent second `connect` | `AlreadyConnected` |
| `connect` after the first returned | allowed; queued messages are sent |
| Session over `max_session_bytes` | session dropped, `LimitExceeded { kind: SessionBytes }` |
| All sessions complete at limit | `SessionLimitReached` |
| Expired session read | treated as unknown |
| Binary / raw frames | logged, ignored |
| Server-supplied text in logs (#628) | error text is escaped and cut to 256 chars; a parse failure logs category/line/column/byte count only; unhandled messages log the variant name; the connect URL is logged without userinfo, query and fragment |

## Success Criteria

- A request against the in-tree server yields complete data, `is_stream_complete == true`, and `close_session` returns the reconstructed object.
- Oversize server messages terminate the connection rather than buffering.

## Agent Boundaries

- Always: keep memory bounded by config; use `try_send` for control messages from the read loop.
- Ask first: adding reconnect or auto-close semantics.
- Never: await a full outgoing channel inside the receive loop.

## Open Questions

- No reconnect: a caller re-invokes `connect` itself.
- The README describes `http-client` (reqwest) alongside; no such code exists.
- Frame ordering is not enforced (`frame_id` unused when merging); the "reconstruction" is a shallow key merge, not the JSON Pointer reconstruction in the core `stream` module.
- `ClientStreamSession.created_at` and `last_activity` use wall-clock `Instant` (not injectable for tests; tests use `close_session_at`).

## See Also

- [[067-websocket-wire-protocol/spec|067]], [[066-websocket-server-transport/spec|066]], [[069-transport-features-and-limits/spec|069]]
