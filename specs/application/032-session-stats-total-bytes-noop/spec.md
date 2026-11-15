---
aliases:
  - SessionStats total_bytes Dead Field
tags:
  - sdd
  - spec
  - http
  - bug
  - data-integrity
created: 2026-08-18
status: implemented
priority: P2
issue: "#400"
pr: "#410"
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
  - "[[031-array-duplication-reconstruction/spec|031 array duplication]]"
  - "[[009-realtime-metrics-observability/spec|009 realtime metrics]]"
code:
  - crates/pjs-core/src/domain/aggregates/stream_session.rs
  - crates/pjs-core/src/infrastructure/adapters/gat_memory_repository.rs
  - crates/pjs-core/src/application/handlers/query_handlers.rs
  - crates/pjs-core/src/infrastructure/http/axum_adapter.rs
verified: 2026-11-15
---

# Feature: SessionStats::total_bytes Must Reflect Real Payload Bytes

> [!info] Verification (2026-11-15)
> **State: implemented** (remediation option 1, populate the counter).
>
> **Evidence**
> - PR #410 (`fix(pjson-rs): populate StreamSession stats.total_bytes from real frame payload size`) closed issue #400. CHANGELOG entry cites #400.
> - `StreamSession::create_stream_patch_frames` adds `bytes_after - bytes_before` of the child `Stream`'s own `stats().total_bytes` to `self.stats.total_bytes` (no extra serialization). `StreamSession::create_priority_frames` adds `Frame::estimated_size()` summed over the retained frames after priority truncation, next to the `total_frames` increment.
> - `SessionStats::total_bytes` doc now states it is estimated wire bytes accumulated from `Frame::estimated_size`, and that repeated polling over unchanged source data adds the same bytes again.
> - Sort wiring still present and now meaningful: `SessionSortField::TotalBytes`, `GatInMemoryStreamRepository` comparator (`SessionSortField::TotalBytes => a.stats().total_bytes.cmp(...)`), the repository port mapping `(SessionSortField::TotalBytes, "total_bytes")` in `domain/ports/repositories.rs`, and the HTTP `sort_by` parser in `axum_adapter.rs` (accepts `created_at`, `updated_at`, `stream_count`, `total_bytes`).
> - Tests: `stream_session.rs` `test_create_stream_patch_frames_tracks_total_bytes` (plus a split-vs-combined equality check) and `query_handlers.rs` `test_search_sessions_sort_by_total_bytes_descending`.
>
> **Drift from original spec**
> - Source spec called the route `GET /pjs/sessions/search` and the `ALLOWED_SORT_FIELDS` whitelist in `domain/config/limits.rs`; sort-field validation now lives in `axum_adapter.rs` (`Invalid sort field: ... expected one of: created_at, updated_at, stream_count, total_bytes`) and the repository port.
> - Line numbers in the source are obsolete (file grew; sort-field handling was refactored).
> - `GetSystemStatsQuery` now derives `bytes_per_second = total_bytes / uptime_seconds` (uptime floored to 1) from the real counter. CHANGELOG notes `bytes_per_second` as a separate pre-existing weakness (sums a filtered/truncated session scan over full process uptime); [NEEDS CLARIFICATION: no open issue found for it].
> - `total_bytes` is an estimate, not exact serialized size, and double-counts re-polled unchanged data (documented on the field).

## 1. Overview

### Problem Statement (historical)

`SessionStats::total_bytes` was never incremented, so `sort_by=total_bytes` compared `0` to `0` for every session and `SystemStatsResponse.total_bytes`/`bytes_per_second` were always zero, while `total_frames` was real.

### Goal

`total_bytes` accurately tracks cumulative estimated payload bytes emitted by a session, so the sort axis and the system stats built on it are meaningful.

### Out of Scope

- General observability beyond this field (see [[009-realtime-metrics-observability/spec|009]]).
- Redesign of `SessionStats`.
- Backfill for existing in-memory sessions.

