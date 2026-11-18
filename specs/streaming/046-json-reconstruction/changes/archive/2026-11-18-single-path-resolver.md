---
tags:
  - sdd
  - delta
created: 2026-11-18
status: merged
related:
  - "[[046-json-reconstruction/spec|spec]]"
---

# Delta: single-path-resolver

Merged into [[046-json-reconstruction/spec|spec]] on 2026-11-18 (#699, #698).

## MODIFIED
- FR-003 set, append and remove resolve their target through one crate-private resolver over `&[PathSegment]` (`resolve_mut`; read-only `resolve` for the WebSocket client), shared with the WebSocket protocol and client; behavior is unchanged.
- FR-009 `PatchError::UnsupportedSegment` is removed: `PathSegment` is closed, so the arm is unreachable.
