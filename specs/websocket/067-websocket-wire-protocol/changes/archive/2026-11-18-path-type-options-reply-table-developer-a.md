---
tags:
  - sdd
  - delta
created: 2026-11-18
status: merged
related:
  - "[[067-websocket-wire-protocol/spec|spec]]"
---

# Delta: path-type-options-reply-table

Merged into [[067-websocket-wire-protocol/spec|spec]] on 2026-11-18 (#698, #700, #702, #707).

## MODIFIED
- FR-009 the patch path is the domain `JsonPath`; `WirePath`/`WireSegment` are removed. The wire form is unchanged: a JSON array of segments (string = key, non-negative integer = index; indices decode through `usize::try_from`). `MAX_PATCH_PATH_DEPTH` is still enforced at apply time.
- FR-002 `StreamInit.options`: `max_frame_size` is a validated `MaxFrameSize` (at least 1024 bytes, default 64 KiB; serde via `usize`) and `client_fps` is `Option<NonZeroU32>`. Invalid values fail decoding, so they are answered like any undecodable text.
- FR-006 the `Invalid StreamOptions` row is removed: invalid options (`max_frame_size` < 1024, `client_fps` 0) are an undecodable frame, answered `400` `malformed message` with `session_id` null and no session (breaking for peers that relied on the old reply and a session id).
- FR-006 the error table is one closed set of replies (code + fixed text), each addressable from the exact `(code, text)` pair; the wire bytes are unchanged.
- `WsMessage`, `WirePatch`, `Framing` and `StreamOptions` are `#[non_exhaustive]`: out-of-crate code builds options through `StreamOptions::default()` (Merge, the wire default) or `StreamOptions::patch()` (Patch) and public-field assignment.
