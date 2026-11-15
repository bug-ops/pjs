---
aliases:
  - Domain Entities, Events and Priority Heuristic
  - Frame and Stream Entities
  - compute_priority
tags:
  - sdd
  - spec
  - domain
  - entities
  - events
  - priority
created: 2026-11-15
status: implemented
priority: P2
code:
  - crates/pjs-domain/src/entities/frame.rs
  - crates/pjs-domain/src/entities/stream.rs
  - crates/pjs-domain/src/entities/stream_map.rs
  - crates/pjs-domain/src/events/mod.rs
  - crates/pjs-domain/src/services/priority.rs
  - crates/pjs-domain/tests/frame_comprehensive.rs
  - crates/pjs-domain/tests/stream_comprehensive.rs
  - crates/pjs-domain/tests/events_comprehensive.rs
  - crates/pjs-wasm/src/priority_assignment.rs
related:
  - "[[042-domain-crate-contract-and-value-objects/spec]]"
  - "[[043-domain-schema-depth-guard-wire-encoding/spec]]"
  - "[[045-priority-stream-planning/spec]]"
  - "[[046-json-reconstruction/spec]]"
  - "[[070-wasm-bindings-api/spec]]"
verified: 2026-11-15
---

# Spec 044: Domain Entities, Events and Priority Heuristic

Part of [[MOC-specs]]. Rules in [[constitution]]. Plan: [[044-domain-entities-events-priority-heuristic/plan|plan]].

## Overview

**Problem.** Native HTTP delivery and the WebAssembly bindings must turn the same JSON payload into the same prioritised frames, and the session layer needs a typed record of what happened.

**Goal.** Specify the `Frame` and `Stream` entities (state machine, frame creation rules, statistics), `StreamMap`, the `DomainEvent` catalogue and event store trait, and `compute_priority`, the shared field-name/shape heuristic.

**Out of scope.** The `StreamSession` aggregate, ports and handlers (`pjs-core`); wire protocol text (`docs/architecture/SPECIFICATION.md`); stream planning in `pjs-core/src/stream` ([[045-priority-stream-planning/spec|045]]); client reconstruction ([[046-json-reconstruction/spec|046]]).

## User Stories

- As a server author, I create a stream from a JSON document, start it, and obtain a skeleton frame, priority-ordered patch frames and a completion frame.
- As a browser consumer (WASM), I get the same priorities as the server for the same path and value.
- As an operator, I subscribe to domain events and reconstruct session history.

## Functional Requirements

### FR-044-001 Frame types and constructors

`Frame` shall have private fields `stream_id`, `frame_type`, `priority`, `sequence: u64`, `timestamp` (UTC, set at construction), `payload: JsonData`, `metadata: HashMap<String, String>`. `FrameType` is `#[non_exhaustive]`: `Skeleton`, `Patch`, `Complete`, `Error`.
Constructors: `skeleton(stream_id, seq, data)` (priority `CRITICAL`); `patch(stream_id, seq, priority, Vec<FramePatch>) -> DomainResult` (error `InvalidFrame` if the list is empty; payload `{"patches":[{"path","operation","value"}]}` with lowercase operation names); `complete(stream_id, seq, Option<checksum>)` (payload `{}` or `{"checksum": ..}`, `CRITICAL`); `error(stream_id, seq, message, Option<code>)` (payload `{"message", ["code"]}`, `CRITICAL`).
Status: done.

### FR-044-002 Frame validation

`Frame::validate()` shall require: skeleton, complete and error frames to be critical priority; patch frames to have an object payload containing a `patches` array; error frames to have a string `message`. It shall return `DomainError::InvalidFrame` otherwise.
Status: done. Deserialisation does not call `validate()`.

### FR-044-003 Frame accessors

