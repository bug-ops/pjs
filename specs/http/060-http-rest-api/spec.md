---
aliases:
  - HTTP REST API
  - PJS REST route table
tags:
  - sdd
  - spec
  - http
  - axum
  - rest
created: 2026-11-15
status: implemented
priority: P2
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
  - "[[061-http-middleware-stack/spec|HTTP middleware stack]]"
  - "[[063-http-streaming-negotiation/spec|Streaming and negotiation]]"
  - "[[017-axum-error-http-status-mapping/spec|Error to status mapping]]"
  - "[[008-cursor-pagination-active-sessions/spec|Cursor pagination]]"
  - "[[007-http-auth-middleware/spec|Auth middleware]]"
code:
  - crates/pjs-core/src/infrastructure/http/axum_adapter.rs
  - crates/pjs-core/src/infrastructure/http/handlers/mod.rs
  - crates/pjs-core/src/infrastructure/http/handlers/sessions.rs
  - crates/pjs-core/src/infrastructure/http/handlers/streams.rs
  - crates/pjs-core/src/infrastructure/http/handlers/health.rs
  - crates/pjs-core/src/infrastructure/http/handlers/dictionary.rs
verified: 2026-11-15
---

# HTTP REST API (route table, DTOs, errors, router constructors)

> [!info] Scope
> As-built description of the `pjson-rs` axum adapter: routes, request/response DTOs, error envelope, application state and router constructors. Cross-cutting layers (timeouts, rate limit, auth, security headers) are in [[061-http-middleware-stack/spec|061]]; the incremental frame endpoint is detailed in [[063-http-streaming-negotiation/spec|063]].

## Overview

**Problem.** Embedders need a ready-made CQRS-backed REST surface for session and stream lifecycle without writing handlers.

**Goal.** `create_pjs_router*` returns an `axum::Router<PjsAppState<R, P, S>>` that exposes the routes below, mapping application errors to a uniform JSON error envelope.

**Out of scope.** Live push (see [[066-websocket-server-transport/spec|066]]), the Prometheus endpoint ([[065-http-prometheus-metrics/spec|065]]).

Feature gate: the whole `infrastructure::http` module requires `http-server` (`infrastructure/mod.rs`).

## User Stories

- As an embedder I mount the router, supply repository/publisher/store implementations, and get sessions and streams over HTTP.
- As an operator I restrict CORS origins and optionally add rate limiting and API-key auth via constructor variants.
- As a client developer I get a stable JSON error body and 4xx codes for bad ids and parameters.

## Functional Requirements

### FR-001 Route table

The router shall expose exactly the routes below. Public routes carry no auth layer; protected routes receive the auth layer when an `*_with_auth` constructor is used.

| Method | Path | Handler | Group | Response |
|---|---|---|---|---|
| GET | `/pjs/health` | `system_health` | public | `{"status":"healthy","version":<crate version>,"features":["pjs_streaming","axum_integration","gat_handlers"]}` |
| GET | `/metrics` | `metrics_handler` (feature `metrics`) | public | Prometheus text, see [[065-http-prometheus-metrics/spec|065]] |
| POST | `/pjs/sessions` | `create_session` | protected | `CreateSessionResponse` |
| GET | `/pjs/sessions` | `list_sessions` | protected | `SessionsResponse` (query: `limit`, `offset`) |
| GET | `/pjs/sessions/search` | `search_sessions` | protected | `SessionsResponse` (query: `state`, `sort_by`, `sort_order`, `limit`, `offset`) |
| GET | `/pjs/sessions/{session_id}` | `get_session` | protected | `SessionResponse` |
| GET | `/pjs/sessions/{session_id}/health` | `session_health` | protected | `SessionHealthResponse` |
| GET | `/pjs/sessions/{session_id}/stats` | `get_session_stats` | protected | `SessionStatsResponse` |
| POST | `/pjs/sessions/{session_id}/streams` | `create_stream` | protected | `{"stream_id":..,"status":"created"}` |
| GET | `/pjs/sessions/{session_id}/streams/{stream_id}` | `get_stream` | protected | `StreamResponse` |
| POST | `/pjs/sessions/{session_id}/streams/{stream_id}/start` | `start_stream` | protected | `{"stream_id":..,"status":"started"}` |
| POST | `/pjs/sessions/{session_id}/streams/{stream_id}/generate-frames` | `generate_frames` | protected | `GenerateFramesResponse` |
| GET | `/pjs/sessions/{session_id}/streams/{stream_id}/frames` | `get_stream_frames` | protected | `FramesResponse` (query: `since_sequence`, `priority`, `limit`) |
| GET | `/pjs/sessions/{session_id}/streams/{stream_id}/frames/stream` | `stream_stream_frames` | protected | NDJSON or SSE body, see 063 |
| GET | `/pjs/stats` | `get_system_stats` | protected | `SystemStatsResponse` |
| GET | `/pjs/sessions/{session_id}/dictionary` | `get_session_dictionary` (features `compression`, not wasm32) | protected | `application/zstd-dictionary` bytes |

