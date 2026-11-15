---
aliases:
  - WebSocket wire protocol
  - WsMessage format
tags:
  - sdd
  - spec
  - websocket
  - protocol
created: 2026-11-15
status: implemented
priority: P2
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
  - "[[066-websocket-server-transport/spec|WebSocket server transport]]"
  - "[[068-websocket-client/spec|WebSocket client]]"
code:
  - crates/pjs-core/src/infrastructure/websocket/mod.rs
  - crates/pjs-core/src/infrastructure/websocket/server.rs
  - crates/pjs-domain/src/serde_support.rs
  - fuzz/fuzz_targets/ws_message_decode.rs
verified: 2026-11-15
---

# WebSocket wire protocol (`WsMessage`)

> [!info] Scope
> Exact JSON framing used on the `/ws` transport as implemented in `pjson-rs`. It is distinct from the draft PJS wire protocol in `docs/architecture/SPECIFICATION.md` and from the demo server's own `ClientCommand` protocol (`crates/pjs-demo`).

## Overview

**Problem.** Server, Rust client and third-party clients need one precise message schema.

**Goal.** A single serde enum `WsMessage`, adjacently tagged, sent as WebSocket text frames (one JSON document per frame).

**Out of scope.** Binary frames (ignored), compression negotiation, priority ordering and multi-frame plans (planner tracked in #624).

## User Stories

- As a client author I can build and parse every message from this document.
- As a maintainer I get fuzz coverage of decode/encode round-trips.

## Functional Requirements

### FR-001 Envelope

Every message shall be `{"type": "<Variant>", "data": { ...fields... }}` (`#[serde(tag = "type", content = "data")]`). Messages are UTF-8 JSON in WebSocket text frames; binary frames are not part of the protocol.

### FR-002 Message table

| `type` | Direction | `data` fields | Notes |
|---|---|---|---|
| `StreamInit` | client -> server | `session_id` (UUID string), `data` (any JSON), `options` (`StreamOptions`) | Server adopts the id; non-default `options` are rejected (400) |
| `StreamFrame` | server -> client | `session_id`, `frame_id` (u32), `priority` (`Priority`, u8 on the wire), `payload` (any JSON), `is_complete` (bool) | Routed only to the owner |
| `FrameAck` | client -> server | `session_id`, `frame_id` (u32), `processing_time_ms` (u64) | Owner only; feeds adaptive delay |
| `StreamComplete` | server -> client | `session_id` | Sent after the last frame; the session is already reusable |
| `StreamCancel` | client -> server | `session_id` | Ends the sender's own session; an unknown id is ignored |
| `Error` | both | `session_id` (UUID string or `null`, may be omitted), `error` (string), `code` (u16) | Server uses 400, 409, 413 and 429 |
| `Ping` | both | `timestamp` (u64) | Both sides reply `Pong` with the same value |
| `Pong` | both | `timestamp` (u64) | Logged and ignored |

`StreamOptions`: `max_frame_size` (usize, default 65536), `client_fps` (u32 or null), `compression` (bool, default true), `priority_mapping` (object string -> u8, or null). `max_frame_size` and `compression` are required on decode (no serde defaults); `client_fps` and `priority_mapping` may be omitted (`Option`). The server accepts only `StreamOptions::default()` and answers anything else with `Error` 400 (`unsupported stream options`) and no session; the options are not otherwise applied (real planner: #624).

Example:

```json
{"type":"StreamInit","data":{"session_id":"0b9f6f0e-2f4c-4f0e-9d0c-6a3d1f6a9b11","data":{"critical":{"id":1}},"options":{"max_frame_size":65536,"client_fps":null,"compression":false,"priority_mapping":null}}}
```

### FR-003 Session id encoding

`session_id` shall be a UUID string (`pjson_rs_domain::serde_support::session_id`, option variant for `Error`). Peers may send any textual UUID form; the server always echoes the canonical lowercase hyphenated form. The nil UUID is rejected on `StreamInit`. Session ids are chosen by the peer and scoped to its connection (two connections may reuse one id).

### FR-004 Completion

`StreamComplete` shall carry only `session_id`. The former `checksum` field was removed (#601) because no peer verified it; decoding a message that still carries it ignores the extra field.

### FR-005 Frame plan (current)

For a `StreamInit` the server shall emit exactly one `StreamFrame { frame_id: 0, priority: Priority::HIGH, payload: <StreamInit.data>, is_complete: true }` followed by `StreamComplete`. The reply frame therefore echoes the whole request data and may reach the inbound cap.

### FR-006 Error replies

| Condition | `code` | `error` text | `session_id` |
|---|---|---|---|
| Undecodable text frame | 400 | `malformed message` | `null` |
| Nil session id | 400 | `nil session id is not allowed` | `null` |
| Session id already in use on this connection | 409 | `session id already in use` | the requested id |
| Non-default `StreamOptions` | 400 | `unsupported stream options` | the requested id |
| Over the per-connection live-session cap (default 64) | 429 | `too many live sessions on this connection` | the requested id |
| Frame above the 16 MiB outgoing byte budget | 413 | `frame exceeds the outgoing byte budget` | the session id; the session ends |

Texts are fixed constants; decode errors are never echoed. Client-originated `Error` messages are logged by length only.

### FR-007 Close codes

The server shall send close code `1008` (policy violation) with the limiter's error text when the per-IP connection cap is hit or an inbound message exceeds the application size check or message rate. Transport-level oversize frames are rejected by the WebSocket implementation. Clean client close ends the session set owned by that connection. On server shutdown every open connection receives close code `1001` ([[037-graceful-shutdown-serve-with-limits/spec|037]]).

### FR-008 Unhandled variants

On the server, `StreamFrame` and `StreamComplete` received from a client are logged as unhandled and ignored, and `Pong` is logged; on the client, `StreamInit` and `FrameAck` received from a server are logged as unhandled.

## Non-Functional Requirements

- NFR-ROB-001: `fuzz/fuzz_targets/ws_message_decode.rs` asserts decode, encode, re-decode equality for arbitrary bytes.
- NFR-SEC-001: inbound text bounded by transport caps (default 1 MiB) and per-IP message rate.
- NFR-TEST-001: `tests/websocket_wire_level.rs`, `tests/serde_wire_format.rs`.

## Data Model

`WsMessage`, `StreamOptions` (see `infrastructure::websocket`); numeric widths are fixed: `frame_id: u32`, `priority: Priority` (u8 on the wire), `code: u16`, `processing_time_ms: u64`, `timestamp: u64`.

## Edge Cases and Errors

| Case | Behaviour |
|---|---|
| Unknown `type` | decode failure, server `Error` 400 |
| Missing `options`, `max_frame_size` or `compression` | decode failure |
| `session_id` not a UUID | decode failure |
| Duplicate `frame_id` | counted again by the client (`total_frames`) |

## Success Criteria

- A third-party client built from FR-002 can complete a stream against the in-tree server.

## Agent Boundaries

- Always: add new variants additively and extend the fuzz target.
- Ask first: renaming tags or changing numeric widths (breaking for all peers).
- Never: echo peer-controlled strings in `Error` replies.

## Open Questions

- #624: a real priority planner honouring `StreamOptions` (non-default options are rejected until then).
- `timestamp` semantics ("wall clock") are not specified; the server does not generate pings.
- No protocol version or capability negotiation field.
- `compression` and `max_frame_size` are advertised but only their default values are accepted.

## See Also

- [[066-websocket-server-transport/spec|066]], [[068-websocket-client/spec|068]], `docs/architecture/SPECIFICATION.md`
