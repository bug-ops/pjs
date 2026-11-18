---
tags:
  - sdd
  - delta
created: 2026-11-18
status: merged
related:
  - "[[076-fuzz-targets/spec|spec]]"
---

# Delta: drop-approx-eq

Merged into [[076-fuzz-targets/spec|spec]] on 2026-11-18 (#721).

## REMOVED
- FR-001a `approx_eq` (1-ULP float tolerance) and its helpers (`MAX_FLOAT_ULPS`, `ordered_bits`, `floats_within_ulps`, `numbers_approx_eq`) are deleted from the `pjs_fuzz` library target: no target has used them since the exact `reparsed` oracle replaced them (#690), and keeping an unused tolerance oracle invites reintroducing 1-ULP slack (#721).