Code: `axum_adapter.rs` (`public_routes`, `protected_routes`). Note `/pjs/sessions/search` coexists with `/pjs/sessions/{session_id}`; the static segment wins in axum routing.

### FR-002 Router constructors

The crate shall provide these constructors; all end in `apply_common_layers` ([[061-http-middleware-stack/spec|061]]).

| Constructor | CORS | Rate limit | Auth | Gate |
|---|---|---|---|---|
| `create_pjs_router()` | `HttpServerConfig::default()` (`http://localhost:3000`) | no | no | - |
| `create_pjs_router_with_config(&cfg)` | `cfg` | no | no | - |
| `create_pjs_router_with_rate_limit(rl)` | default | yes | no | - |
| `create_pjs_router_with_rate_limit_and_config(&cfg, rl)` | `cfg` | yes | no | - |
| `create_pjs_router_with_auth(&cfg, ApiKeyAuthLayer)` | `cfg` | no | API key | `http-server` |
| `create_pjs_router_with_rate_limit_and_auth(&cfg, rl, auth)` | `cfg` | yes | API key | `http-server` |

Config-taking variants return `Result<Router, PjsError>`; `create_pjs_router()` returns the router directly. Status: done.

### FR-003 CORS configuration

When `HttpServerConfig::allowed_origins` is empty, the CORS layer shall deny all cross-origin requests; when it is `["*"]` it shall allow any origin; when it mixes `"*"` with explicit origins or contains an unparseable origin, router construction shall fail with `PjsError::HttpError`. Allowed methods are `GET` and `POST`, allowed headers `Content-Type` and `Authorization`, `max_age` 3600 s, credentials are never enabled. Origins match by case-sensitive byte equality. `HttpServerConfig` is `#[non_exhaustive]`; `Default` is development-only. Code: `build_cors_layer_from_origins` (shared with [[064-http-axum-extension/spec|064]]).

### FR-004 Session creation

When `POST /pjs/sessions` receives `CreateSessionRequest { max_concurrent_streams?, timeout_seconds?, client_info? }`, the handler shall default `max_concurrent_streams` to 10 and `timeout_seconds` to 3600, record the `User-Agent` header, and return `{ session_id, expires_at }` where `expires_at = now + timeout_seconds`. The command handler rejects `max_concurrent_streams == 0`, `timeout_seconds == 0`, and `timeout_seconds > MAX_SESSION_TIMEOUT_SECONDS` (604 800, seven days) with `ApplicationError::Validation` (HTTP 400). The session config sets `enable_compression: true`; `ip_address` is always `None`.

### FR-005 Stream lifecycle

- `POST .../streams` takes `StartStreamRequest { data: JsonData, priority_threshold?, max_frames? }` and creates a stream from `data` only; a `null` payload is rejected with 400 before session lookup.
- `POST .../start` starts the stream.
- `POST .../generate-frames` takes an optional body `GenerateFramesRequest { priority_threshold?, max_frames? }`; defaults are `Priority::BACKGROUND` and 16 frames; `max_frames == 0` or `> MAX_FRAMES_PER_REQUEST` (1000) yields 400; an invalid priority yields `PjsError::InvalidPriority` (400). Frames are also fed to the per-session dictionary training corpus when `compression` is enabled.
- `GET .../frames` returns a `FramesResponse` page; an invalid `priority` yields 400.

### FR-006 Query parameter validation

`state`, `sort_by`, `sort_order` on `/pjs/sessions/search` are taken as raw strings and parsed inside the handler so that failures use the JSON error envelope instead of axum's plain-text `Query` rejection. Accepted values: `state` in `Initializing|Active|Closing|Completed|Failed` (case-sensitive); `sort_by` in `created_at|updated_at|stream_count|total_bytes`; `sort_order` in `asc|ascending|desc|descending`. Empty strings are rejected. Absent parameters mean "no filter / default order". Path ids are parsed to `SessionId`/`StreamId`; failures give `InvalidSessionId`/`InvalidStreamId` (400).

### FR-007 Error envelope and status mapping

Every `PjsError` shall render as `{"error": "<message>"}` with the status below.

| Error | Status |
|---|---|
| `Application(NotFound)` | 404 |
| `Application(Validation)` | 400 |
| `Application(Authorization)` | 401 |
| `Application(Concurrency)` / `Application(Conflict)` | 409 |
| `Application(Domain)` / `Application(Logic)` | 500 |
| `InvalidSessionId`, `InvalidStreamId`, `InvalidPriority`, `InvalidSessionState`, `InvalidSortField`, `InvalidSortOrder` | 400 |
| `HttpError` | 500 |

