---
aliases:
  - Priority Stream Planning
  - PriorityStreamer
tags:
  - sdd
  - spec
  - stream
  - priority
  - skeleton-patch
created: 2026-11-15
status: partial
issue: "#394"
related:
  - "[[046-json-reconstruction/spec]]"
  - "[[047-streaming-frame-compression/spec]]"
  - "[[031-array-duplication-reconstruction/spec]]"
  - "[[014-stream-poll-waker-contract/spec]]"
code:
  - crates/pjs-core/src/stream/priority.rs
  - crates/pjs-core/src/stream/mod.rs
verified: 2026-11-18
---

# Spec 045: Priority Stream Planning (skeleton + patches)

[[constitution]] | [[MOC-specs]]

> [!info] Scope
> Documents existing behaviour of `pjson_rs::stream::priority` (`PriorityStreamer`, `StreamingPlan`, `PriorityStreamFrame`, `JsonPatch`, `StreamerConfig`; patches are `stream::patch::WirePatch`) and `pjson_rs::stream::{StreamProcessor, StreamProcessorConfig}`. The reverse direction is [[046-json-reconstruction/spec|spec 046]]. The array-duplication bug is covered by [[031-array-duplication-reconstruction/spec|spec 031]]; this spec describes the resulting design.

## Overview

**Problem.** A server holding a complete `serde_json::Value` must emit it so a client can render the important fields first (the PJS value proposition).

**Goal (as built).** `PriorityStreamer::analyze` turns one JSON value into a finite, ordered `StreamingPlan`: one Skeleton frame, then Patch frames in descending priority, then one Complete frame.

**Out of scope.** Transport (HTTP/WS/SSE), compression ([[047-streaming-frame-compression/spec|spec 047]]), the wire protocol draft in `docs/architecture/SPECIFICATION.md` (see Drift in Open Questions), per-frame sequence numbers, error/heartbeat frames.

## User Stories

- As a server author, I call `analyze(&json)` and send `plan.next_frame()` results in order so clients see `id`/`status`/`name` first.
- As a client author, I receive frames in plan order and replay them into `JsonReconstructor` to rebuild the original value.
- As an operator, I bound the size of any single Patch frame with `StreamerConfig::max_patch_size`.

## Functional Requirements

### FR-001 Plan shape
When `analyze` is called, the system shall return a `StreamingPlan` whose `frames` queue is: exactly one `PriorityStreamFrame::Skeleton`, zero or more `Patch` frames, exactly one `Complete { checksum: None }`.
Status: done.
Code: `PriorityStreamer::analyze`.

