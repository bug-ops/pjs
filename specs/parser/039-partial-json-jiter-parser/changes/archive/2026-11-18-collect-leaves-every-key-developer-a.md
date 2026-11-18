---
tags:
  - sdd
  - delta
created: 2026-11-18
status: merged
related:
  - "[[039-partial-json-jiter-parser/spec|spec]]"
---

# Delta: collect-leaves-every-key

Merged into [[039-partial-json-jiter-parser/spec|spec]] on 2026-11-18 (#698).

## MODIFIED
- `collect_leaves` and the streaming-hint path builder append every key through the infallible `JsonPath::append_key`; the parent-path fallback for keys with `.`, `[`, `]` or the empty key is removed, so hints for such keys no longer collapse onto the parent.
