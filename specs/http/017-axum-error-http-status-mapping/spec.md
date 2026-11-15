---
aliases:
  - PjsError HTTP Status Mapping
tags:
  - sdd
  - spec
  - http
  - errors
created: 2026-04-28
status: implemented
priority: P2
issue: "#173"
pr: "#187"
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
code:
  - crates/pjs-core/src/infrastructure/http/axum_adapter.rs
  - crates/pjs-core/src/application/mod.rs
verified: 2026-11-15
---

# Feature: Axum Error to HTTP Status Mapping

> [!info] Verification (2026-11-15)
> **State: implemented**, with one deviation: `Validation` maps to 400, not 422. No test exercises the per-variant mapping.
>
> **Evidence**
> - Issue #173 CLOSED (2026-04-28). PR #187 (`5e05535`, "correct ApplicationError HTTP status codes"); CHANGELOG 0.5.2: "`NotFound` -> 404, `Validation` -> 400, `Authorization` -> 401, `Concurrency`/`Conflict` -> 409, `Logic`/`Domain` -> 500 (closes #173)".
> - `impl IntoResponse for PjsError` in `infrastructure/http/axum_adapter.rs`: exhaustive inner `match` on `ApplicationError` with no wildcard arm (new variants fail to compile). Body: `Json({"error": self.to_string()})`.
> - Non-application variants: `InvalidSessionId`, `InvalidStreamId`, `InvalidPriority`, `InvalidSessionState`, `InvalidSortField`, `InvalidSortOrder` -> 400; `HttpError` -> 500.
> - `PjsError` is re-exported from `infrastructure::http` (`mod.rs`).
>
> **Drift from original spec**
> - FR-002 asked for 422; implementation and CHANGELOG use 400 Bad Request. [NEEDS CLARIFICATION: 400 accepted as final?]
> - `PjsError` gained `InvalidSessionState`, `InvalidSortField`, `InvalidSortOrder` (cursor/search routes), all 400.
> - Optional RFC 9457 `application/problem+json` was not adopted.
> - `rg "ApplicationError::(NotFound|Conflict|Authorization)"` shows no test that builds a `PjsError::Application(..)` and asserts its `into_response()` status; coverage is only indirect via router tests.

## Overview

`PjsError::Application(_)` always returned 500, hiding client errors. Goal: semantically correct statuses so clients can branch on HTTP semantics.

## User Stories

- As an API client, I get 404 for a missing session, 4xx for bad input, 401 for authorization failures, 409 for conflicts.
- As an operator, 500s indicate real server faults.

## Functional Requirements

### FR-001: NotFound -> 404. Status: done.
### FR-002: Validation -> 422. Status: superseded (implemented as 400).
### FR-003: Authorization -> 401. Status: done.
### FR-004: Conflict -> 409. Status: done.
### FR-005: Concurrency -> 409. Status: done.
### FR-006: Domain -> 500. Status: done.
### FR-007: Logic -> 500. Status: done.
### FR-008: JSON body `{ "error": "<message>" }`. Status: done.
### FR-009: Exhaustive mapping (compile-time). Status: done.

## Non-Functional Requirements

- No extra crate; error path only.

## Data Model

| `ApplicationError` | Status |
|-------------------|--------|
| `NotFound` | 404 |
| `Validation` | 400 |
| `Authorization` | 401 |
| `Concurrency`, `Conflict` | 409 |
| `Domain`, `Logic` | 500 |

## Success Criteria

- Each variant yields its status; body parses as `{"error": string}`.

## Agent Boundaries

- Always: update the match and CHANGELOG when adding an `ApplicationError` variant.
- Never: put internal error `Display` text in `HttpError` (see the invariant doc on `PjsError::HttpError`).

## Open Questions

- Missing direct unit test for the mapping (suggest one table-driven test over all variants).
- The body embeds `self.to_string()`, so `ApplicationError::Domain(..)`/`Logic(..)` messages reach the client verbatim with a 500 status; leak policy for these is not documented. [NEEDS CLARIFICATION]
- `ApplicationError` has no 403 distinction (authorization is always 401).

## See Also

- [[007-http-auth-middleware/spec|007 HTTP auth middleware]]
- RFC 9457 Problem Details: https://www.rfc-editor.org/rfc/rfc9457.html
