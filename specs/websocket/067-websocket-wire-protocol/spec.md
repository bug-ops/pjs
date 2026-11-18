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
  - crates/pjs-core/src/infrastructure/websocket/protocol.rs
  - crates/pjs-core/src/infrastructure/websocket/planner.rs
  - crates/pjs-core/src/infrastructure/websocket/server.rs
  - crates/pjs-domain/src/serde_support.rs
  - fuzz/fuzz_targets/ws_message_decode.rs
verified: 2026-11-18
---

# WebSocket wire protocol (`WsMessage`)

> [!info] Scope
> Exact JSON framing used on the `/ws` transport as implemented in `pjson-rs`. It is distinct from the draft PJS wire protocol in `docs/architecture/SPECIFICATION.md` and from the demo server's own `ClientCommand` protocol (`crates/pjs-demo`).

## Overview

**Problem.** Server, Rust client and third-party clients need one precise message schema.

**Goal.** A single serde enum `WsMessage`, adjacently tagged, sent as WebSocket text frames (one JSON document per frame).

**Out of scope.** Binary frames (ignored), compression negotiation, heuristic priorities for unmapped keys.

## User Stories

- As a client author I can build and parse every message from this document.
- As a maintainer I get fuzz coverage of decode/encode round-trips.

## Functional Requirements

### FR-001 Envelope

Every message shall be `{"type": "<Variant>", "data": { ...fields... }}` (`#[serde(tag = "type", content = "data")]`). Messages are UTF-8 JSON in WebSocket text frames; binary frames are not part of the protocol.

### FR-002 Message table

| `type` | Direction | `data` fields | Notes |
|---|---|---|---|
| `StreamInit` | client -> server | `session_id` (UUID string), `data` (any JSON), `options` (`StreamOptions`) | Server adopts the id; invalid `options` are rejected (400) |
| `StreamFrame` | server -> client | `session_id`, `frame_id` (u32), `priority` (`Priority`, u8 on the wire), `payload` (any JSON), `is_complete` (bool) | Routed only to the owner; merge framing |
| `StreamPatch` | server -> client | `session_id`, `frame_id` (u32), `priority`, `patches` (array of `WirePatch`, FR-009), `is_complete` (bool) | Only for sessions with `framing: "patch"` |
| `FrameAck` | client -> server | `session_id`, `frame_id` (u32), `processing_time_ms` (u64) | Owner only; feeds adaptive delay |
| `StreamComplete` | server -> client | `session_id` | Sent after the last frame; the session is already reusable |
| `StreamCancel` | client -> server | `session_id` | Ends the sender's own session; an unknown id is ignored |
| `Error` | both | `session_id` (UUID string or `null`, may be omitted), `error` (string), `code` (u16) | Server uses 400, 408, 409, 413 and 429 |
| `Ping` | both | `timestamp` (u64) | Both sides reply `Pong` with the same value |
| `Pong` | both | `timestamp` (u64) | Logged and ignored |

`StreamOptions` (`#[non_exhaustive]`, as are `WsMessage`, `WirePatch` and `Framing`; out-of-crate code builds options through `StreamOptions::default()` (Merge, the wire default) or `StreamOptions::patch()` (Patch) and public-field assignment): `max_frame_size` (validated `MaxFrameSize`, at least 1024, default 65536, serde through `usize`), `client_fps` (`Option<NonZeroU32>`: positive integer or null), `priority_mapping` (object string -> u8, or null), `framing` (`"merge"` or `"patch"`, default merge). `max_frame_size` is the only required option on decode (no serde default); unknown keys, including the legacy `compression`, are ignored; `client_fps`, `priority_mapping` and `framing` may be omitted. `framing` is serialised lowercase and omitted on the wire when merge, so a merge `StreamInit` carries no `framing` key; an absent field decodes as merge and an unknown value is a decode error. Compatibility is one-way: servers of this release still accept the legacy `compression` key, but clients of this release omit it and older servers, which require it, reject their `StreamInit` as malformed (400), so upgrade servers first.

