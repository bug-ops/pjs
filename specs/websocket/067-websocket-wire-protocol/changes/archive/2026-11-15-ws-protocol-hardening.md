---
tags:
  - sdd
  - delta
created: 2026-11-15
status: merged
related:
  - "[[067-websocket-wire-protocol/spec|spec]]"
---

# Delta: ws-protocol-hardening

Merged into [[067-websocket-wire-protocol/spec|spec]] on 2026-11-15.

## MODIFIED
- FR-002: `StreamFrame.priority` is a `Priority` (u8 on the wire); `StreamComplete` carries only `session_id`; `Ping` is answered with `Pong` on both sides.
- FR-003: session ids are scoped per connection (#572).
- FR-006: adds 400 for non-default `StreamOptions` (#600), 429 over the 64-session cap (#571) and 413 for a frame above the 16 MiB outgoing budget.
- FR-007, FR-008: close 1001 on shutdown; the server logs `Pong`.

## ADDED
- `StreamCancel` message (#572).

## REMOVED
- FR-004 `checksum` (#601).
- Open question on raw `u8` priority and checksum (#601); the planner is tracked in #624.
