---
aliases:
  - In-memory repositories and stores
  - GatInMemoryStreamRepository
tags:
  - sdd
  - spec
  - infrastructure
  - adapters
  - repository
created: 2026-11-15
status: implemented
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
  - "[[052-domain-gat-ports/spec|Domain GAT ports]]"
  - "[[053-stream-session-aggregate/spec|StreamSession aggregate]]"
  - "[[055-application-commands/spec|Application commands]]"
  - "[[056-application-queries-dto/spec|Application queries]]"
  - "[[008-cursor-pagination-active-sessions/spec|Pagination of active sessions (008)]]"
code:
  - crates/pjs-core/src/infrastructure/adapters/mod.rs
  - crates/pjs-core/src/infrastructure/adapters/gat_memory_repository.rs
  - crates/pjs-core/src/infrastructure/adapters/generic_store.rs
  - crates/pjs-core/src/infrastructure/adapters/frame_store.rs
  - crates/pjs-core/src/infrastructure/adapters/limits.rs
  - crates/pjs-core/tests/gat_repository_comprehensive.rs
  - crates/pjs-core/tests/repositories_comprehensive.rs
  - crates/pjs-core/tests/security_bounded_iteration_integration.rs
verified: 2026-11-15
---

# Spec 057: In-Memory Repository, Stream Store, Frame Store and Limits

> [!info] Verification (2026-11-15)
> **State: implemented.** These are the only implementations of `StreamRepositoryGat`, `StreamStoreGat` and `FrameStoreGat` and back the HTTP server (`PjsAppState`). There is no persistent adapter and no eviction of finished sessions.

## 1. Overview

### Problem
The HTTP adapter and tests need thread-safe, bounded, in-process storage for sessions, streams and frames.

### Goal
Document `GatInMemoryStreamRepository`, `GatInMemoryStreamStore`, `InMemoryStore<K, V>` (`SessionStore`, `StreamStore`), `InMemoryFrameStore` and the limits in `adapters/limits.rs`.

### Out of scope
- Event publisher, metrics, dictionary store: [[058-event-metrics-dictionary-adapters/spec|058]].
- Port contracts: [[052-domain-gat-ports/spec|052]].

## 2. User Stories
- As the HTTP server, I store sessions and run atomic stream operations concurrently without lost updates.
- As an operator, I know the worst-case CPU and memory of a listing or frame read.
- As a test author, I clear the stores between cases.

## 3. Functional Requirements

### FR-001 Generic store
`InMemoryStore<K, V>` (`K: Eq + Hash + Clone + Send + Sync`, `V: Clone + Send + Sync`) shall wrap `Arc<DashMap<K, V>>` and offer `new`, `count`, `is_empty`, `clear`, `all_keys`, `get` (clone), `insert`, `remove`, `contains_key`, `filter`, `filter_limited(predicate, result_limit, scan_limit) -> (Vec<V>, bool)` and `update_with(key, FnOnce(&mut V) -> R) -> Option<R>`. `filter_limited` shall stop after `scan_limit` scanned entries or `result_limit` results and return `true` when either bound stopped the scan. Pre-allocation is capped at 1024. Aliases: `SessionStore = InMemoryStore<SessionId, StreamSession>`, `StreamStore = InMemoryStore<StreamId, Stream>`. `Clone` shares the underlying map.
Code: `generic_store.rs`. Iteration is weakly consistent and in arbitrary order.

### FR-002 Repository state
`GatInMemoryStreamRepository { store: SessionStore, stats_cache: DashMap<SessionId, CachedSessionStats> }` shall implement `StreamRepositoryGat`. `new`, `Default`, `session_count`, `clear`, `all_session_ids` are public helpers.

### FR-003 Basic operations
`find_session` returns a clone; `save_session` invalidates the stats cache then inserts (blind overwrite); `remove_session` invalidates the cache and removes; `session_exists` checks the key; none of these error.

