---
aliases:
  - Application Commands
  - CQRS write side
tags:
  - sdd
  - spec
  - application
  - cqrs
  - commands
created: 2026-11-15
status: implemented
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
  - "[[052-domain-gat-ports/spec|Domain GAT ports]]"
  - "[[053-stream-session-aggregate/spec|StreamSession aggregate]]"
  - "[[056-application-queries-dto/spec|Queries and DTOs]]"
  - "[[017-axum-error-http-status-mapping/spec|ApplicationError to HTTP (017)]]"
  - "[[011-zstd-dictionary-compression/spec|zstd dictionary (011)]]"
code:
  - crates/pjs-core/src/application/mod.rs
  - crates/pjs-core/src/application/commands/mod.rs
  - crates/pjs-core/src/application/handlers/mod.rs
  - crates/pjs-core/src/application/handlers/command_handlers.rs
  - crates/pjs-core/src/application/shared.rs
verified: 2026-11-15
---

# Spec 055: Application Commands and Command Handlers

> [!info] Verification (2026-11-15)
> **State: implemented.** Seven commands are handled by `SessionCommandHandler<R, P, F>`. The handler was rewritten around atomic repository methods in PR #472 and #499; `CommandValidator` was wired in PR #444. `CompleteStreamCommand.checksum` is accepted and never read.

## 1. Overview

### Problem
Write operations on sessions and streams need one orchestration layer that validates input, applies domain changes atomically, publishes domain events, and persists produced frames.

### Goal
Document commands, `CommandHandlerGat`, `SessionCommandHandler`, `CommandValidator`, `ApplicationError` and the ordering guarantees between persistence, event publishing and frame storage.

### Out of scope
- HTTP routing and status mapping ([[017-axum-error-http-status-mapping/spec|017]]).
- Read side ([[056-application-queries-dto/spec|056]]).
- WebSocket frame production, which uses a separate session model.

## 2. User Stories
- As an HTTP adapter, I send a typed command and receive an id, a frame list or `()`.
- As an operator, I get a `Validation` error before any state changes when input is out of limits.
- As a client polling frames, I read previously generated frames from the frame store.

## 3. Functional Requirements

### FR-001 Commands are plain serde DTOs
The crate shall define `CreateSessionCommand { config: SessionConfig, client_info, user_agent, ip_address }`, `CreateStreamCommand { session_id, source_data: JsonData, config: Option<StreamConfig> }`, `StartStreamCommand`, `GenerateFramesCommand { session_id, stream_id, priority_threshold: PriorityDto, max_frames }`, `CompleteStreamCommand { session_id, stream_id, checksum }`, `CloseSessionCommand`, `BatchGenerateFramesCommand { session_id, priority_threshold, max_frames }`. Ids are `SessionIdDto`/`StreamIdDto`, priority is `PriorityDto`.
Code: `application/commands/mod.rs`.

### FR-002 GAT handler traits
`CommandHandlerGat<TCommand>` shall declare `type Response`, `type HandleFuture<'a>: Future<Output = ApplicationResult<Self::Response>> + Send + 'a` and `fn handle(&self, TCommand)`. `SessionCommandHandler<R, P, F = InMemoryFrameStore>` shall implement it for all seven commands, with `R: StreamRepositoryGat`, `P: EventPublisherGat`, `F: FrameStoreGat`. Responses: `SessionId`, `StreamId`, `()`, `Vec<Frame>`.
Code: `application/handlers/mod.rs`, `command_handlers.rs`.

### FR-003 Construction
`SessionCommandHandler::new(repo, publisher)` shall use `NoopDictionaryStore` and a fresh `InMemoryFrameStore`; `with_dictionary_store` swaps the dictionary store; `with_stores(repo, publisher, dictionary_store, frame_store)` sets all.

### FR-004 CreateSession
When `CreateSessionCommand` is handled, the handler shall validate it (FR-009), build `StreamSession::new(config)`, set client info when `client_info` is `Some` (user agent and IP are attached to it), call `activate()`, `save_session` and `publish_batch` of the drained events, and return the new `SessionId`. The save-and-publish path (`save_and_publish`) drains events from the session before saving, so a later command on the stored session does not re-publish them.

### FR-005 CreateStream, StartStream, CompleteStream, CloseSession
Each shall call exactly one `*_atomic` repository method (`create_stream_atomic`, `start_stream_atomic`, `complete_stream_atomic`, `close_session_atomic`) and then `publish_batch` the returned events. `CreateStream` shall validate first (`source_data` must not be `JsonData::Null`). `DomainError::SessionNotFound` shall map to `ApplicationError::NotFound("Session {id} not found")`; every other `DomainError` maps to `ApplicationError::Domain`.
Code: `CommandHandlerGat` impls.

### FR-006 GenerateFrames
The handler shall validate (`max_frames` in `1..=MAX_FRAMES_PER_REQUEST`), convert `PriorityDto` to `Priority` (0 becomes `ApplicationError::Domain(InvalidPriority)`), call `create_stream_patch_frames_atomic`, map `SessionNotFound` and `StreamNotFound` to `NotFound`, publish events **immediately**, then (feature `metrics`) increment `pjs_frames_total` by `frames.len()`, then (feature `compression`, not wasm) feed each frame payload (`serde_json::to_vec`) to `DictionaryStore::train_if_ready` ignoring errors, then `append_frames` to the frame store, and return the frames.

### FR-007 BatchGenerateFrames
The handler shall call `batch_generate_frames_atomic(session_id, priority, max_frames)`, publish events, increment the metric, train the dictionary, group frames by `stream_id` and `append_frames` per stream. `max_frames == 0` returns an empty vector without error. Sessions with no `Streaming` stream return an empty vector.

