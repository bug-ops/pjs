---
aliases:
  - Priority Stream Planning Plan
tags:
  - sdd
  - plan
created: 2026-11-15
status: implemented
related:
  - "[[045-priority-stream-planning/spec]]"
verified: 2026-11-18
---

# Plan 045: Priority Stream Planning (as built)

[[045-priority-stream-planning/spec|Spec]] | [[MOC-specs]]

## Architecture

Pure, synchronous, allocation-based transformation in `pjs-core::stream`. No I/O, no async. Consumer: `pjs-demo`.

```mermaid
flowchart LR
  J[serde_json::Value] --> A[PriorityStreamer::analyze]
  A --> S[generate_skeleton]
  A --> E[extract_patches]
  E --> P[compute_priority (pjs-domain)]
  E --> K[skeletonize_arrays]
  A --> O[stable sort by Reverse priority + batching]
  S --> PL[StreamingPlan: Skeleton, Patch*, Complete]
  O --> PL
  PL --> W[transport serialises frames]
```

## Design decisions

- Skeleton-first with `Set` patches that carry array-emptied subtrees, and `Append` patches for array data (fix for #394).
- Priority is attached per patch; frames batch equal-priority patches only.
- Heuristics are name-based constants in code, not configuration.
- `Priority` is the domain newtype over `NonZeroU8`; constants CRITICAL 100, HIGH 80, MEDIUM 50, LOW 25, BACKGROUND 10.

## Public API surface

`PriorityStreamer::{new, with_config, analyze}`, `StreamerConfig`, `StreamingPlan`, `PriorityStreamFrame`, `JsonPatch`, `stream::patch::WirePatch`; `StreamProcessor::{new, process_frame, stats}`, `StreamProcessorConfig`, `StreamFrame`, `ProcessResult`, `StreamStats`. Crate root re-exports `PriorityStreamer`, `StreamProcessor`, `StreamProcessorConfig`, `StreamFrame`, `ProcessResult`, `StreamStats`.

## Contracts

| Function | Precondition | Postcondition | Undefined / notes |
|---|---|---|---|
| `PriorityStreamer::analyze(&json)` | none enforced; input depth must be bounded by caller | plan = Skeleton, Patch*, Complete; patch priorities non-increasing; applying frames in order to `JsonReconstructor` reproduces `json` for object/array roots | `max_patch_size` is a `NonZeroUsize`, so zero is unrepresentable; scalar root not transmitted (see spec Open Questions) |
| `StreamingPlan::next_frame` | none | pops front frame, `None` when empty | |
| `StreamProcessor::process_frame(frame)` | none | `Processed` iff serialised data length <= `max_frame_size`; counter incremented only then | `Err(DomainError::Logic)` if serialisation fails |
| `StreamerConfig::default` | | `max_patch_size=100` (`NonZeroUsize`; the only field) | none |

## Testing strategy

- Unit: `crates/pjs-core/src/stream/priority.rs` (19 tests incl. 14 round trips through `JsonReconstructor`), `stream/mod.rs` (7).
- Integration: `crates/pjs-core/tests/stream_session_comprehensive.rs` exercises consumers.
- No fuzz target and no benchmark targets `analyze`.

## Security notes

- Input is trusted-size: no depth or size limit inside `analyze` (see [[051-security-limits-and-validated-config/spec|spec 051]]).
- Frame-size cap is advisory output of `StreamProcessor`, not enforced inside the plan.
