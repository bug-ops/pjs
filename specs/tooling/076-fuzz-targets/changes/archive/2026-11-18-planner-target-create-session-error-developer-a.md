---
tags:
  - sdd
  - delta
created: 2026-11-18
status: merged
related:
  - "[[076-fuzz-targets/spec|spec]]"
---

# Delta: planner-target-create-session-error

Merged into [[076-fuzz-targets/spec|spec]] on 2026-11-18 (#700, #707).

## MODIFIED
- FR-011 `planner_round_trip` (spec.md:101): the target builds `StreamOptions` through `StreamOptions::default()` and field assignment (the type is `#[non_exhaustive]`), takes `max_frame_size` through `MaxFrameSize::new` and ends the run when it is below 1024. `fuzzing::plan_messages` fails only with `CreateSessionError::PlanRejected`; `InvalidOptions` no longer exists.
