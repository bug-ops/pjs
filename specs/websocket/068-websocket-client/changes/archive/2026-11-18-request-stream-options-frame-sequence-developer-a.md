---
tags:
  - sdd
  - delta
created: 2026-11-18
status: merged
related:
  - "[[068-websocket-client/spec|spec]]"
---

# Delta: request-stream-options-frame-sequence

Merged into [[068-websocket-client/spec|spec]] on 2026-11-18 (#700, #701, #702, #707, #718).

## MODIFIED
- FR-003 `request_stream(data, options: StreamOptions)` sends `options` verbatim; there is no `None` default. `StreamOptions::patch()` requests Patch framing, which this client reconstructs.
- FR-005 a live session applies a frame if and only if its kind matches the requested framing and `frame_id` equals the number of frames applied so far (from 0). Otherwise the session is removed, nothing is applied, no ack is sent and `PjsError::FrameSequence(..)` (`UnexpectedFrameId` or `FramingMismatch`) is returned. Frames for unknown, expired, closed or completed sessions keep today's behavior (ignored, unacked).
- FR-005 merging a `StreamFrame` payload moves or extends the session data without cloning it (#701); behavior is unchanged.
- Error handling: the session survives a `409` reply only for the exact pair `(409, "session id already in use")`; any other `409` text removes it.
- `WebSocketClientConfig` is `#[non_exhaustive]` (construct from `Default` and assign fields).