### FR-004 Atomic operations
`create_stream_atomic`, `start_stream_atomic`, `complete_stream_atomic`, `close_session_atomic` shall run the corresponding `StreamSession` method inside `InMemoryStore::update_with` (the DashMap shard write lock) and return `session.take_events()` collected into a `Vec`. `create_stream_patch_frames_atomic` and `batch_generate_frames_atomic` shall first take a snapshot clone with `store.get`, run the `extract_*` traversal on the snapshot **without** the lock, then run `commit_*` inside `update_with` with the extracted patches. All shall return `DomainError::SessionNotFound("Session {id} not found")` for a missing session and invalidate the session's stats cache on success. Delivered by PR #472 and #499.
Code: `atomic_session_update`, `StreamRepositoryGat` impl.

### FR-005 Active and criteria queries
`find_active_sessions` shall return `filter_limited(is_active, MAX_RESULTS_LIMIT, MAX_SCAN_LIMIT)`. `find_sessions_by_criteria(criteria, pagination)` shall (1) take `criteria` and `pagination`, which are valid by construction, (2) `filter_limited` with `matches_criteria`, (3) set `total_count = filtered.len()` (after caps), (4) sort by `SessionSortField` if given (`CreatedAt`, `UpdatedAt`, `StreamCount`, `TotalBytes`; ascending or descending), (5) `skip(offset).take(limit)`, (6) `has_more = offset + page_len < total_count`, and return `SessionQueryResult` with `query_duration_ms` and `scan_limit_reached`.
`matches_criteria` shall AND these: `states` (ASCII case-insensitive compare with `SessionState::as_str`), `exclude_expired`, `created_after`/`created_before` (inclusive), `has_active_streams`, `min/max_stream_count`, `client_info_pattern` (case-insensitive substring; sessions without client info never match).

### FR-005a Ordering without sort
Without `sort_by` the order is the DashMap iteration order, which is not stable between calls; offset pages may overlap or skip items under concurrent mutation.

### FR-006 Session health
`get_session_health` shall return `SessionHealthSnapshot` for an existing session (else `SessionNotFound`): `is_healthy` and `active_streams` from `StreamSession::health_check`, `error_rate = failed_streams / stream_count` (0.0 if no streams), `last_activity = updated_at`, `total_frames` from a per-session cache with TTL 5 s (`STATS_CACHE_TTL_SECS`) that sums per-stream `total_frames`, and `metrics` with exactly `active_streams`, `total_bytes`, `recent_avg_duration_ms` (bounded by `MAX_HEALTH_METRICS = 16`).

