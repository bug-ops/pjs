---
aliases:
  - Domain GAT Ports
  - gat_port macro and port contracts
tags:
  - sdd
  - spec
  - domain
  - ports
  - gat
  - hexagonal
created: 2026-11-15
status: implemented
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
  - "[[053-stream-session-aggregate/spec|StreamSession aggregate]]"
  - "[[057-in-memory-repositories/spec|In-memory adapters]]"
  - "[[058-event-metrics-dictionary-adapters/spec|Event, metrics, dictionary adapters]]"
  - "[[008-cursor-pagination-active-sessions/spec|Cursor pagination (008)]]"
  - "[[011-zstd-dictionary-compression/spec|zstd dictionary (011)]]"
code:
  - crates/pjs-core/src/domain/macros.rs
  - crates/pjs-core/src/domain/ports/mod.rs
  - crates/pjs-core/src/domain/ports/gat.rs
  - crates/pjs-core/src/domain/ports/repositories.rs
  - crates/pjs-core/src/domain/ports/writer.rs
  - crates/pjs-core/src/domain/ports/dictionary_store.rs
  - crates/pjs-core/src/domain/config/limits.rs
  - crates/pjs-core/tests/domain_ports_test.rs
verified: 2026-11-15
---

# Spec 052: Domain GAT Ports

> [!info] Verification (2026-11-15)
> **State: implemented.** Existing behaviour documented from code at HEAD `dcf9b11`. Five ports are used end-to-end (`StreamRepositoryGat`, `StreamStoreGat`, `FrameStoreGat`, `EventPublisherGat`, `MetricsCollectorGat`/`SessionMetricsGat`); several declared ports have no implementation in the repository (see Open Questions).

## 1. Overview

### Problem
The domain needs async storage, publishing and I/O contracts that do not force `async_trait` boxing on hot paths and that keep infrastructure types out of the domain layer.

### Goal
Document the contract of every port under `domain/ports/`: how it is declared (`gat_port!`), what each method guarantees, and the supporting query types (`Pagination<S>`, `SessionQueryCriteria`, `StreamFilter`, writer/connection types) and domain limits (`domain/config`).

### Out of scope
- The `StreamSession` aggregate itself: [[053-stream-session-aggregate/spec|053]].
- In-memory implementations: [[057-in-memory-repositories/spec|057]], [[058-event-metrics-dictionary-adapters/spec|058]].
- The zstd dictionary algorithm: [[011-zstd-dictionary-compression/spec|011]].
- Wire protocol: `docs/architecture/SPECIFICATION.md`.

## 2. User Stories
- As an adapter author, I implement a port trait with `impl Future` associated types and get static dispatch.
- As an application-layer author, I depend only on port traits and never on a concrete store.
- As a reviewer, I can read one place to learn which port methods are atomic and which are blind overwrites.

## 3. Functional Requirements

### FR-001 `gat_port!` declares a GAT trait from `async fn` syntax
When a trait is declared inside `gat_port! { ... }`, the macro shall emit `pub trait X: Send + Sync` with, per `async fn m(..) -> T;`, an associated type `MFuture<'a>: Future<Output = DomainResult<T>> + Send + 'a where Self: 'a` and a method `fn m(..) -> Self::MFuture<'_>`. Supports `&self` and `&mut self` receivers. Every method therefore returns `DomainResult<T>`.
Code: `domain/macros.rs` (`gat_port!`, re-exported as `pjson_rs::domain::macros::gat_port`; `pastey` re-exported as hidden `pjson_rs::__pastey`).

### FR-002 Frame source and sink
The crate shall define `FrameSourceGat` (`next_frame`, `has_frames`, `close`) and `FrameSinkGat` (`send_frame`, `send_frames`, `flush`, `close`). `FrameSinkGatExt::send_frames_default` shall send frames one by one, stopping at the first error. Implementations of `FrameSourceGat`/`FrameSinkGat` in the repository exist only as test mocks.
Code: `domain/ports/gat.rs`.

