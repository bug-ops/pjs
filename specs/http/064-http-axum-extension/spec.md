---
aliases:
  - PjsExtension
  - Axum extension for existing apps
tags:
  - sdd
  - spec
  - http
  - axum
  - sse
created: 2026-11-15
status: obsolete
priority: P3
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
  - "[[060-http-rest-api/spec|HTTP REST API]]"
  - "[[063-http-streaming-negotiation/spec|Streaming and negotiation]]"
code:
verified: 2026-11-15
---

# `PjsExtension`: bolt-on PJS routes for existing axum apps

> [!info] Status
> `obsolete`: removed in #748. The module was a scaffold (stub stream endpoints, inert config knobs). Use `create_pjs_router*` from [[060-http-rest-api/spec|060]] and compose with `Router::merge`. The text below describes the removed behaviour and is kept for history.

## Overview

**Problem.** Apps with their own router want PJS detection and a few streaming endpoints without adopting `PjsAppState`.

**Goal.** `PjsExtension::new(config).extend_router(router)` nests `/pjs` routes (prefix configurable) and adds a middleware that marks PJS-style requests.

**Out of scope.** Sessions, repositories, auth, rate limiting, real stream storage.

Feature gate: module compiled under `infrastructure::http` (`http-server`). It uses `PriorityStreamer` from the crate root.

## User Stories

- As an embedder I add PJS endpoints to an existing `Router<S>` in one call.
- As an operator I keep same-origin-only by default and opt in to explicit CORS origins.

## Functional Requirements

### FR-001 Mounting

`extend_router(router)` shall `nest(route_prefix, pjs_routes)` and add `pjs_middleware` via `from_fn_with_state`. Routes (relative to the prefix, default `/pjs`):

| Method | Path | Handler | Behaviour |
|---|---|---|---|
| POST | `/stream` | `handle_stream_request` | Body `StreamRequest { data: serde_json::Value, priority?, format?, max_frames? }`; analyses with `PriorityStreamer`; returns `201` + `Location: <prefix>/stream/<uuid>` + `StreamResponse { stream_id, format, estimated_frames }` |
| GET | `/stream/{stream_id}/sse` | `handle_sse_stream` | `text/event-stream`, `Cache-Control: no-cache`, `Connection: keep-alive`; emits `data: <frame json>\n\n` for a fixed sample document |
| GET | `/health` | `handle_pjs_health` | JSON with `status`, `service: "pjs-extension"`, `version`, `capabilities` |

### FR-002 Format default

When `StreamRequest.format` is absent the handler shall pick `sse` if `Accept` contains `text/event-stream`, else `ndjson` if it contains `application/x-ndjson`, else `json` (substring match, not RFC 9110 negotiation). `priority` and `max_frames` are accepted but unused.

### FR-003 Request marker

The middleware shall insert `PjsStreamingRequest { enabled: true }` into request extensions when `Accept` contains `application/pjs-stream` or `text/event-stream`, or header `x-pjs-stream` is present. It never rejects.

### FR-004 CORS opt-in (CWE-942)

`HttpExtensionConfig::allowed_origins` defaults to `[]`: no CORS layer and no `Access-Control-Allow-Origin` header. A non-empty list is validated by the shared `build_cors_layer_from_origins`; an invalid list (for example `*` mixed with explicit origins) logs an error and adds no CORS layer (fail closed, no panic).

### FR-005 Config defaults

`HttpExtensionConfig`: `route_prefix` `/pjs`, `auto_detect` true, `default_priority` `Priority::MEDIUM`, `max_streams_per_client` 10, `session_timeout` 3600 s, `allowed_origins` empty. Of these only `route_prefix` and `allowed_origins` influence behaviour.

### FR-006 Errors

`StreamExtensionError` renders `{"error": msg}`: `AnalysisError` 400, `ResponseError` 500, `StreamNotFound` 404 (variant never constructed).

### FR-007 Response helper

`PjsResponseExt::pjs_stream` for `Json<serde_json::Value>` returns `application/pjs-stream` with the raw JSON text when the request carries `PjsStreamingRequest`, else a normal JSON response. The `pjs_endpoint!` macro wraps handlers with it.

## Non-Functional Requirements

- NFR-SEC-001: no wildcard CORS unless the operator sets it (issue #403).
- NFR-TEST-001: `tests/http_axum_extension_comprehensive.rs` (29 tests) covers config, health, prefix, middleware marker, stream creation, SSE content type and CORS behaviour.

## Data Model

`HttpExtensionConfig`, `PjsExtension { config, streamer: Arc<PriorityStreamer> }`, `PjsStreamingRequest { enabled }`, `StreamRequest`, `StreamResponse`, `StreamExtensionError`, trait `PjsResponseExt`.

## Edge Cases and Errors

| Case | Behaviour |
|---|---|
| `analyze` fails | 400 `Analysis error` |
| SSE for any `stream_id` | same sample payload; id ignored |
| `allowed_origins` invalid | no CORS, error logged |
| Non-JSON body | axum `Json` rejection |

## Success Criteria

- Default mount adds no `Access-Control-Allow-Origin` to any PJS route.
- `/pjs/health` answers 200 under a custom prefix.

## Agent Boundaries

- Always: treat this module as a scaffold; do not rely on it for real data.
- Ask first: connecting it to `PjsAppState` or a stream store.
- Never: reintroduce a hard-coded CORS wildcard.

## Open Questions

- POST `/stream` stores nothing ("In production, this would use a proper store"); `GET /stream/{id}/sse` returns fixed sample products, so the two endpoints are unrelated.
- `handle_sse_stream` uses `.expect(...)` on frame serialization in non-test code (justified by `JsonData` rejecting NaN/Infinity; still a panic path against the project rule).
- The `pjs_endpoint!` macro moves `req` into the handler and then uses `&req`; it appears not to compile when instantiated [NEEDS CLARIFICATION: no test instantiates it].
- Unused config fields: `auto_detect`, `default_priority`, `max_streams_per_client`, `session_timeout`.
- Duplicate health payload shapes: `/pjs/health` here vs `system_health` in [[060-http-rest-api/spec|060]] differ (`service`, `capabilities`).
- `StreamExtensionError::StreamNotFound` unused.

## See Also

- [[063-http-streaming-negotiation/spec|063]] for the RFC-conformant negotiation this module does not use.
