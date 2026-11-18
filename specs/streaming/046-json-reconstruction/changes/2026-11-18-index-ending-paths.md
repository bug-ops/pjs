---
tags:
  - sdd
  - delta
created: 2026-11-18
status: proposed
related:
  - "[[046-json-reconstruction/spec|spec]]"
---

# Delta: index-ending-paths

Proposed for [[046-json-reconstruction/spec|spec]] (#719, #725).

## MODIFIED
- FR-004 `Set` delegates to shared `json_tree::set`: paths ending in an index overwrite an existing element; `index >= len` is `IndexOutOfBounds`; state is unchanged on error.
- FR-006 `Remove` delegates to `json_tree::remove`: missing object key is an idempotent `Ok`; index removal rejects `index >= len`; consistent in outcome with JS FR-009.
- FR-009 `PatchError::NotAnIndex` and `NotAContainer` are removed (breaking, approved by the user).
- Edge cases: "path ending in an index" resolved; `Remove` out-of-range index is now an error.
