---
tags:
  - sdd
  - delta
created: 2026-11-18
status: merged
related:
  - "[[073-js-client-core/spec|spec]]"
---

# Delta: websocket-protocol-alignment

Merged into [[073-js-client-core/spec|spec]] on 2026-11-18 (#731).

## MODIFIED
- FR-011 `WebSocketTransport`: speaks the shipped `WsMessage` protocol on `/ws` with patch framing (`StreamInit`, `StreamPatch`, `StreamComplete`, `StreamCancel`, `Error`); requires `StreamOptions.data`; one stream at a time; no automatic reconnection; stale-socket events ignored. Status: works against a stock `pjs-core` server (#731); HTTP and SSE stay experimental.
- FR-009: root `append` pushes onto an array root; new `append_str` operation (`PATCH_OPERATIONS` is the single operation list used by the type and `FrameProcessor`).
- Open Questions: WebSocket endpoint compatibility resolved; the line about the inert nested workflow is removed (the file was deleted).

## ADDED
- Internal `formatJsonPath`, the inverse of `tokenizeJsonPath`.
- `JsonValue` type and `StreamOptions.data`.
- Cross-stack suite `tests/cross-stack/` run against the compiled bundle and `examples/js_client_ws_fixture.rs`.