`Frame` shall expose getters, `with_metadata(key, value)` (builder), `metadata()`, `metadata_value()`, `is_critical()`, `is_high_priority()`, and `estimated_size()` (payload `Display` length + metadata + 200).
`FramePatch { path: JsonPath, operation: PatchOperation, value }` with constructors `set`, `append`, `merge`, `delete` (value `Null`); `PatchOperation` is `Set | Append | Merge | Delete` (`#[non_exhaustive]`, serde lowercase).
Status: done.

### FR-044-004 Stream state machine

`Stream` shall hold `id` (fresh `StreamId`), `session_id`, `state`, `config`, `stats`, timestamps, `next_sequence` (starts at 1), optional `source_data`, `metadata`. `StreamState` (`#[non_exhaustive]`): `Preparing`, `Streaming`, `Completed`, `Failed`, `Cancelled`.
Transitions: `start_streaming` (Preparing -> Streaming); `complete` (Streaming -> Completed); `fail(error)` (Preparing or Streaming -> Failed, stores `error` metadata); `cancel` (Preparing or Streaming -> Cancelled). Any other source state shall return `DomainError::InvalidStateTransition`. `Completed`/`Failed`/`Cancelled` set `completed_at`. Predicates: `is_active` (Preparing or Streaming), `is_finished`.
Status: done.

### FR-044-005 Frame generation rules

While not `Streaming`, `create_skeleton_frame`, `create_patch_frames`, `extract_prioritized_patches`, `commit_patch_frames`, `finalize_patch_frame` and `create_completion_frame` shall return `InvalidStreamState`. In `Streaming`:
- the skeleton replaces strings with `null`, numbers with 0, booleans with `false`, arrays with `[]`, recurses into objects, and yields `null` for a scalar root and `[]` for an array root;
- patch extraction walks objects only; every non-object value (scalars and whole arrays) becomes a `Set` patch at its `JsonPath`, kept when its priority is `>= threshold`; keys that `JsonPath` cannot encode (`.`, `[`, `]`, empty) are skipped;
- extracted patches are sorted by descending priority;
- `chunk_patches_for_commit(patches, max_frames)` splits them into `ceil(n / max_frames)`-sized chunks (empty for no patches or `max_frames == 0`), each chunk's priority being its maximum;
- each created frame takes `next_sequence`, then `next_sequence += 1`.
Status: done.

### FR-044-006 Stream statistics and progress

Each created frame shall update `StreamStats` (`total_frames`, per-type counters, `total_bytes` from `estimated_size`, `critical_bytes`, `high_priority_bytes`, `average_frame_size`). `progress()` shall be 0.0 (Preparing), 1.0 (Completed), a heuristic `min(total_frames / 100, 0.9)` (Streaming; 0.1 before the first frame) or `min(total_frames / 100, 0.99)` (Failed/Cancelled).
Status: done (heuristic by design).

### FR-044-007 Stream configuration

`StreamConfig` shall hold `max_frame_size` (default 64 KiB), `max_frames_per_batch` (10), `enable_compression` (true), `priority_rules: HashMap<String, Priority>` (serialised as `u8` values). `update_config` shall succeed only while the stream is active.
`priority_rules`, when non-empty, become `overrides` of the heuristic in FR-044-011.
Status: done. `max_frame_size`, `max_frames_per_batch` and `enable_compression` are not read inside the domain crate; they are carried for callers.

### FR-044-008 `StreamMap`

`StreamMap` shall index streams by their own id (`insert`, `get`, `get_mut`, `len`, `is_empty`, `iter`, `values`, `drain`); its wire format and decode checks are in [[043-domain-schema-depth-guard-wire-encoding/spec|043]] FR-043-009.
Status: done.

### FR-044-009 Domain events

