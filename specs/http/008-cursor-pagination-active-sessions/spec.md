---
aliases:
  - Cursor Pagination for Active Sessions
  - Spec 008
tags:
  - sdd
  - spec
  - application
  - pagination
created: 2026-04-28
status: partial
priority: P3
issue: "#136"
pr: "#206"
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
code:
  - crates/pjs-core/src/application/handlers/query_handlers.rs
  - crates/pjs-core/src/application/queries/mod.rs
  - crates/pjs-core/src/domain/ports/repositories.rs
  - crates/pjs-core/src/infrastructure/http/handlers/sessions.rs
  - crates/pjs-core/src/infrastructure/http/axum_adapter.rs
verified: 2026-11-15
---

# Spec 008: Pagination for GetActiveSessionsQuery

> [!info] Verification (2026-11-15)
> **State: partial.** Bounded offset pagination with `has_more` is implemented; cursor pagination was not.
>
> Evidence: issue #136 closed 2026-04-28 by PR #206 (`2a80e06`, "route session listing through bounded repository pagination"); #208/#213 added `has_more` and page-cap tests. `GetActiveSessionsQuery` handler calls `find_sessions_by_criteria(SessionQueryCriteria::default(), SessionPagination{offset, limit, sort_by: None, sort_order: Ascending})` with `limit = min(query.limit or 100, 100)`. `SessionsResponse { sessions, total_count, has_more }`. `SearchSessionsQuery` was fixed the same way in #395 (`111426f`, pagination pushdown), with `limit.clamp(1,100)` and `offset.min(MAX_PAGINATION_OFFSET)`. The `TODO(CQ-010)` comment is gone.
>
> Drift from original spec:
> - No cursor: `GetActiveSessionsQuery` has only `limit`/`offset`; `Pagination<S>` has `offset, limit, sort_by, sort_order`; `SessionsResponse` has no `next_cursor`; `PaginationParams` (HTTP) has only `limit`, `offset`. A follow-up for cursors was not found [NEEDS CLARIFICATION: no open issue located].
> - Max page size 100 is a hard-coded `const MAX_PAGE_SIZE` in the handler, not configurable via `PjsAppState`.
> - `GetActiveSessionsQuery` does not clamp `offset` (unlike `SearchSessionsQuery`, which caps at `MAX_PAGINATION_OFFSET`) and uses default criteria (no state/expiry filter), so it is not strictly "active only".
> - `find_active_sessions` still exists on the port and is used by `SystemQueryHandler` (`GetSystemStatsQuery`) and benches, i.e. the unbounded scan remains for stats.
> - `MemoryRepository` placeholder (#133) was replaced by `infrastructure/adapters/gat_memory_repository.rs`.

**Type**: enhancement | **Priority**: P3 | **Status**: partial

## Functional requirements

| FR | Requirement | Status |
|----|-------------|--------|
| 1 | `GetActiveSessionsQuery` must not call `find_active_sessions` | done |
| 2 | Delegate to `find_sessions_by_criteria` with `Pagination` | done |
| 3 | `SessionsResponse.has_more: bool` | done |
| 4 | Cursor variant (opaque token of last seen id) stable across inserts | open |
| 5 | `GET /pjs/sessions?cursor=&limit=` | open (`limit`/`offset` only) |
| 6 | Max page size enforced (default 100, configurable) | partial: enforced at 100, not configurable |

## Non-functional requirements

- Offset pagination retained: met.
- Cursor must not expose raw ids (base64url JSON): open with FR-4.
- In-memory repo supports cursor seek: open.

## Open questions

- Cursor pagination: keep as separate follow-up or drop in favour of offset plus stable sort.
- Clamp `offset` in `GetActiveSessionsQuery` like search does; decide whether "active" should filter state/expiry.
- Port sort stability: `sort_by: None` ordering across inserts depends on the repository.

## See Also

- `crates/pjs-core/tests/security_bounded_iteration_integration.rs` (pagination limits)
- [[constitution]]