The server validates the options and applies them in the planner ([[066-websocket-server-transport/spec|066]] FR-011): `max_frame_size` is the size limit of the whole message, envelope included (minimum 1024); `client_fps` must not be 0 and sets a pacing floor; `priority_mapping` assigns priorities by top-level key. Invalid values (`max_frame_size` below 1024, `client_fps` 0, a `priority_mapping` value that is not a valid priority, a `framing` other than `merge`/`patch`) fail `WsMessage` decoding and are answered `Error` 400 with a fixed text naming the first invalid field in declaration order, `session_id` null and no session (#768; #700 had made them `malformed message`). Only that first field is named, and the rejected value is never echoed.

Example:

```json
{"type":"StreamInit","data":{"session_id":"0b9f6f0e-2f4c-4f0e-9d0c-6a3d1f6a9b11","data":{"critical":{"id":1}},"options":{"max_frame_size":65536,"client_fps":null,"priority_mapping":null}}}
```

### FR-003 Session id encoding

`session_id` shall be a UUID string (`pjson_rs_domain::serde_support::session_id`, option variant for `Error`). Peers may send any textual UUID form; the server always echoes the canonical lowercase hyphenated form. The nil UUID is rejected on `StreamInit`. Session ids are chosen by the peer and scoped to its connection (two connections may reuse one id).

### FR-004 Completion

`StreamComplete` shall carry only `session_id`. The former `checksum` field was removed (#601) because no peer verified it; decoding a message that still carries it ignores the extra field.

### FR-005 Frame plan

For a `StreamInit` the server shall emit the planned frames in priority order (highest first, stable), each holding one priority, with `frame_id` counting from 0 and `is_complete: true` only on the last, followed by `StreamComplete`. Under merge framing every frame is a `StreamFrame` whose `payload` holds top-level entries to shallow-merge (an object root is split by key; a scalar, array or `{}` root is one frame). Under patch framing every frame is a `StreamPatch` and the session receives nothing but `StreamPatch` messages then `StreamComplete`. Priority of a frame is `priority_mapping[top-level key]`, else `Priority::HIGH`. Planning limits and pacing are in [[066-websocket-server-transport/spec|066]] FR-007 and FR-011.

**Breaking change.** With default options an object payload above about 64 KiB used to arrive as one frame; it now arrives in several `StreamFrame`s and clients must shallow-merge top-level keys.

### FR-006 Error replies

| Condition | `code` | `error` text | `session_id` |
|---|---|---|---|
| Undecodable text frame (including a document nested beyond the decoder limit) | 400 | `malformed message` | `null` |
| `StreamInit` with an invalid `max_frame_size` (< 1024) | 400 | `invalid stream option: max_frame_size must be at least 1024` | `null` |
| `StreamInit` with an invalid `client_fps` (not a positive integer) | 400 | `invalid stream option: client_fps must be a positive integer` | `null` |
| `StreamInit` with an invalid `priority_mapping` value | 400 | `invalid stream option: priority_mapping values must be valid priorities` | `null` |
| `StreamInit` with an invalid `framing` | 400 | `invalid stream option: framing must be "merge" or "patch"` | `null` |
| Nil session id | 400 | `nil session id is not allowed` | `null` |
| Session id already in use on this connection | 409 | `session id already in use` | the requested id |
| Plan over the connection plan budget | 429 | `connection plan budget exhausted` | the requested id |
| Plan too large, too many frames or paths over 128 | 413 | `stream plan too large` | the requested id |
| Over the per-connection live-session cap (default 64) | 429 | `too many live sessions on this connection` | the requested id |
| Frame above the 16 MiB outgoing byte budget (at `StreamInit` or while streaming) | 413 | `frame exceeds the outgoing byte budget` | the session id; no session at `StreamInit`, otherwise it ends |
| One enqueue waited over 30 s for channel room | 408 | `stream stalled` | the session id; best-effort, session removed, no `StreamComplete` |
| Stream past `120 s + plan_bytes / 64 KiB/s` | 408 | `stream deadline exceeded` | the session id; best-effort, session removed, no `StreamComplete` |

The replies are one closed set (code plus fixed text, `ServerReply`), each recognisable from its exact `(code, text)` pair; the wire bytes are unchanged (#702). Texts are fixed constants; decode errors are never echoed. Client-originated `Error` messages are logged by length only.

### FR-007 Close codes

The server shall send close code `1008` (policy violation) with the limiter's error text when the per-IP connection cap is hit or an inbound message exceeds the application size check or message rate. Transport-level oversize frames are rejected by the WebSocket implementation. Clean client close ends the session set owned by that connection. On server shutdown every open connection receives close code `1001` ([[037-graceful-shutdown-serve-with-limits/spec|037]]).

### FR-008 Unhandled variants

On the server, the server-to-client kinds `StreamFrame`, `StreamPatch`, `StreamComplete` and `Pong` received from a client are ignored with a debug log and no reply (connection stays open); the match is exhaustive, so a new variant must be classified; on the client, `StreamInit` and `FrameAck` received from a server are logged as unhandled.

### FR-009 Patch grammar

A `StreamPatch.patches` entry (`WirePatch`, defined in `pjson_rs::stream::patch` and re-exported from the websocket module) shall be a 3-element array `[op, path, arg]`; the same type is `JsonPatch.patch` in the stream layer (#751), and the stream reconstructor and the WebSocket client apply it through one shared core:

| `op` | `arg` | Effect at `path` |
|---|---|---|
| `set` | any JSON value | replaces the value; path `[]` replaces the whole document; a missing key in a parent object is inserted; a parent array needs an existing index |
| `merge` | object | shallow insert or replace of each entry into the object at `path` |
| `append` | array | elements appended to the array at `path` |
| `append_str` | string | appended to the string at `path` |

`path` is the domain `JsonPath` (`WirePath`/`WireSegment` were removed in #698); its wire form is an array of segments: a JSON string addresses an object key, a JSON integer an array index (decoded through `usize::try_from`). Any key (including `.`, `[`, `]` and `""`) is therefore expressible and `"0"` differs from `0`. A path may have at most `MAX_PATCH_PATH_DEPTH` = 128 segments: decoding does not check the length, a receiver refuses a longer path when it applies the patch (`PatchError::PathTooDeep`) and drops the stream, and the server never plans one. Wrong arity, unknown op, wrong argument kind, wrong segment kind, and negative or non-integer indexes are decode errors. The server ignores a client-sent `StreamPatch` (FR-008).

Example: `["merge",[],{"a":1}]`, `["append",["items"],[1,2]]`, `["set",["users",0,"name"],"x"]`.

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
| Missing `options` or `max_frame_size` | decode failure |
| Absent `framing` | merge |
| Unknown `framing` value | decode failure |
| Malformed patch (arity, op, argument or segment kind, bad index) | decode failure |
| Path over 128 segments | decodes; refused when applied (`PathTooDeep`) |
| `session_id` not a UUID | decode failure |
| Duplicate `frame_id` | counted again by the client (`total_frames`) |

## Success Criteria

- A third-party client built from FR-002 can complete a stream against the in-tree server.

## Agent Boundaries

- Always: add new variants additively and extend the fuzz target.
- Ask first: renaming tags or changing numeric widths (breaking for all peers).
- Never: echo peer-controlled strings in `Error` replies.

## Open Questions

- Heuristic priorities for keys without a `priority_mapping` entry are not computed (`Priority::HIGH`).
- `timestamp` semantics ("wall clock") are not specified; the server does not generate pings.
- No protocol version or capability negotiation field.
- Client-side `StreamPatch` handling is specified by the client change ([[068-websocket-client/spec|068]]), not here.

## See Also

- [[066-websocket-server-transport/spec|066]], [[068-websocket-client/spec|068]], `docs/architecture/SPECIFICATION.md`