## 2. User Stories

- US-001: an API consumer sorting by `total_bytes` gets sessions ordered by real data volume.
- US-002: a maintainer can trust the doc comment on `total_bytes` (estimated, wire-oriented, not de-duplicated).
- US-003: regression tests exercise frame generation through to `stats().total_bytes` and the sort path.

## 3. Functional Requirements

| ID | Requirement | Priority | Status |
|----|------------|----------|--------|
| FR-001 | THE SYSTEM SHALL NOT expose `total_bytes` as a sort axis unless populated with real byte counts | must | done (populated) |
| FR-002 | WHEN `create_stream_patch_frames` or `create_priority_frames` emits frames THE SYSTEM SHALL increase `stats.total_bytes` consistently with `stats.total_frames` | must | done |
| FR-003 | WHEN a client requests the session search with `sort_by=total_bytes` THE SYSTEM SHALL order by cumulative bytes, honouring `sort_order` | must | done |
| FR-004 | IF the sort field were removed THE SYSTEM SHALL reject `sort_by=total_bytes` | must (alt.) | superseded |
| FR-005 | WHEN a developer reads `SessionStats::total_bytes` docs THE SYSTEM SHALL describe actual behaviour | must | done |

## 4. Non-Functional Requirements

| ID | Category | Requirement | Status |
|----|----------|-------------|--------|
| NFR-001 | Correctness | `total_bytes` and `total_frames` incremented at the same call sites | done |
| NFR-002 | Compatibility | Other sort fields unchanged | done |
| NFR-003 | Performance | No re-serialization to measure size | done (child stream counter delta; `estimated_size()`) |
| NFR-004 | Testability | Tests cover the real path | done |

## 5. Data Model

| Entity | Description |
|--------|-------------|
| `SessionStats` | `total_streams`, `active_streams`, `completed_streams`, `failed_streams`, `total_frames`, `total_bytes`, `average_stream_duration_ms` |
| `SessionSortField` | `CreatedAt`, `UpdatedAt`, `StreamCount`, `TotalBytes` |
| `SystemStatsResponse` | includes `total_bytes` and `bytes_per_second` |

## 6. Edge Cases and Error Handling

| Scenario | Behaviour |
|----------|-----------|
| Session with no frames | `total_bytes == 0` |
| Equal byte counts | tie; secondary/insertion order |
| Zero frames produced | counter does not move |
| Repeated polling of unchanged data | adds the same bytes again (documented) |
| Unknown `sort_by` | rejected (`Invalid sort field: ...`) |

## 7. Success Criteria

| ID | Metric | Target | Status |
|----|--------|--------|--------|
| SC-001 | `stats().total_bytes` after patch frames | non-zero, equals expected delta | done |
| SC-002 | sort by `total_bytes` desc | heavy session before empty | done (`test_search_sessions_sort_by_total_bytes_descending`) |
| SC-003 | regression tests | present | done |
| SC-004 | other sorts unchanged | green | done in CI |
| SC-005 | docs match behaviour | reviewed | done |

## 8. Agent Boundaries

**Always**: run fmt, clippy, nextest; keep `total_frames` and `total_bytes` updates co-located.
**Ask first**: changes to `SessionSortField` variants or accepted `sort_by` values; `SessionStats` serde shape.
**Never**: estimate `total_bytes` from `total_frames`; alter unrelated `SessionStats` fields.

## 9. Open Questions

- `bytes_per_second` semantics in `GetSystemStatsQuery` (see drift) [NEEDS CLARIFICATION].
- Audit other `SessionStats` fields for the same "exposed but unpopulated" pattern (e.g. `average_stream_duration_ms` is documented as updated on completion) [NEEDS CLARIFICATION].

## 10. See Also

- [[constitution]], [[MOC-specs]], [[031-array-duplication-reconstruction/spec|031]]
- `crates/pjs-core/src/domain/aggregates/stream_session.rs`
- Issues #400, #123, #139; PR #410