### FR-007 Stream store
`GatInMemoryStreamStore` shall implement `StreamStoreGat` over `StreamStore`: `store_stream`, `get_stream`, `delete_stream` (no error if absent), `list_streams_for_session` and `find_streams_by_session` through `filter_limited` with the same caps, `update_stream_status`, `get_stream_statistics`.
- `find_streams_by_session` shall match on session id, status (`Created`=`Preparing`, `Active`=`Streaming`, `Completed`, `Failed`, `Cancelled`), `created_after`, `has_frames`. `StreamFilter` has no priority bounds (#604).
- `update_stream_status` shall call `start_streaming` (`Active`), `complete`, `fail("Status update to Failed")`, `cancel`; `Paused` and `Created` return `InvalidStateTransition`; an unknown id returns `StreamNotFound`.
- `get_stream_statistics` shall derive a `PriorityDistribution` where only `critical_frames` and `high_frames` are populated (approximate); `medium`, `low`, `background` are 0.

### FR-008 Frame store
`InMemoryFrameStore` shall implement `FrameStoreGat` with `DashMap<StreamId, VecDeque<Frame>>`. `append_frames` appends in order and evicts the oldest frame when the per-stream cap (`DEFAULT_FRAME_HISTORY_PER_STREAM` = 10 000, or `with_capacity(n)`) is reached; `get_frames` scans the deque applying `since_sequence` (exclusive), `priority_filter` (inclusive minimum) and `limit`, returning `total_matching` over the full filtered set; `delete_frames_for_stream` removes the stream entry. `with_capacity(0)` triggers `debug_assert!` and is clamped to 1 in release. `stream_count()` reports streams with history.

### FR-009 Limits
`adapters/limits.rs` shall define `MAX_SCAN_LIMIT = 10_000`, `MAX_RESULTS_LIMIT = 10_000`, `MAX_HEALTH_METRICS = 16`, and re-export the domain `MAX_PAGINATION_LIMIT` and `MAX_PAGINATION_OFFSET`.

## 4. Non-Functional Requirements
- NFR-PERF-001: store operations are O(1) per key; listings are O(min(n, 10 000)) plus a sort of at most 10 000 clones; frame reads are linear in the per-stream history (at most 10 000).
- NFR-SEC-001: bounded iteration (scan and result caps) verified by `tests/security_bounded_iteration_integration.rs`.
- NFR-CONC-001: atomic operations on the same session serialize on the DashMap shard lock; `commit_*` after a lock-free extract re-validates stream state. Test: `test_batch_generate_frames_concurrent_with_complete_stream_loses_no_update`.
- NFR-MEM-001: frame memory bounded at `streams * 10 000` frames; session memory is not bounded (Open Questions).

## 5. Data Model
`CachedSessionStats { total_frames: AtomicU64, computed_at_secs: AtomicU64 }`; `InMemoryFrameStore { frames: Arc<DashMap<StreamId, VecDeque<Frame>>>, max_frames_per_stream }`.

## 6. Edge Cases and Errors

| Case | Result |
|---|---|
| `*_atomic` on unknown session | `SessionNotFound` |
| `Pagination::new` with limit 0 or above 1000 | `InvalidInput` |
| criteria built with `states = Some([])` | `InvalidInput` (from `build`) |
| more than 10 000 sessions | scan stops, `scan_limit_reached = true`, `total_count` reflects only scanned matches |
| `get_session_health` unknown id | `SessionNotFound` |
| `update_stream_status(Paused)` | `InvalidStateTransition` |
| `get_frames` unknown stream | empty page |

## 7. Success Criteria
- 48 unit tests in `gat_memory_repository.rs`, 13 in `generic_store.rs`, 7 in `frame_store.rs`, plus the three integration test files above, pass.
- Concurrent atomic operations preserve `SessionStats` totals.

## 8. Agent Boundaries
- Always: use `update_with` for mutations; apply `filter_limited` to every scan; invalidate the stats cache after any mutation.
- Ask first: introducing a second storage backend; changing the scan or result caps.
- Never: iterate a store without caps on request-path code; call `save_session` concurrently with `*_atomic` on the same id.

## 9. Open Questions
- No component removes completed, closed or expired sessions: `remove_session` has no production caller and `force_close_expired` is never invoked, so the session map and `stats_cache` grow without bound. [NEEDS CLARIFICATION: TTL sweeper, or max-session cap at create time.]
- `total_count` is the capped scan size, not the true total, once caps are hit; `has_more` inherits the error.
- `GatInMemoryStreamRepository::clone` shares `store` but copies `stats_cache` (`DashMap::clone`), so clones can serve stale `total_frames` up to 5 s after the other instance invalidates.
- `stats_cache` uses wall-clock seconds from `SystemTime`, not the injected `TimeProvider`.
- `GatInMemoryStreamStore` is constructed by `PjsAppState` but no query handler reads it (`StreamQueryHandler` ignores it); streams live inside `StreamSession`. Two sources of truth would result if it were used.
- `StreamStatus::Paused` unsupported.
- Priority distribution in `get_stream_statistics` is derived from byte ratios, not counted per frame.
- `find_sessions_by_criteria` clones up to 10 000 sessions per call, then sorts clones.
- `InMemoryStore::filter` (unbounded) is `pub` and still available to callers.
- `SessionQueryCriteria.states` is stringly typed (see [[052-domain-gat-ports/spec|052]]).

## 10. See Also
[[057-in-memory-repositories/plan|plan]], [[008-cursor-pagination-active-sessions/spec|008]], [[056-application-queries-dto/spec|056]], [[058-event-metrics-dictionary-adapters/spec|058]].
