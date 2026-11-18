---
aliases:
  - Application Queries and DTOs
  - CQRS read side
tags:
  - sdd
  - spec
  - application
  - cqrs
  - queries
  - dto
created: 2026-11-15
status: implemented
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
  - "[[052-domain-gat-ports/spec|Domain GAT ports]]"
  - "[[055-application-commands/spec|Application commands]]"
  - "[[057-in-memory-repositories/spec|In-memory adapters]]"
  - "[[008-cursor-pagination-active-sessions/spec|Pagination of active sessions (008)]]"
  - "[[032-session-stats-total-bytes-noop/spec|SessionStats total_bytes (032)]]"
code:
  - crates/pjs-core/src/application/queries/mod.rs
  - crates/pjs-core/src/application/handlers/query_handlers.rs
  - crates/pjs-core/src/application/dto/mod.rs
  - crates/pjs-core/src/application/dto/id_dto.rs
  - crates/pjs-core/src/application/dto/priority_dto.rs
  - crates/pjs-core/src/application/dto/schema_dto.rs
verified: 2026-11-15
---

# Spec 056: Application Queries, Query Handlers, Pagination and DTOs

> [!info] Verification (2026-11-15)
> **State: implemented.** Ten queries are served by three handlers (`SessionQueryHandler`, `StreamQueryHandler`, `SystemQueryHandler`). Session listing now runs through `find_sessions_by_criteria` with validated pagination (issue #136 closed). Findings: `GetActiveSessionsQuery` does not filter by state; `SystemStatsResponse` is computed from `find_active_sessions` only.

## 1. Overview

### Problem
The read side needs typed queries and responses, bounded pagination, and serializable id/priority/schema DTOs that keep serde concerns out of the domain.

### Goal
Document queries, response types, handler behaviour (limits, defaults, errors) and DTOs.

### Out of scope
- Write side ([[055-application-commands/spec|055]]).
- HTTP query-string parsing and status mapping (`infrastructure/http`, [[017-axum-error-http-status-mapping/spec|017]]).
- Cursor pagination, tracked as [[008-cursor-pagination-active-sessions/spec|008]].

## 2. User Stories
- As an API client, I list, search and sort sessions with bounded pages.
- As a client, I fetch frames of a stream after a known sequence number.
- As an operator, I read system and session stats and a health snapshot.

## 3. Functional Requirements

### FR-001 Query set
The crate shall define `GetSessionQuery`, `GetActiveSessionsQuery { limit, offset }`, `GetStreamQuery`, `GetStreamsForSessionQuery { include_inactive }`, `GetSessionHealthQuery`, `GetStreamFramesQuery { since_sequence, priority_filter, limit }`, `GetSessionStatsQuery`, `GetSystemStatsQuery { include_historical }`, `SearchSessionsQuery { filters: SessionFilters, sort_by, sort_order, limit, offset }`. Responses: `SessionResponse`, `SessionsResponse { sessions, total_count, has_more }`, `StreamResponse`, `StreamsResponse`, `FramesResponse { frames, total_count }`, `HealthResponse`, `SessionStatsResponse`, `SystemStatsResponse`.
Code: `application/queries/mod.rs`.

### FR-002 QueryHandlerGat
`QueryHandlerGat<TQuery>` shall mirror `CommandHandlerGat` (GAT `HandleFuture`, `Response`). `SessionQueryHandler<R>` handles session queries, `StreamQueryHandler<R, S, F>` handles stream and frame queries, `SystemQueryHandler<R>` handles `GetSystemStatsQuery` (`with_start_time` injects the uptime origin).

### FR-003 Single-session queries
`GetSessionQuery`, `GetSessionHealthQuery`, `GetSessionStatsQuery` shall call `find_session` and return `ApplicationError::NotFound("Session {id} not found")` when absent. Stats response carries `SessionStats`, stream count, active stream count, timestamps and `duration_ms`. Health response is `StreamSession::health_check()`.

### FR-004 GetActiveSessions
The handler shall use `limit = query.limit.unwrap_or(100).min(100)`, `offset = query.offset.unwrap_or(0)`, `SessionQueryCriteria::default()` and ascending unsorted `SessionPagination`, and call `find_sessions_by_criteria`. A `limit` of 0 or an `offset` above `MAX_PAGINATION_OFFSET` is rejected by `Pagination::new` and surfaces as `ApplicationError::Domain(InvalidInput)`.
Status: done with drift. With default criteria the in-memory adapter matches sessions of **every** state and does not exclude expired ones, although the query is named "active".

### FR-005 SearchSessions
The handler shall clamp `limit` to `1..=100` (default 100) and `offset` to at most `MAX_PAGINATION_OFFSET`, so it never errors on out-of-range paging. With no `filters.state`, criteria shall be `states = [SessionState::Active]`, `exclude_expired = true`; an explicit state matches only that state, expired or not. `created_after`, `created_before`, `client_info` (case-insensitive substring, `None` client info never matches) and `has_active_streams` pass through. `sort_by` is `SessionSortField` (`created_at`, `updated_at`, `stream_count`, `total_bytes`), `sort_order` is `ascending|descending` (serde aliases `asc`, `desc`); default ascending.
Code: `query_handlers.rs`.

### FR-006 Stream queries
`GetStreamQuery` and `GetStreamsForSessionQuery` shall read the stream from the owning session (`find_session` then `session.stream(id)`), returning `NotFound` for a missing session or stream. `include_inactive == false` keeps only streams for which `Stream::is_active()` (`Preparing` or `Streaming`). The injected `StreamStoreGat` is accepted by the constructor and not used.

### FR-007 GetStreamFrames
The handler shall verify the session and stream exist, cap `limit` to `MAX_FRAMES_PAGE_SIZE` (= `MAX_PAGINATION_LIMIT`, 1000; `None` becomes 1000), convert `priority_filter` via `Priority::try_from` (0 becomes a `Domain` error), and call `FrameStoreGat::get_frames`. `FramesResponse.total_count` is `FrameStorePage::total_matching`. `since_sequence` is exclusive.

### FR-008 System stats
`GetSystemStatsQuery` shall call `find_active_sessions` (capped by the adapter at 10 000), then sum `total_streams`, `active_streams`, `total_frames`, `total_bytes` from each session's stats, average the duration of sessions that have `completed_at`, and compute `frames_per_second` and `bytes_per_second` over `uptime_seconds = max(1, elapsed)`. `total_sessions` therefore equals the number of sessions returned by `find_active_sessions`, which are already active, so `total_sessions == active_sessions`. `include_historical` is ignored.

### FR-009 Id DTO
`IdDto<T: IdMarker>` shall serialize transparently as a UUID string; aliases `SessionIdDto`, `StreamIdDto`. `From<Id<T>>`/`Into<Id<T>>` are infallible; `from_string` returns `uuid::Error` on a bad UUID; `Display` prints the UUID.
Code: `dto/id_dto.rs`.

### FR-010 Priority DTO
`PriorityDto` shall serialize transparently as a `u8`. `PriorityDto::new(u8)` validates via `Priority::new`; `TryFrom<PriorityDto> for Priority` validates; `From<Priority>` is infallible. Deserialization does **not** validate (`0`, which `Priority` rejects, deserializes and fails at the `TryFrom` step).
Code: `dto/priority_dto.rs`.

### FR-011 Schema DTOs
`SchemaDefinitionDto` (`String`, `Integer`, `Number`, `Boolean`, `Null`, `Array`, `Object`, `OneOf`, `AllOf`, `Any`), `SchemaRegistrationDto { id: String, schema, metadata }`, `SchemaMetadataDto`, `ValidationRequestDto { schema_id: String, data: String }`, `ValidationResultDto { valid, errors }`, `ValidationErrorDto { path, message, error_type: String }` shall convert to and from `Schema` (`From<SchemaDefinitionDto> for Schema` is infallible, `From<&Schema> for SchemaDefinitionDto` for the reverse) and map each `SchemaValidationError` to a snake_case `error_type` (`type_mismatch`, `missing_required`, `out_of_range`, `string_length`, `pattern_mismatch`, `array_size`, `duplicate_items`, `invalid_enum`, `additional_property`, ...). Deserialization of nested schemas shall be depth-guarded with `enter_deserialize_depth` and the thread-local `SCHEMA_DTO_DESERIALIZE_DEPTH` (limit `MAX_DESERIALIZE_DEPTH` from `pjs-domain`). `Object.additional_properties` defaults to `true`; `unique_items` defaults to `false`.
Code: `dto/schema_dto.rs`. No handler or endpoint consumes these DTOs yet.

## 4. Non-Functional Requirements
- NFR-SEC-001: every page size is capped (sessions 100, frames 1000); offset is capped at 1 000 000.
- NFR-SEC-002: schema DTO nesting is bounded during deserialization (60 unit tests in `schema_dto.rs` include depth cases).
- NFR-PERF-001: stats and listings clone sessions out of the store; the cost is bounded by the adapter's scan/result caps (10 000).

## 5. Data Model
See FR-001. `SessionFilters { state: Option<SessionState>, created_after, created_before, client_info: Option<String>, has_active_streams }`. `SessionSortField` is re-exported from `domain::ports`; `application::queries::SortOrder` is separate from `domain::ports::SortOrder`.

## 6. Edge Cases and Errors

| Case | Result |
|---|---|
| unknown session in any single-session query | `NotFound` |
| unknown stream in `GetStream` or `GetStreamFrames` | `NotFound` |
| `GetActiveSessions` limit 0 | `Domain(InvalidInput)` |
| `SearchSessions` limit 0 | clamped to 1, no error |
| `GetStreamFrames` limit above 1000 | clamped to 1000 |
| `priority_filter` of 0 | `Domain(InvalidPriority)` |
| page beyond end | empty `sessions`, `has_more = false` |

## 7. Success Criteria
- 43 async tests in `query_handlers.rs` pass (pagination, sort fields, state filter default scope, frame filters, caps).
- `GET /pjs/sessions` and `/pjs/sessions/search` return `SessionsResponse` with `has_more` consistent with `total_count`.

## 8. Agent Boundaries
- Always: cap page sizes in the handler and validate in the repository; return `NotFound` for absent entities.
- Ask first: changing the default scope of `SearchSessions` or `GetActiveSessions` (public behaviour); adding fields to responses (serialized).
- Never: return unbounded collections; deserialize nested schemas without the depth guard.

## 9. Open Questions
- `GetActiveSessionsQuery` returns all states while `SearchSessions` defaults to `Active` and not expired; align or rename. [NEEDS CLARIFICATION]
- Offset pagination order is unstable without `sort_by` because the store iterates a `DashMap` (see [[057-in-memory-repositories/spec|057]]); [[008-cursor-pagination-active-sessions/spec|008]] remains the place for cursor semantics.
- `SystemStatsResponse` reports only active sessions and ignores `include_historical`; `find_active_sessions` is capped at 10 000 so large deployments get truncated sums.
- `GetStreamsForSessionQuery` and `StreamStoreGat` are not used by any HTTP route; `StreamQueryHandler` keeps a phantom `S` type parameter.
- `SessionResponse`/`StreamResponse` embed full domain entities (`StreamSession`, `Stream`, including `pending_events` and `source_data`) in the response, leaking domain shape through the serialization boundary.
- `ValidationErrorDto.error_type`, `SchemaRegistrationDto.id`, `ValidationRequestDto.schema_id` are `String` (stringly typed); `PriorityDto` deserialization is unvalidated.
- Schema DTOs and `ValidationService` ([[054-domain-services/spec|054]]) are not wired to any endpoint.

## 10. See Also
[[056-application-queries-dto/plan|plan]], [[057-in-memory-repositories/spec|057]], [[055-application-commands/spec|055]].
