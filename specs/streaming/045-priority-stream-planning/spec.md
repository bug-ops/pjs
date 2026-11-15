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
  - crates/pjs-core/src/infrastructure/http/axum_extension.rs
verified: 2026-11-15
---

# Spec 045: Priority Stream Planning (skeleton + patches)

[[constitution]] | [[MOC-specs]]

> [!info] Scope
> Documents existing behaviour of `pjson_rs::stream::priority` (`PriorityStreamer`, `StreamingPlan`, `PriorityStreamFrame`, `JsonPatch`, `PatchOperation`, `StreamerConfig`) and `pjson_rs::stream::{StreamProcessor, StreamProcessorConfig}`. The reverse direction is [[046-json-reconstruction/spec|spec 046]]. The array-duplication bug is covered by [[031-array-duplication-reconstruction/spec|spec 031]]; this spec describes the resulting design.

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
The Skeleton frame shall carry priority `Priority::CRITICAL` and `complete: false`. For an object root, `generate_skeleton` maps strings to `null`, numbers to `0`, booleans to `false`, nulls to `null`, arrays to `[]`, and recurses into objects. An array root yields `[]`; any other root yields `null`. When the root is a scalar, or an object with a key `JsonPath` cannot encode, `analyze` shall additionally emit a root `Set` patch (`JsonPath::root()`, value is `skeletonize_arrays` of the input: arrays under JsonPath-encodable keys replaced by `[]`, arrays under keys JsonPath cannot encode preserved whole, since no `Append` patch can reach them) at `Priority::CRITICAL`, ordered before every other CRITICAL patch, so the root round-trips (#626).
Status: done. `complete` is never set to `true`.
Code: `PriorityStreamer::generate_skeleton`.

### FR-003 Patch extraction
For each object field whose key is encodable as a `JsonPath` segment, the system shall emit a `Set` patch at the field path whose value is the field value with every array replaced by `[]` (`skeletonize_arrays`), then recurse into the value. For an array it shall emit `Append` patches carrying the elements; arrays longer than 10 elements are split into chunks of `max_patch_size`, shorter non-empty arrays produce one `Append`. Empty arrays produce no `Append`.
Status: done.
Code: `PriorityStreamer::extract_patches`.

### FR-004 Unencodable keys
If an object key contains `.`, `[`, `]` or is empty (`JsonPath::append_key` fails), the system shall skip that field and its subtree in patch extraction without failing the plan, and shall leave the enclosing subtree fully populated in its parent's `Set` value so no data is lost.
Status: done (regression test `test_round_trip_unencodable_key_parent_wipe`).
Code: `extract_patches`, `skeletonize_arrays`.

### FR-005 Priority heuristics
The field priority shall be, in order: `CRITICAL` for keys `id|uuid|status|type|kind`; `HIGH` for `name|title|label|email|username`; `LOW` for keys containing `analytics`, `stats` or `meta`; `BACKGROUND` for keys `reviews|comments|logs|history`; otherwise by value: array longer than 100 -> `BACKGROUND`, object containing `timestamp` -> `MEDIUM`, string longer than 1000 bytes -> `LOW`, else `MEDIUM`. Array priority: more than 50 elements -> `BACKGROUND`; last key `reviews|comments|logs` -> `BACKGROUND`; `items|data|results` or anything else -> `MEDIUM`.
Status: done. Matching is by exact/substring key name only; there is no user-supplied extractor.
Code: `calculate_field_priority`, `calculate_array_priority`.

### FR-006 Ordering and batching
The system shall sort all patches by descending priority with a stable sort, then emit them as `Patch` frames in which every patch shares one priority and a frame holds at most `max_patch_size` patches (default 100).
Status: done.
Code: `analyze`.

### FR-007 Reconstruction-safe ordering
The priority of a `Set` patch shall be raised to at least the highest `Append` priority anywhere in its subtree, and all chunks of one array shall share one priority computed from the full array, so a `Set` never applies after an `Append` it must precede and chunk order is preserved.
Status: done (#394, closed). Tests: `test_round_trip_*` in `priority.rs`.
Code: `extract_patches`.

### FR-008 Plan consumption API
`StreamingPlan` shall expose `next_frame`, `is_complete`, `remaining_frames`, `frames` (iterator).
Status: done.

### FR-009 Frame wire form (serde)
`PriorityStreamFrame`, `JsonPatch` and `PatchOperation` derive `Serialize` only. The default serde enum representation applies (externally tagged: `{"Skeleton":{...}}`, `{"Patch":{...}}`, `{"Complete":{...}}`); `JsonPatch.path` serialises as the `JsonPath` display string (`$.a.b[0]`), `priority` as its `u8` value.
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
| `JsonPatch` | `path: JsonPath`, `operation: PatchOperation`, `priority: Priority` |
| `PatchOperation` | `Set{value}`, `Append{values}`, `Replace{value}`, `Remove` |
| `StreamerConfig` | `detect_semantics: bool`, `max_patch_size: usize`, `priority_threshold: Priority` |
| `StreamingPlan` | `frames: VecDeque<PriorityStreamFrame>` |
| `StreamFrame` / `StreamProcessorConfig` / `ProcessResult` / `StreamStats` | in `stream/mod.rs` |

## Edge Cases and Errors

| Case | Behaviour |
|---|---|
| Key with `.`/`[`/`]`/empty | skipped, parent `Set` keeps full subtree (FR-004) |
| Empty array field | skeleton `[]`, no `Append` |
| Array of 11..n elements | chunked by `max_patch_size`, one priority for all chunks |
| `max_patch_size = 0` | `[NEEDS CLARIFICATION]` no validation in `StreamerConfig`; `chunks(0)` panics in std for arrays longer than 10 |
| Frame larger than limit | `ProcessResult::FrameTooLarge`, not an `Err` |
| `analyze` error | `Result` return exists but no code path in `analyze` currently returns `Err` |

## Success Criteria

- All `stream::priority` and `stream::tests` unit tests pass under `cargo nextest run -p pjson-rs --lib`.
- Round trip equals input for every payload class in NFR-CORR-001.

## Agent Boundaries

- Always: keep plan order = Skeleton, Patches by descending priority, Complete; add a round-trip test for any change to patch extraction or priorities.
- Ask first: changing the serde frame representation, adding frame kinds, changing heuristic key lists (alters wire order).
- Never: emit an `Append` before the `Set` that creates its parent; drop data for unencodable keys.

## Open Questions

1. `StreamerConfig::detect_semantics` and `priority_threshold` are never read (grep: only the definition and `Default`). Heuristics always run and no patch is dropped. Dead configuration.
2. Nested objects are transmitted redundantly (by code reading): an object field's `Set` value contains its whole subtree except arrays, and child fields then get their own `Set`. Bytes grow with nesting depth. `[NEEDS CLARIFICATION: confirm with a payload-size measurement]`.
3. Resolved (#626): a scalar root, or an object root with a key `JsonPath` cannot encode, is transmitted through a root `Set` patch at CRITICAL priority; the skeleton stays `null` for a scalar root.
4. `PatchOperation::Replace` is identical to `Set` in the only consumer; the streamer never emits `Replace` or `Remove`.
5. `Complete.checksum` is always `None`.
6. Drift from `docs/architecture/SPECIFICATION.md` (sections 3.1-3.6): spec uses `@type/@seq/@priority` envelopes, JSON Pointer paths, ops `replace|add|remove|move|copy`, `@array_metadata`, error and heartbeat frames, `sha256` checksum. Implementation: externally tagged enum, `$.a[0]` paths, ops `Set|Append|Replace|Remove`, no sequence numbers, no error/heartbeat frame, no array metadata. The draft is a design target, not what `pjson-rs` emits.
7. `axum_extension.rs` serialises frames with `serde_json::to_string(&frame).expect(..)` on the SSE path; the stated reason (NaN rejected at construction) applies to `JsonData`, whereas `PriorityStreamFrame` carries `serde_json::Value`. `[NEEDS CLARIFICATION: confirm the invariant]`.
8. Priority wire value is a bare `u8`; `StreamFrame.metadata` is `HashMap<String,String>` (stringly typed).

## See Also

- [[045-priority-stream-planning/plan|plan]]
- [[046-json-reconstruction/spec|Spec 046 reconstruction]]
- [[031-array-duplication-reconstruction/spec|Spec 031]]
- `docs/architecture/SPECIFICATION.md`
- `crates/pjs-core/src/infrastructure/http/axum_extension.rs` (production consumer), `crates/pjs-demo/src/servers/simple_demo.rs`