### FR-008 Event ordering and partial failure
Events shall be published right after the atomic call and before any fallible I/O (frame store), because the atomic call has already drained them from the session. If `append_frames` fails after publish, stats and `FramesBatched` are committed while the frame store may hold fewer frames; the handler returns the error. Accepted trade-off documented in code (#477).

### FR-009 CommandValidator
`CommandValidator` shall collect all violations into `Vec<String>` and the handler shall join them with `"; "` into `ApplicationError::Validation`:
- `validate_create_session`: `max_concurrent_streams > 0`. The timeout range (1..=`MAX_SESSION_TIMEOUT_SECONDS`, 604800) is guaranteed by the `SessionTimeout` type in `SessionConfig`, so an out-of-range value fails at decode (#596).
- `validate_create_stream`: `source_data` not null.
- `validate_generate_frames`: `max_frames > 0` and `<= MAX_FRAMES_PER_REQUEST` (1000).
No validator exists for `BatchGenerateFramesCommand` (its `max_frames` is unbounded).

### FR-010 ApplicationError
`ApplicationError` shall have variants `Domain(#[from] DomainError)`, `Validation(String)`, `Authorization(String)`, `Concurrency(String)`, `NotFound(String)`, `Conflict(String)`, `Logic(String)`. `ApplicationResult<T> = Result<T, ApplicationError>`. HTTP mapping is defined in the axum adapter (`NotFound` 404, `Validation` 400, `Authorization` 401, `Concurrency`/`Conflict` 409, `Domain`/`Logic` 500).
Code: `application/mod.rs`; `infrastructure/http/axum_adapter.rs` (`IntoResponse for PjsError`).

### FR-011 Shared urgency type
`AdjustmentUrgency` (`Low`, `Medium`, `High`, `Critical`) with `as_level()`, `is_immediate()` and `Ord` shall exist in `application/shared.rs`. It has no consumer in the workspace.

## 4. Non-Functional Requirements
- NFR-CONC-001: no handler performs find-mutate-save for stream/session mutations except `CreateSession`; concurrency tests `test_batch_generate_frames_concurrent_with_complete_stream_loses_no_update` and `test_save_and_publish_does_not_leak_pending_events_across_commands` pin this.
- NFR-SEC-001: `MAX_FRAMES_PER_REQUEST` and `SessionTimeout` (bounded by `MAX_SESSION_TIMEOUT_SECONDS`) bound work and avoid chrono overflow.
- NFR-PERF-001: handlers are monomorphised over `R`, `P`, `F`; no boxed futures.

## 5. Data Model
`SessionCommandHandler { repository: Arc<R>, event_publisher: Arc<P>, dictionary_store: Arc<dyn DictionaryStore>, frame_store: Arc<F> }`. Commands in FR-001.

## 6. Edge Cases and Errors

| Case | Result |
|---|---|
| `CreateSession` with zero streams or zero timeout | `Validation` |
| timeout above 604800 s | `Validation` |
| `CreateStream` with `Null` data | `Validation`, repository not touched |
| unknown session on any atomic command | `NotFound` |
| `StartStream` or `CompleteStream` with unknown stream | `Domain(StreamNotFound)` (not `NotFound`) |
| `GenerateFrames` with unknown stream | `NotFound` |
| `priority_threshold` of 0 (`Priority` is non-zero) | `Domain(InvalidPriority)` |
| stream not `Streaming` when generating | `Domain(InvalidStreamState)` |
| frame store failure after publish | error returned, events already published |

## 7. Success Criteria
- 32 async unit tests in `command_handlers.rs` pass, including frame-store persistence, concurrency and timeout-overflow cases.
- Events for a command appear exactly once in the publisher.

## 8. Agent Boundaries
- Always: use `*_atomic` repository methods for mutations; publish events before fallible follow-up I/O; return `NotFound` for missing session.
- Ask first: adding a field to a command (serialized); changing which `DomainError` become `NotFound`.
- Never: mutate a `StreamSession` outside the repository lock; construct `ApplicationError::Domain` for a missing session.

## 9. Open Questions
- `CompleteStreamCommand.checksum` is never read or verified.
- `BatchGenerateFramesCommand.max_frames` is not validated against `MAX_FRAMES_PER_REQUEST`; `Start/Complete/Close` have no validators.
- Inconsistent `NotFound` mapping: `StartStream`/`CompleteStream` leak `Domain(StreamNotFound)` which the HTTP layer renders as 500.
- Every `ApplicationError::Domain` (including `InvalidStateTransition`, `InvalidInput`, `TooManyStreams`) renders as HTTP 500 in the axum adapter; [[017-axum-error-http-status-mapping/spec|017]] covers the variant mapping, not the nested domain mapping.
- No outbox: if `publish_batch` fails after the atomic state change, the state stays changed and drained events are lost.
- `ApplicationError` is not `Clone`/`non_exhaustive`, and carries `String` payloads (stringly typed); `Authorization`, `Concurrency`, `Conflict`, `Logic` have no producer in handlers.
- Application layer imports `infrastructure::adapters::InMemoryFrameStore` as the default type parameter of `SessionCommandHandler`, a dependency from application to infrastructure against the project's layering rule.
- `AdjustmentUrgency` is dead code.
- `train_from_frames` runs `serde_json::to_vec` per frame inside the request path.

## 10. See Also
[[055-application-commands/plan|plan]], [[052-domain-gat-ports/spec|052]], [[053-stream-session-aggregate/spec|053]], [[057-in-memory-repositories/spec|057]].
