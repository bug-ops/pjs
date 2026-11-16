---
tags:
  - sdd
  - delta
created: 2026-11-16
status: merged
related:
  - "[[070-wasm-bindings-api/spec|spec]]"
---

# Delta: non-object-root-set

Merged into [[070-wasm-bindings-api/spec|spec]] on 2026-11-16 (#666, #667, #668).

## MODIFIED
- FR-005 non-object roots emit one CRITICAL root `set` frame exempt from `min_priority`; the value is NOT truncated at `max_depth` (truncating a root `set` would corrupt the data); nesting stays bounded by the `serde_json` parse recursion cap and `maxJsonSize`.
- Edge Cases: nesting row split into public (rejected) and internal (object roots truncated) behaviour; array/scalar root row.
