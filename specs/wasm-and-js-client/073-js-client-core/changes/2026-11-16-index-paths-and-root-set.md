---
tags:
  - sdd
  - delta
created: 2026-11-16
status: merged
related:
  - "[[073-js-client-core/spec|spec]]"
---

# Delta: index-paths-and-root-set

Merged into [[073-js-client-core/spec|spec]] on 2026-11-16 (#666, #667, #668).

## MODIFIED
- FR-009 one path grammar (`$`, `.key`, `[n]`) for validation, `parseJsonPath` and the reconstructor; multi-index and root-index paths; strict index resolution (no sparse arrays, no literal keys); root `merge` in place.
- Edge Cases: out-of-range and ill-formed index rows, `delete` out of range.
