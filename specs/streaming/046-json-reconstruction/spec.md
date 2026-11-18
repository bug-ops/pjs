---
aliases:
  - JSON Reconstruction
  - JsonReconstructor
tags:
  - sdd
  - spec
  - stream
  - client
created: 2026-11-15
status: implemented
related:
  - "[[045-priority-stream-planning/spec]]"
  - "[[031-array-duplication-reconstruction/spec]]"
code:
  - crates/pjs-core/src/stream/reconstruction.rs
verified: 2026-11-18
---

# Spec 046: JSON Reconstruction

[[constitution]] | [[MOC-specs]]

## Overview

**Problem.** A receiver of `PriorityStreamFrame`s must rebuild the full JSON progressively and know when it is complete.

**Goal (as built).** `JsonReconstructor` is a synchronous, in-memory state machine: queue frames, apply them in order, expose the current partial document, statistics and a rough progress estimate.

**Out of scope.** Network I/O, ordering/reordering of out-of-order frames (frames are applied strictly in queue order), checksum verification, TypeScript client (see [[020-ts-client-wasm-api-sync/spec|spec 020]]).

## User Stories

- As a client, I add frames as they arrive and call `process_all_frames` to update the visible document.
- As a UI, I read `current_state()` and `progress()` to render partial data.
- As a tester, I replay a whole `StreamingPlan` and assert equality with the original ([[045-priority-stream-planning/spec|spec 045]]).

## Functional Requirements

### FR-001 Frame queue
`add_frame` shall enqueue a frame and record `start_time` on the first call. `process_next_frame` pops one frame (returns `ProcessResult::NoFrames` when empty); `process_all_frames` drains the queue and returns one `ProcessResult` per frame, stopping at the first error.
Status: done. On error the failing frame has already been popped and counted in `frames_processed`; remaining frames stay queued.
Code: `add_frame`, `process_next_frame`, `process_all_frames`.

### FR-002 Skeleton application
A `Skeleton` frame shall replace the entire current state with its `data` (its `priority` and `complete` fields are ignored).
Status: done.

### FR-003 Patch application
For a `Patch` frame, each patch shall be applied in order: `Set` and `Replace` -> `set_at_path`; `Append` -> `append_at_path`; `Remove` -> `remove_at_path`. Target resolution goes through one crate-private resolver over `&[PathSegment]` (`resolve_mut`; read-only `resolve` for the WebSocket client), shared with the WebSocket protocol and client (#699). The result is `PatchesApplied { count, priority, paths }`.
Status: done. A failing patch aborts the frame; patches before it stay applied (no rollback).

### FR-004 Set semantics
`set_at_path` on the root path replaces the whole document. Otherwise it navigates to the parent (all segments but the last) and: for an object parent inserts/overwrites the key; for an array parent parses the key as `usize` and overwrites an existing index, failing for out-of-bounds or non-numeric keys; for any other parent fails.
Status: done.

### FR-005 Append semantics
`append_at_path` shall extend the array at the path; a non-array target is an error.
Status: done.

### FR-006 Remove semantics
`remove_at_path` shall remove the object key; for arrays, remove the index if it is in range (an out-of-range index is a silent no-op); other parents fail. A missing object key is a silent no-op.
Status: done.

### FR-007 Completion
A `Complete` frame shall set `is_complete()` to true and record `end_time`; the checksum is ignored.
Status: done.

### FR-008 Progress and statistics
`progress()` returns `1.0` when complete, `0.0` before any frame, otherwise `processed / (processed + queued)` capped at `0.95`. `ReconstructionStats` counts frames, patches, skeleton frames, patch frames and exposes `frames_per_second`/`patches_per_second` once start and end times exist. `reset()` restores the initial state.
Status: done.

### FR-009 Error type
All failures shall be `crate::Error::Patch(PatchError)` with variants `KeyNotFound(String)`, `IndexOutOfBounds { index, len }`, `NotAnIndex`, `ExpectedObject`, `ExpectedArray`, `NotAContainer`, `NoTargetKey` (`UnsupportedSegment` was removed in #699: `PathSegment` is closed).
Status: done (stringly typed; see Open Questions).

## Non-Functional Requirements

- NFR-CORR-001: replaying any plan produced by `PriorityStreamer::analyze` in order yields the source value (tests in `stream/priority.rs`).
- NFR-ROB-001: patches for missing parents must return `Err`, never panic (`test_error_invalid_parent`, `test_error_array_index_out_of_bounds`).
- NFR-SEC-001: no limits on frame count, queue length, patch count, document size or path depth. `[NEEDS CLARIFICATION: untrusted input needs external bounds]`.

## Data Model

`JsonReconstructor { current_state: serde_json::Value, frame_queue: VecDeque<PriorityStreamFrame>, is_complete: bool, stats: ReconstructionStats }`; `ProcessResult { NoFrames, SkeletonApplied, PatchesApplied{count, priority, paths}, ReconstructionComplete }` (distinct from `stream::ProcessResult` of `StreamProcessor`, same name); `ReconstructionStats` with `u32` counters and `Option<Instant>` times.

## Edge Cases and Errors

| Case | Result |
|---|---|
| `process_next_frame` on empty queue | `Ok(NoFrames)` |
| Patch path with missing intermediate key | `Err("Key not found: ..")` |
| `Set` on array parent, index == len | `Err("Array index out of bounds")` |
| `Remove` missing key / out-of-range index | `Ok`, no change |
| Path ending in an index segment | by code reading `last_key()` returns the last *key* segment, not the last segment, so `Set`/`Remove` on `$.a[0]` use the wrong key and fail. The streamer never emits such paths. `[NEEDS CLARIFICATION: add a test]` |
| Frames after `Complete` | still applied; `is_complete` stays true |

## Success Criteria

- 20 unit tests in `stream/reconstruction.rs` pass; round-trip tests in `stream/priority.rs` pass.

## Agent Boundaries

- Always: keep application strictly ordered; add a test for each new path shape.
- Ask first: adding out-of-order buffering, rollback, or changing the `PatchError` variants.
- Never: panic on malformed patches; silently swallow errors that lose data.

## Open Questions

1. Counters `frames_processed: u32` / `patches_applied: u32` use unchecked `+=`.
2. Resolved by #599: errors are typed `PatchError` variants.
3. `Skeleton.complete` is ignored, so "skeleton may continue in later frames" has no effect.
4. `Replace` and `Set` are indistinguishable in behaviour.
5. `Instant` times make `ReconstructionStats` non-`Serialize`; fine for local use only.

## See Also

- [[046-json-reconstruction/plan|plan]], [[045-priority-stream-planning/spec|spec 045]], [[031-array-duplication-reconstruction/spec|spec 031]]
- `crates/pjs-demo/src/clients/websocket_demo.rs` (demo-local reconstructor type)