`DomainEvent` shall be a `#[non_exhaustive]` enum serialised with an `event_type` tag in `snake_case` and timestamped (`DateTime<Utc>`), with these 20 variants:
- session: `SessionActivated`, `SessionClosed`, `SessionExpired`, `SessionTimedOut`, `SessionTimeoutExtended`;
- stream lifecycle: `StreamCreated`, `StreamStarted`, `StreamCompleted`, `StreamFailed { error }`, `StreamCancelled`, `StreamConfigUpdated`;
- generation: `SkeletonGenerated { frame_size_bytes }`, `PatchFramesGenerated { frame_count, total_bytes, highest_priority }`, `FramesBatched { frame_count }`, `PriorityThresholdAdjusted { old_threshold, new_threshold, reason }`, `PerformanceMetricsRecorded { metrics }`;
- flow control: `BackpressureReceived { stream_id: Option<_>, signal }`, `StreamPaused { reason }`, `StreamResumed { paused_duration_ms }`, `CreditsUpdated { available_credits, max_credits }`.
Accessors: `session_id()`, `stream_id() -> Option`, `timestamp()`, `occurred_at()` (alias), `event_type() -> &'static str` (equal to the serde tag), `is_critical()` (`StreamFailed`, `SessionExpired`), `is_error()` (`StreamFailed`), `is_completion()` (`StreamCompleted`, `SessionClosed`), `metadata() -> HashMap<String,String>`.
Status: done.

### FR-044-010 Session state, event store, subscriber

`SessionState` shall be `Initializing | Active | Closing | Completed | Failed` (`as_str`). The `EventStore` trait shall provide `append_events`, `get_events_for_session`, `get_events_for_stream`, `get_events_since` (all `Result<_, String>`); `InMemoryEventStore` implements it for tests. `EventId` wraps a UUID. `EventSubscriber` is a GAT-based trait (`handle(&self, &DomainEvent) -> HandleFuture`). `PerformanceMetrics`, `PriorityDistribution` (with `total_frames`, `as_percentages`, `from_counts`) and `PriorityPercentages` support the metrics event.
Status: done. These items have no users outside `pjs-domain` (see Open Questions); `pjs-core` defines its own ports (`EventPublisherGat`, `EventStoreGat`).

### FR-044-011 Priority heuristic

`compute_priority(&PriorityHeuristicConfig, &JsonPath, &JsonData) -> Priority` shall apply, in order:
1. exact, case-sensitive match of the path's last key in `overrides` (only when the last segment is a key);
2. case-insensitive match of that key against the lists, first match wins in order critical, high, medium, low, background. Defaults: critical `id uuid status state error type kind`; high `name title label email username description message`; medium `content body value data`; low `created_at updated_at version metadata`; background `analytics debug trace logs history comments reviews`;
3. otherwise start at `MEDIUM` (50): depth 0 or 1 adds 20, depth 2 adds 10, depth > 5 subtracts 10; then a value adjustment: string longer than 1000 subtracts 20, shorter than 50 adds 5; array longer than 100 subtracts 40, longer than 10 subtracts 15; object with more than 10 keys subtracts 10. Arithmetic saturates within 1..=255.
`PriorityHeuristicConfig` has public fields for the lists, thresholds (`large_array_threshold` 100, `mid_array_threshold` 10, `large_string_threshold` 1000, `small_string_threshold` 50, `large_object_threshold` 10) and `overrides`, plus `add_*_field` and `add_override` helpers.
The function is shared by `Stream` and `pjs-wasm` (`priority_assignment.rs`), so server and browser agree.
Status: done.

## Non-Functional Requirements

- NFR-TYPE-044-001: state transitions return typed errors instead of panicking; frame constructors enforce the non-empty patch rule.
- NFR-PERF-044-001: priority computation is O(list size) per leaf (linear scans over small string lists, `eq_ignore_ascii_case` without allocation except one `to_ascii_lowercase` of the key); frame creation clones patches.
- NFR-COMPAT-044-001: wire shapes of `Frame`, `Stream` and `DomainEvent` are pinned by `tests/serde_golden.rs`; event tags are snake_case and an unknown tag fails deserialisation.
- NFR-DET-044-001: for a fixed input, patch ordering within equal priority follows `HashMap` iteration order and is not deterministic across runs (see Open Questions).

## Data Model