`PjsError::HttpError` payloads reachable from requests must be generic client-safe text; internal errors are logged server-side (invariant documented on the variant).

### FR-008 Dictionary endpoint

`GET /pjs/sessions/{id}/dictionary` returns 200 with `Content-Type: application/zstd-dictionary` and `Cache-Control: private, max-age=300` when a dictionary is trained; 404 (plain text `dictionary not yet trained`) when absent; 400 for a malformed id; 500 with generic text on store failure. `PjsAppState::new` wires `NoopDictionaryStore`, so the route answers 404 until `with_dictionary_store` is used.

## Non-Functional Requirements

- NFR-SEC-001: request bodies are capped at 10 MiB (`DefaultBodyLimit`), see 061.
- NFR-SEC-002: list/page endpoints are bounded by `MAX_PAGINATION_LIMIT`/`MAX_PAGINATION_OFFSET` (`domain::config::limits`), and frame pages by `MAX_FRAMES_PER_REQUEST`.
- NFR-TEST-001: `tests/http_axum_endpoints.rs`, `http_dto_serialization.rs` and unit tests in `axum_adapter.rs` cover routes, DTO shapes, CORS construction and the error mapping.

## Data Model

| Type | Role |
|---|---|
| `PjsAppState<R, P, S, F = InMemoryFrameStore>` | `Arc`-held `SessionCommandHandler`, `SessionQueryHandler`, `StreamQueryHandler`, `SystemQueryHandler`, `Arc<dyn DictionaryStore>`; constructors `new`, `with_dictionary_store`, `with_stores` |
| `HttpServerConfig { allowed_origins }` | CORS allow-list |
| `CreateSessionRequest`/`Response`, `StartStreamRequest`, `GenerateFramesRequest`/`Response`, `SessionHealthResponse` | JSON DTOs |
| `PaginationParams`, `SearchSessionsParams`, `FrameQueryParams` | Query DTOs |
| `PjsError` | `Application`, `InvalidSessionId`, `InvalidStreamId`, `InvalidPriority`, `InvalidSessionState`, `InvalidSortField`, `InvalidSortOrder`, `HttpError` |

## Edge Cases and Errors

| Case | Behaviour |
|---|---|
| Unknown session or stream | 404 via `ApplicationError::NotFound` |
| Bad UUID in path | 400 `Invalid session ID`/`Invalid stream ID` |
| `?sort_by=` (empty) | 400 |
| `null` stream data | 400 |
| Malformed JSON body | axum `Json` extractor rejection (plain-text, not the envelope) [NEEDS CLARIFICATION: exact status/body not asserted in tests reviewed] |
| `generate-frames` with no body | defaults used |

## Success Criteria

- Every route in FR-001 answers with the documented status on the happy path and on bad ids.
- Router construction fails closed on invalid CORS config.

## Agent Boundaries

- Always: keep DTOs free of domain internals; route new handlers through `protected_routes` unless deliberately public.
- Ask first: adding a public (unauthenticated) route; changing the error envelope shape.
- Never: put `Display` text of foreign errors into `PjsError::HttpError`.

## Open Questions

- `StartStreamRequest.priority_threshold` and `max_frames` are accepted but ignored by `create_stream` (`CreateStreamCommand` gets `config: None`); `StreamParams` is exported but unused by any handler.
- `get_stream_frames` doc comment says it "currently returns empty; no persistent frame store exists yet", but `StreamQueryHandler` reads from `FrameStoreGat` (`get_frames`); the comment is stale.
- `ApplicationError::Authorization` maps to 401 rather than 403.
- All router constructors and handlers are typed to `PjsAppState<R, P, S>` (default `F = InMemoryFrameStore`), so `PjsAppState::with_stores` with a custom `FrameStoreGat` cannot be mounted through the provided routers.
- `README.md` and `CLAUDE.md` list an `http-client` feature; no such feature or `reqwest` dependency exists in `crates/pjs-core/Cargo.toml` (see [[069-transport-features-and-limits/spec|069]]).
- Session-search filters `created_after`, `created_before`, `client_info`, `has_active_streams` exist on `SessionFilters` but the HTTP handler always passes `None`.
- Cursor pagination is not implemented; offset/limit only ([[008-cursor-pagination-active-sessions/spec|008]]).

## See Also

- [[061-http-middleware-stack/spec|HTTP middleware stack]], [[063-http-streaming-negotiation/spec|Streaming]], [[065-http-prometheus-metrics/spec|Metrics]]
- `docs/architecture/SPECIFICATION.md` (wire protocol draft), `README.md`
