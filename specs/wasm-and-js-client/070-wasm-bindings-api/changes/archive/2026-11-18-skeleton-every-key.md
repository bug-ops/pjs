---
tags:
  - sdd
  - delta
created: 2026-11-18
status: merged
related:
  - "[[070-wasm-bindings-api/spec|spec]]"
---

# Delta: skeleton-every-key

Merged into [[070-wasm-bindings-api/spec|spec]] on 2026-11-18 (#698).

## MODIFIED
- FR-005 step 1: the skeleton keeps no value verbatim by key; every key recurses and nulls leaves like any other. Fields are extracted for every key (steps 2-3), and `JsonPath` quoted segments address keys with `.`, `[`, `]` or the empty key. The `JsonPath::is_valid_key` reference is removed.
- FR-006 `PriorityAssigner` receives paths for all keys; no field is skipped.