`Frame`, `FrameType`, `FramePatch`, `PatchOperation`, `Stream`, `StreamState`, `StreamConfig`, `StreamStats`, `StreamMap`, `DomainEvent`, `SessionState`, `PerformanceMetrics`, `PriorityDistribution`, `PriorityPercentages`, `EventStore`, `InMemoryEventStore`, `EventId`, `EventSubscriber`, `PriorityHeuristicConfig`, `compute_priority`.

## Edge Cases and Errors

| Case | Result |
|---|---|
| `Frame::patch` with no patches | `InvalidFrame` |
| `start_streaming` twice | `InvalidStateTransition` |
| `complete` while `Preparing` | `InvalidStateTransition` |
| `fail`/`cancel` after `Completed` | `InvalidStateTransition` |
| frame creation while `Preparing` | `InvalidStreamState` |
| skeleton with `source_data == None` | `InvalidStreamState` (the data is never cleared by this crate) |
| `chunk_patches_for_commit(.., 0)` | empty vector |
| object key `"a.b"` | its subtree is not patched |
| unknown `event_type` on decode | serde error |

## Success Criteria

- `cargo nextest run -p pjson-rs-domain` passes the frame, stream, events and golden suites.
- `cargo test -p pjson-rs-domain --doc` passes the `compute_priority` examples (`$.user.id` critical; `$.nickname` short string = 75).

## Agent Boundaries

- Always: add new frame or event kinds as new enum variants with golden-test coverage; keep `compute_priority` the single source of priority logic (WASM and HTTP).
- Ask first: changing default field lists or thresholds (visible to every consumer); making `Frame` deserialisation validate.
- Never: add I/O or time-provider indirection hacks to entities without updating the `StreamSession` clock design in `pjs-core` (it uses a `TimeProvider` port; entities here call `Utc::now()` directly).

## Open Questions

- Entities call `Utc::now()` directly (`Frame::*`, `Stream::*`), while `StreamSession` in `pjs-core` uses a `TimeProvider` port (CHANGELOG 0.6.3, #330); frame and stream timestamps are therefore untestable with a fake clock.
- `Stream::collect_patches`/`generate_skeleton` recurse over `JsonData` without a depth bound; trees built programmatically (not via deserialisation, which is capped at 64) can overflow the stack.
- Patch order within one priority level is hash-map dependent (`HashMap` iteration then a stable sort by priority).
- Patches treat whole arrays as leaf values, so array elements are never prioritised individually; `append`/`merge`/`delete` operations exist on `FramePatch` but `Stream` only emits `set`.
- `Frame`, `Stream` and `DomainEvent` deserialise without invariant checks (`validate()` is opt-in; a `Streaming` stream with `completed_at` set is decodable).
- Events carry priorities as raw `u8` (`highest_priority`, `old_threshold`, `new_threshold`) and counts as `usize` (platform-width on the wire); `Priority` and fixed-width integers would be stricter.
- `EventStore` returns `Result<_, String>` (stringly typed errors); `EventStore`, `InMemoryEventStore`, `EventId`, `EventSubscriber` are unused outside the domain crate.
- `StreamConfig` limits (`max_frame_size`, `max_frames_per_batch`, `enable_compression`) are not enforced by `Stream`.
- `Stream::progress` is a frame-count heuristic, not a measure of delivered data.
- `PriorityPercentages::default` is hand-written equivalent to a derive.
- Lists in `PriorityHeuristicConfig` are `Vec<String>`; a field can appear in several tiers and the first tier wins silently.

## See Also

- [[044-domain-entities-events-priority-heuristic/plan|Plan (as built)]]
- [[042-domain-crate-contract-and-value-objects/spec|042 Value objects]]
- [[043-domain-schema-depth-guard-wire-encoding/spec|043 Schema, depth guard, wire encoding]]
- [[045-priority-stream-planning/spec|045 Priority stream planning]], [[046-json-reconstruction/spec|046 JSON reconstruction]], [[070-wasm-bindings-api/spec|070 WASM bindings]]
- `docs/architecture/SPECIFICATION.md`
