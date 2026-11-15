---
aliases:
  - Array Duplication in Reconstruction
tags:
  - sdd
  - spec
  - stream
  - bug
  - data-integrity
created: 2026-08-17
status: implemented
priority: P0
issue: "#394"
pr: "#397"
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
code:
  - crates/pjs-core/src/stream/priority.rs
  - crates/pjs-core/src/stream/reconstruction.rs
verified: 2026-11-15
---

# Feature: PriorityStreamer::analyze() + JsonReconstructor Must Not Duplicate Object-Nested Arrays

> [!info] Verification (2026-11-15)
> **State: implemented** (remediation option 1, skeleton-ify arrays in `Set`).
>
> **Evidence**
> - PR #397 (commit `e2e40f3`, "stop PriorityStreamer::analyze() from duplicating object-nested arrays on reconstruction") closed issue #394. CHANGELOG entry cites #394.
> - `PriorityStreamer::extract_patches` (`crates/pjs-core/src/stream/priority.rs`) now emits, per object field, a `PatchOperation::Set { value: Self::skeletonize_arrays(value) }` followed by the child patches from the recursive call. `skeletonize_arrays` replaces every array in the subtree with `[]`, except under keys `JsonPath::append_key` cannot encode (those subtrees stay populated because recursion never reaches them).
> - Priority hoisting: the `Set` priority is raised to at least the highest `Append` priority in its subtree (`append_ceiling`), so a `Set` is never sorted after an `Append` it must precede. Chunked arrays compute one priority from the full array (`calculate_array_priority`) shared by all chunks.
> - Regression tests in `priority.rs` run the full `analyze()` -> `JsonReconstructor::add_frame` -> `process_all_frames` -> `current_state()` round trip via a `round_trip` helper: `test_round_trip_exact_repro_case`, `..._multi_entity_payload`, `..._chunked_array_exceeds_max_patch_size`, `..._chunked_array_divergent_chunk_priority`, `..._array_nested_at_depth_three`, `..._array_of_arrays`, `..._array_of_objects_with_nested_arrays`, `..._empty_array_field`, `..._top_level_bare_array`, `..._mixed_payload_no_regression`, `..._same_path_priority_inversion`, `..._parent_object_priority_inversion` (+ `_chunked`), `..._unencodable_key_parent_wipe`.
>
> **Drift from original spec**
> - Line numbers in the source spec are obsolete (file reorganised, `JsonPath::append_key` now fallible and handled with `let Ok(..) else continue`).
> - The fix went beyond duplication: priority-sort inversions between `Set` and `Append` and divergent per-chunk priorities (#394 C1/C2) and unencodable keys (C3) were fixed in the same PR.
> - `PatchOperation` shape is unchanged (`Set`, `Append`, `Replace`, `Remove`); no wire break.
> - Open question about other patch producers (`pjs-wasm`, HTTP/WebSocket paths) was not audited here. [NEEDS CLARIFICATION]
> - Fields whose key cannot be encoded are still silently skipped from patches (`continue`), as the original spec declared out of scope.

## 1. Overview

### Problem Statement (historical)

`extract_patches` pushed a `Set` patch holding the full clone of each object field, then recursed into the same value; for a non-empty array child the recursion added an `Append` of the same elements. `JsonReconstructor` applied both in order, so `{"items":[1,2,3]}` came out as `{"items":[1,2,3,1,2,3]}`. No test fed `analyze()` output into `JsonReconstructor`.

### Goal

For any valid JSON `payload`, applying every frame of `PriorityStreamer::analyze(&payload)` in produced order to a `JsonReconstructor` yields `current_state() == payload`, including arrays at any depth and arrays longer than 10 elements or `StreamerConfig::max_patch_size`.

### Out of Scope

- New patch kinds; wire-format redesign.
- `calculate_field_priority` / `calculate_array_priority` heuristics.
- Partial or out-of-order frame application.

## 2. User Stories

- US-001: a consumer reconstructs an object with a nested array with no duplication.
- US-002: the guarantee holds at depth >= 3 and across chunk boundaries.
- US-003: a maintainer has round-trip regression tests (not patch-inspection-only).

## 3. Functional Requirements

| ID | Requirement | Priority | Status |
|----|------------|----------|--------|
| FR-001 | WHEN `analyze()` processes an object field holding a non-empty array THE SYSTEM SHALL produce patches that reconstruct exactly the original elements | must | done |
| FR-002 | WHEN the array is nested at any depth THE SYSTEM SHALL hold FR-001 at every depth | must | done |
| FR-003 | WHEN an array exceeds 10 elements (chunked by `max_patch_size`) THE SYSTEM SHALL preserve elements and order across chunks | must | done |
| FR-004 | WHEN a field is a primitive or empty array/object THE SYSTEM SHALL reconstruct it unchanged | must | done |
| FR-005 | WHEN a field is a non-empty nested object THE SYSTEM SHALL reconstruct it unchanged | must | done |
| FR-006 | WHEN all frames of one `analyze()` call are applied in produced order THE SYSTEM SHALL converge to the original document | must | done |

## 4. Non-Functional Requirements

| ID | Category | Requirement | Status |
|----|----------|-------------|--------|
| NFR-001 | Correctness | `current_state()` equals the payload for arbitrary valid JSON | done for the tested shapes; no property test found [NEEDS CLARIFICATION] |
| NFR-002 | Compatibility | `PriorityStreamFrame`/`JsonPatch` wire shape unchanged | done |
| NFR-003 | Performance | No second full clone of array contents | partial: `Set` carries a skeleton (`[]`), arrays are cloned once into `Append`; `skeletonize_arrays` still clones non-array leaves per level |
| NFR-004 | Testability | Full round-trip tests exist | done |

## 5. Data Model

| Entity | Description |
|--------|-------------|
| `JsonPatch` | `path: JsonPath`, `operation: PatchOperation`, `priority: Priority` |
| `PatchOperation` | `Set { value }`, `Append { values }`, `Replace { value }`, `Remove` |
| `PriorityStreamFrame` | `Skeleton`, `Patch`, `Complete` variants |
| `StreamingPlan` | ordered `frames` produced by `PriorityStreamer::analyze` |

## 6. Edge Cases and Error Handling

| Scenario | Behaviour |
|----------|-----------|
| Empty array field | `Set` carries `[]`; no `Append` emitted |
| Array of length <= 10 | single `Append` with all elements |
| Array of length > 10 | one `Append` per `max_patch_size` chunk, shared priority |
| Array of arrays | outer array appended; inner arrays are plain values |
| Key not encodable as a `JsonPath` segment (`.`, `[`, `]`) | field skipped by `extract_patches`; its subtree stays populated inside the parent's `Set` |
| Top-level bare array | `Append` patches only, no parent `Set` |
| Partial or out-of-order frame application | not supported |

## 7. Success Criteria

| ID | Metric | Target | Status |
|----|--------|--------|--------|
| SC-001 | `{"items":[1,2,3]}` round-trips | equal | done (`test_round_trip_exact_repro_case`) |
| SC-002 | multi-entity payload with nested arrays | equal | done |
| SC-003 | array longer than `max_patch_size` | equal | done |
| SC-004 | regression tests exist | present | done |
| SC-005 | existing suites pass | green | done in CI |

## 8. Agent Boundaries

**Always**: run fmt, clippy, nextest; keep a round-trip test for any change to `extract_patches` or `skeletonize_arrays`.
**Ask first**: changing `PatchOperation` or `JsonPatch`/`PriorityStreamFrame` serialization (consumed by `pjs-wasm`, `pjs-js-client`, HTTP/WebSocket adapters).
**Never**: fix at the reconstructor layer by deduplicating `Append`; change priority heuristics as part of this contract.

## 9. Open Questions

- Audit other patch producers for the same class of defect [NEEDS CLARIFICATION].
- Add a property-based round-trip test over arbitrary JSON [NEEDS CLARIFICATION: none found].

## 10. See Also

- [[constitution]], [[MOC-specs]]
- `crates/pjs-core/src/stream/priority.rs` (`extract_patches`, `skeletonize_arrays`), `crates/pjs-core/src/stream/reconstruction.rs` (`JsonReconstructor`)
- Issue #394; PR #397