### FR-003 `StreamRepositoryGat` is the session persistence port
The port shall provide `find_session`, `save_session`, `remove_session`, `find_active_sessions`, `find_sessions_by_criteria`, `get_session_health`, `session_exists`, plus six atomic read-modify-write methods: `create_stream_atomic`, `start_stream_atomic`, `complete_stream_atomic`, `create_stream_patch_frames_atomic`, `batch_generate_frames_atomic`, `close_session_atomic`.
- `save_session` is a blind overwrite and shall not be mixed with `*_atomic` calls on the same session id (documented race, issue #457).
- Each `*_atomic` method shall apply the corresponding `StreamSession` method under a per-session mutation and return **all** events pending on the session at drain time, not only the events that call produced.
- `*_atomic` methods return `DomainError::SessionNotFound` for an unknown session id.
- `priority_threshold` is an inclusive minimum.
Code: `domain/ports/gat.rs` (`StreamRepositoryGat`). Delivered by PR #472 and #499.

### FR-004 `StreamStoreGat` is the stream persistence port
The port shall provide `store_stream`, `get_stream`, `delete_stream`, `list_streams_for_session`, `find_streams_by_session(StreamFilter)`, `update_stream_status(StreamStatus)`, `get_stream_statistics`.
Code: `domain/ports/gat.rs`.

### FR-005 `FrameStoreGat` persists emitted frames
The port shall provide `append_frames`, `get_frames(stream_id, since_sequence, priority_filter, limit) -> FrameStorePage`, `delete_frames_for_stream`. `since_sequence` is exclusive (`sequence > since`), `priority_filter` is an inclusive minimum, `FrameStorePage::total_matching` counts matches before `limit`. Implementations shall keep append order and bound per-stream memory.
Code: `domain/ports/gat.rs` (`FrameStoreGat`, `FrameStorePage`). Delivered by PR #272.

### FR-006 `EventPublisherGat` publishes domain events
The port shall provide `publish(event)` and `publish_batch(events)`. `DomainEvent` has no identity; an implementation that stores events under a key shall mint a fresh `EventId` at publish time and shall not derive identity from event content.
Code: `domain/ports/gat.rs`.

### FR-007 Metrics ports
`MetricsCollectorGat` (`increment_counter`, `set_gauge`, `record_timing`, taking borrowed `&str` names and `HashMap<String,String>` tags) is hand-written GAT; `SessionMetricsGat` (`record_session_created`, `record_session_ended`, `record_stream_created`, `record_stream_completed`) is declared with `gat_port!`.
Code: `domain/ports/gat.rs`.

### FR-008 Declared-but-unimplemented ports
The crate declares `FrameRepositoryGat`, `EventStoreGat`, `CacheGat` (with `CacheExtensions`), `SessionTransactionGat`, `FrameWriterGat`, `WriterFactoryGat`, `ConnectionMonitorGat`. No implementation exists in `crates/` outside test code (verified with `grep "impl .* for"`).
Code: `domain/ports/gat.rs`, `domain/ports/repositories.rs`.

### FR-009 Pagination is generic over sort vocabulary
`Pagination<S>` (private `offset`, `limit`, `sort_by: Option<S>`, `sort_order`, read through accessors) shall be used via the aliases `SessionPagination = Pagination<SessionSortField>` and `FramePagination = Pagination<FrameSortField>`. `Default` is offset 0, limit 50, ascending. `Pagination::new(offset, limit) -> DomainResult<Self>` shall reject `limit == 0`, `limit > MAX_PAGINATION_LIMIT` (1000) and `offset > MAX_PAGINATION_OFFSET` (1_000_000) as `DomainError::InvalidInput`; `with_sort(sort_by, order)` sets the ordering. A `Pagination` therefore cannot hold out-of-range values, and `validate()` no longer exists (#604). `SessionSortField` (`CreatedAt`, `UpdatedAt`, `StreamCount`, `TotalBytes`) is serde `snake_case`; `FrameSortField` (`Sequence`, `Priority`) has no serde derive. PR #504.
Code: `domain/ports/repositories.rs`.

### FR-010 Query criteria validate at construction
`SessionQueryCriteria` and `StreamFilter` are built through `builder()` and `build() -> DomainResult<_>`; their fields are private (read through accessors) and there is no `validate()`. `SessionQueryCriteriaBuilder::build` shall reject `min_stream_count > max_stream_count`, `created_after > created_before`, and an empty `states` vector (`None` means no filter). `states` is `Option<Vec<SessionState>>` (accessor `Option<&[SessionState]>`), so an invalid state cannot be expressed and adapters match with exact `contains` (#754). `StreamFilterBuilder::build` shall reject an empty `statuses` list (`NonEmptyStatuses`); `has_frames` is a `FramePresence` enum. `StreamFilter.min_priority` and `max_priority` were removed because no adapter honoured them (#604).
Status: done.
Code: `domain/ports/repositories.rs`.

### FR-011 Writer and connection value types
`WriterConfig` (default: buffer 1024, timeout 30 s, compression on, max frame 1 MiB, `DropLowPriority`), `WriterMetrics`, `BackpressureStrategy` (`Block`, `DropLowPriority`, `DropOldest`, `Error`), `ConnectionState`, `ConnectionMetrics` shall be plain data types with `Default` where listed. They serve the unimplemented writer ports (FR-008).
Code: `domain/ports/writer.rs`.

### FR-012 `TimeProvider` makes clocks injectable
`TimeProvider: Send + Sync` shall expose `now()` and default `now_millis()`; `SystemTimeProvider` is the default. `StreamSession::with_time_provider` accepts it. PR #369.
Code: `domain/ports/mod.rs`.

### FR-013 `DictionaryStore` is a boxed-future port, not GAT
`DictionaryStore: Send + Sync` shall return `DictionaryFuture<'a, T> = Pin<Box<dyn Future<Output = DomainResult<T>> + Send + 'a>>`. Methods `get_dictionary(SessionId) -> Option<Arc<TrainedDictionary>>` and `train_if_ready(SessionId, Vec<u8>)` are always present: the port is no longer feature-gated and depends only on `pjson-rs-domain` types (#605). `get_dictionary` returns `Ok(None)` until training completes; `train_if_ready` fires training exactly once. `NoopDictionaryStore` is the default.
Code: `domain/ports/dictionary_store.rs`. Implementation: [[058-event-metrics-dictionary-adapters/spec|058]].

### FR-014 Domain limits
The crate shall define `MAX_PAGINATION_LIMIT = 1_000`, `MAX_PAGINATION_OFFSET = 1_000_000`, `DEFAULT_FRAME_HISTORY_PER_STREAM = 10_000`, `MAX_FRAMES_PER_REQUEST = 1_000` (must not exceed the frame history), `MAX_SESSION_TIMEOUT_SECONDS = 604_800`. Compile-time `const` assertions in unit tests pin the relations.
Code: `domain/config/limits.rs`, `domain/config/mod.rs`. Config validation of `PjsConfig` is a separate concern: [[018-config-validation-startup/spec|018]].

## 4. Non-Functional Requirements
- NFR-PERF-001: ports on the request path (FR-003..FR-007) use GAT futures, so no per-call `Box<dyn Future>`; `DictionaryStore` is exempt by design (called at most once per dictionary request and per sample). Evidence: `crates/pjs-bench/benches/gat_query_benchmarks.rs`.
- NFR-SEC-001: every iteration or page size reachable from untrusted input is capped by `MAX_PAGINATION_LIMIT`/`MAX_PAGINATION_OFFSET` (tested in `repositories.rs` unit tests).

## 5. Data Model
`FrameStorePage`, `SessionQueryResult { sessions, total_count, has_more, query_duration_ms, scan_limit_reached }`, `SessionHealthSnapshot { session_id, is_healthy, active_streams, total_frames, last_activity, error_rate, metrics: HashMap<String,f64> }`, `FrameQueryResult`, `StreamStatistics`, `StreamStatus` (`Created`, `Active`, `Paused`, `Completed`, `Failed`, `Cancelled`), `StreamMetadata`, `CacheStatistics`, `SortOrder` (`Ascending`, `Descending`).

## 6. Edge Cases and Errors
Port errors are `DomainError` (`#[non_exhaustive]`): `SessionNotFound`, `StreamNotFound`, `InvalidInput`, `InvalidStateTransition`, `InvalidStreamState`, `InvalidSessionState`, `TooManyStreams`, `InvariantViolation`, `NotFound`, `ConcurrencyConflict`, `Logic`, `Io`, `CompressionError`, `InvalidFrame`, `InvalidPriority`, `InvalidPath`.

| Situation | Result |
|---|---|
| `Pagination::new` with limit 0 or `> 1000`, or offset `> 1_000_000` | `InvalidInput` |
| `SessionQueryCriteria` built with `states == Some([])` | `InvalidInput` |
| `*_atomic` on unknown session | `SessionNotFound` |
| `StreamStatus::Paused` or `Created` target in `update_stream_status` | `InvalidStateTransition` (adapter behaviour, see 057) |

## 7. Success Criteria
- `cargo nextest run -p pjson-rs domain::ports` and `tests/domain_ports_test.rs` pass.
- Application handlers compile against the port traits only (they additionally depend on `InMemoryFrameStore` as a default type parameter, see Open Questions).

## 8. Agent Boundaries
- Always: declare new async ports with `gat_port!`; return `DomainResult`; document atomicity and ordering in the method doc.
- Ask first: adding a method to a port with existing implementations; adding a new non-GAT (boxed) port.
- Never: put `axum`, `tokio::net`, `reqwest`, filesystem types in a port signature; add `async_trait`.

## 9. Open Questions
- Seven declared ports (FR-008) have no implementation or caller. [NEEDS CLARIFICATION: keep as forward declarations or remove before v1.0.0?]
- `SessionQueryCriteria::client_info_pattern` is a `String`, not a validated type.
- `StreamStatus::Paused` has no `StreamState` counterpart.
- Two `SortOrder` enums exist: `domain::ports::SortOrder` (no serde) and `application::queries::SortOrder` (serde, `asc`/`desc` aliases); the handler maps one to the other.
- `gat_port!` bound is always `Send + Sync`; no port can opt out.
- `CLAUDE.md` still lists the port set as five traits; `domain::ports` exports 15 GAT traits plus `FrameSinkGatExt`.

## 10. See Also
[[052-domain-gat-ports/plan|plan]], `docs/architecture/SPECIFICATION.md`, [[055-application-commands/spec|055]], [[056-application-queries-dto/spec|056]], [[053-stream-session-aggregate/spec|053]].