### FR-002 Skeleton content
The Skeleton frame shall carry priority `Priority::CRITICAL` and `complete: false`. For an object root, `generate_skeleton` maps strings to `null`, integers to `0`, floats to `0.0`, booleans to `false`, nulls to `null`, arrays to `[]`, and recurses into objects. An array root yields `[]`; any other root yields `null`. When the root is a scalar, `analyze` shall additionally emit a root `Set` patch (`JsonPath::root()`, value is `skeletonize_arrays` of the input) at `Priority::CRITICAL`, ordered before every other CRITICAL patch, so the root round-trips (#626). An object root needs no root `Set`: every key is addressable (FR-003).
Status: done. `complete` is never set to `true`.
Code: `PriorityStreamer::generate_skeleton`.

### FR-003 Patch extraction
For every object field (every key is addressable: a key that is not plain gets a quoted `JsonPath` segment), the system shall emit a `Set` patch at the field path whose value is the field value with every array replaced by `[]` (`skeletonize_arrays`), then recurse into the value. A `Set` is omitted when an ancestor field's effective priority (FR-007) is at least its own: the ancestor `Set` is applied first and already carries the same content, so along any root-to-leaf chain a leaf is cloned once per strictly increasing effective-priority step, not once per depth (#770). The walk is two passes over one `JsonPath` buffer (`push`/`pop`); a path is cloned only when a patch is emitted. For an array it shall emit `Append` patches carrying the elements; arrays longer than 10 elements are split into chunks of `max_patch_size`, shorter non-empty arrays produce one `Append`. Empty arrays produce no `Append`.
Status: done.
Code: `PriorityStreamer::{collect_effective_priorities, extract_patches}`.

### FR-004 Unencodable keys (removed)
Removed in #698: `JsonPath::append_key` is infallible, so `PriorityStreamer` neither skips fields nor keeps subtrees whole for special keys; there is no object case in `needs_root_set` and no key check in `skeletonize_arrays`. `test_round_trip_unencodable_key_parent_wipe` is a round-trip over the same documents.
Status: obsolete.

### FR-005 Priority heuristics
Every `Set` and `Append` priority shall be `compute_priority(&PriorityHeuristicConfig::default(), path, shape)` from `pjs-domain` ([[044-domain-entities-events-priority-heuristic/spec|spec 044]] FR-044-011), with `shape` the `ValueShape` of the value (a string's byte length, an array's or object's element count) and `path` the full path, so a document is ordered the same way on every transport (#750). The streamer owns one heuristic, built when it is constructed. A `Set` is still raised to the highest `Append` priority in its subtree (FR-007).
Status: done. The streamer uses the default field lists; there is no user-supplied extractor.
Code: `PriorityStreamer::{priority_of, priority_of_array}`, `ValueShape::from(&serde_json::Value)`.

### FR-006 Ordering and batching
The system shall sort all patches by descending priority with a stable sort, then emit them as `Patch` frames in which every patch shares one priority and a frame holds at most `max_patch_size` patches (default 100).
Status: done.
Code: `analyze`.

### FR-007 Reconstruction-safe ordering
The effective priority of a `Set` patch is its own priority raised to at least the highest `Append` priority anywhere in its subtree (first pass), and all chunks of one array shall share one priority computed from the full array, so a `Set` never applies after an `Append` it must precede and chunk order is preserved.
Status: done (#394, closed). Tests: `test_round_trip_*` in `priority.rs`.
Code: `extract_patches`.

### FR-008 Plan consumption API
`StreamingPlan` shall expose `next_frame`, `is_complete`, `remaining_frames`, `frames` (iterator).
Status: done.

### FR-009 Frame wire form (serde)
`PriorityStreamFrame` and `JsonPatch` derive `Serialize` only. The default serde enum representation applies (externally tagged: `{"Skeleton":{...}}`, `{"Patch":{...}}`, `{"Complete":{...}}`); `JsonPatch` serialises as `{"patch": <WirePatch>, "priority": <u8>}`, where `WirePatch` is the spec 067 3-array `[op, [segments], argument]`.
Status: done. There is no `Deserialize`; clients in this crate consume frames as Rust values.
Code: `stream/priority.rs`, `pjs-domain` `JsonPath` `Serialize`.

### FR-010 Frame-size gate (`StreamProcessor`)
When `StreamProcessor::process_frame` receives a `StreamFrame`, it shall serialise `data` and return `ProcessResult::FrameTooLarge { size, limit }` if the byte length exceeds `StreamProcessorConfig::max_frame_size` (default 1 MiB), otherwise `ProcessResult::Processed(frame)` and increment the processed counter. A serialisation failure shall return `DomainError::Logic`.
Status: done.
Code: `stream/mod.rs`.

## Non-Functional Requirements

- NFR-CORR-001: `analyze` followed by in-order replay through `JsonReconstructor` shall reproduce the input for objects, nested arrays, arrays of arrays and top-level arrays. Verified by 14 `test_round_trip_*` tests in `stream/priority.rs`.
- NFR-DET-001: for identical input and config the plan is identical (stable sort; key traversal follows `serde_json::Map` order).
- NFR-SEC-001: `analyze` recursion is not depth-limited. `[NEEDS CLARIFICATION: callers must bound input depth before analyze; see spec 051]`.
- Performance: no dedicated benchmark for `analyze`. `[NEEDS CLARIFICATION]`.

## Data Model

| Type | Notes |
|---|---|
| `PriorityStreamFrame` | `Skeleton{data, priority, complete}`, `Patch{patches, priority}`, `Complete{checksum: Option<u64>}` |
| `JsonPatch` | `patch: WirePatch`, `priority: Priority` |
| `WirePatch` | `Set`, `Merge`, `Append`, `AppendStr` (`pjson_rs::stream::patch`); the streamer emits `Set` and `Append` only |
| `StreamerConfig` | `max_patch_size: NonZeroUsize` (default 100) |
| `StreamingPlan` | `frames: VecDeque<PriorityStreamFrame>` |
| `StreamFrame` / `StreamProcessorConfig` / `ProcessResult` / `StreamStats` | in `stream/mod.rs` |

## Edge Cases and Errors

| Case | Behaviour |
|---|---|
| Key with `.`/`[`/`]`/empty | `Set` at a quoted path segment (FR-003) |
| Empty array field | skeleton `[]`, no `Append` |
| Array of 11..n elements | chunked by `max_patch_size`, one priority for all chunks |
| `max_patch_size = 0` | unrepresentable: the field is a `NonZeroUsize` (#760) |
| Frame larger than limit | `ProcessResult::FrameTooLarge`, not an `Err` |
| `analyze` error | `Result` return exists but no code path in `analyze` currently returns `Err` |

## Success Criteria

- All `stream::priority` and `stream::tests` unit tests pass under `cargo nextest run -p pjson-rs --lib`.
- Round trip equals input for every payload class in NFR-CORR-001.

## Agent Boundaries

- Always: keep plan order = Skeleton, Patches by descending priority, Complete; add a round-trip test for any change to patch extraction or priorities.
- Ask first: changing the serde frame representation, adding frame kinds, changing the shared heuristic's field lists (alters wire order).
- Never: emit an `Append` before the `Set` that creates its parent.

## Open Questions

1. Resolved (#760): the never-read `StreamerConfig::detect_semantics` and `priority_threshold` are removed.
2. Resolved (#770): a child `Set` that an ancestor `Set` of at least the same priority already covers is no longer emitted (FR-003).
3. Resolved (#626): a scalar root, or an object root with a key `JsonPath` cannot encode, is transmitted through a root `Set` patch at CRITICAL priority; the skeleton stays `null` for a scalar root.
4. Resolved (#751): `PatchOperation` and its `Replace` and `Remove` are removed; patches use the `WirePatch` vocabulary.
5. `Complete.checksum` is always `None`.
6. Drift from `docs/architecture/SPECIFICATION.md` (sections 3.1-3.6): spec uses `@type/@seq/@priority` envelopes, JSON Pointer paths, ops `replace|add|remove|move|copy`, `@array_metadata`, error and heartbeat frames, `sha256` checksum. Implementation: externally tagged enum, `$.a[0]` paths, ops `set|merge|append|append_str` (`WirePatch`), no sequence numbers, no error/heartbeat frame, no array metadata. The draft is a design target, not what `pjson-rs` emits.
7. Priority wire value is a bare `u8`; `StreamFrame.metadata` is `HashMap<String,String>` (stringly typed).

## See Also

- [[045-priority-stream-planning/plan|plan]]
- [[046-json-reconstruction/spec|Spec 046 reconstruction]]
- [[031-array-duplication-reconstruction/spec|Spec 031]]
- `docs/architecture/SPECIFICATION.md`
- `crates/pjs-demo/src/servers/simple_demo.rs` (consumer)
