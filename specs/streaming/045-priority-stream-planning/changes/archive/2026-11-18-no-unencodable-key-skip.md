---
tags:
  - sdd
  - delta
created: 2026-11-18
status: merged
related:
  - "[[045-priority-stream-planning/spec|spec]]"
---

# Delta: no-unencodable-key-skip

Merged into [[045-priority-stream-planning/spec|spec]] on 2026-11-18 (#698).

## REMOVED
- FR-004 Unencodable keys: `PriorityStreamer` no longer skips fields whose key `JsonPath` once could not encode, no object case in `needs_root_set`, and no key check in `skeletonize_arrays`.

## MODIFIED
- FR-003 patch extraction addresses every object key; a key that is not plain gets a quoted path segment.
- Boundaries: "drop data for unencodable keys" is vacuous (no key is unencodable); the regression test `test_round_trip_unencodable_key_parent_wipe` becomes a round-trip over the same documents.
