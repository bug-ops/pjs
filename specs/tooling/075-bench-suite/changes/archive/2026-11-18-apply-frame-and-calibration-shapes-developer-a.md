---
tags:
  - sdd
  - delta
created: 2026-11-18
status: merged
related:
  - "[[075-bench-suite/spec|spec]]"
---

# Delta: apply-frame-and-calibration-shapes

Merged into [[075-bench-suite/spec|spec]] on 2026-11-18 (#701, #715).

## MODIFIED
- FR-009 `client_apply_patches` gains a merge-framing case for the `apply_frame` payload merge (#701).
- `calibration_shapes()` (bench and `planner/gate_calibration_acceptance.rs`) gains depth 2/3/4 nested-object array shapes; every shape must stay within 1.1x of the reference.
- FR-008 `websocket_controller` calls the now-sync controller methods directly.
