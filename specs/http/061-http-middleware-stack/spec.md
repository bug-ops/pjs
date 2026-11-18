---
aliases:
  - HTTP middleware stack
  - HTTP hardening layers
tags:
  - sdd
  - spec
  - http
  - security
  - rate-limiting
  - auth
created: 2026-11-15
status: partial
priority: P2
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
  - "[[060-http-rest-api/spec|HTTP REST API]]"
  - "[[062-http-serve-connection-limits/spec|serve_with_limits]]"
  - "[[007-http-auth-middleware/spec|Auth middleware]]"
  - "[[026-apikey-config-debug-redaction/spec|ApiKeyConfig Debug redaction]]"
code:
  - crates/pjs-core/src/infrastructure/http/axum_adapter.rs
  - crates/pjs-core/src/infrastructure/http/middleware.rs
  - crates/pjs-core/src/infrastructure/http/auth.rs
  - crates/pjs-core/src/security/rate_limit.rs
verified: 2026-11-18
---

# HTTP middleware stack: layers, rate limiting, auth, security headers

> [!info] Status
> `partial`: the wired stack (timeouts, concurrency cap, body limit, CORS, security headers, per-IP rate limit, API-key auth) is implemented. Several public middleware types in `middleware.rs` are not wired into any router (see Open Questions), and `JwtAuthLayer` exists but cannot be passed to the router constructors.

## Overview

**Problem.** A public streaming API needs DoS bounds, abuse limits and optional auth without every embedder re-deriving a tower stack.

**Goal.** `apply_common_layers` applies one fixed, documented layer order to all router variants; opt-in rate limiting and API-key auth compose with it.

**Out of scope.** Connection-level limits ([[062-http-serve-connection-limits/spec|062]]), WebSocket rate limiting ([[066-websocket-server-transport/spec|066]]), Prometheus ([[065-http-prometheus-metrics/spec|065]]).

## User Stories

- As an operator I want requests bounded in time, size and concurrency by default.
- As an operator behind a reverse proxy I want to opt in to `X-Forwarded-For` without exposing a spoofing bypass.
- As an integrator I want API-key auth that does not leak key length or index through timing.

## Functional Requirements

### FR-001 Layer order

`apply_common_layers` shall apply layers so that, from outermost to innermost: `TraceLayer`, `TimeoutLayer` (408), `ResponseBodyTimeoutLayer`, CORS, `DefaultBodyLimit`, `security_middleware`, optional rate limit, `GlobalConcurrencyLimitLayer`, then the routes (with the auth layer innermost on protected routes only). Axum applies the last `.layer()` call outermost. Rate limit therefore sits inside CORS/security/trace (a 429 keeps security headers, CORS headers and traces) and outside the concurrency limiter (a 429 never consumes a permit). Code: `apply_common_layers`.

### FR-002 Limits

| Constant | Value | Effect |
|---|---|---|
| `MAX_CONCURRENT_REQUESTS` | 512 | One shared semaphore (`GlobalConcurrencyLimitLayer`, deliberately not per-route `ConcurrencyLimitLayer`) bounding handler execution; excess requests wait and are bounded by the 30 s timeout |
| `REQUEST_TIMEOUT` | 30 s | `TimeoutLayer::with_status_code(408)`; covers the time to produce a `Response`, not the streaming body |
| `RESPONSE_BODY_IDLE_TIMEOUT` | 60 s | Resets on each polled frame; guards producer stalls only, not slow readers |
| `DefaultBodyLimit` | 10 MiB | Request body cap |

The concurrency permit is released when the handler returns its `Response`, so it does not bound concurrently open streaming bodies.

### FR-003 Security headers

`security_middleware` shall add `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY` and `Content-Security-Policy: default-src 'self'` to every response (overwriting existing values). No HSTS or `Referrer-Policy`.

### FR-004 Rate limiting

When a `RateLimitMiddleware` is supplied, the stack shall key requests by client IP via `ConnectInfo<SocketAddr>` (canonicalised with `to_canonical`), consult `ClientRateLimiter::check_request`, and:

- on success add `X-RateLimit-Limit`, `X-RateLimit-Remaining`, `X-RateLimit-Reset` (epoch seconds) to the response;
- on `LimitExceeded` return `429` with JSON `{"error":"Too Many Requests","message":..,"retry_after":N}`, `Retry-After: N` (seconds, rounded up, minimum 1) and the `X-RateLimit-*` headers;
- on `CapacityExceeded` (tracked-client table full, `MAX_TRACKED_CLIENTS` = 100 000) return `503 Service Unavailable` with `Retry-After` equal to the cleanup interval (300 s) and without `X-RateLimit-*` headers.

`RateLimitMiddleware::new` takes `security::RateLimitConfig` (only `max_requests_per_window` 100 and `window_duration` 60 s apply to HTTP); there is no separate HTTP `RateLimitConfig` (removed in #704) and the 60 s default window lives only in `security`. `RateLimitMiddleware::new` spawns a periodic cleanup task (default 300 s) when inside a Tokio runtime; `from_limiter` shares an existing `Arc<ClientRateLimiter>`.

### FR-005 Trusted proxies

By default `X-Forwarded-For` and `X-Real-IP` shall be ignored. When `TrustedProxyConfig { trusted_proxies }` is set through `RateLimitMiddleware::with_trusted_proxies` (the only way to configure proxies) and the TCP peer is in the allow-list, the limiter shall walk `X-Forwarded-For` right to left (all header lines joined), return the first entry not in the allow-list, stop at the first unparseable entry, then fall back to `X-Real-IP`, then to the peer address. Without `ConnectInfo` every client maps to `127.0.0.1` and a warning is logged once.

### FR-006 API-key authentication

`ApiKeyAuthLayer::new(ApiKeyConfig)` shall accept a request only if its token matches a configured key. Token sources, first match wins: `Authorization: Bearer <token>`, then `X-PJS-API-Key`. Keys are stored as HMAC-SHA256 tags under a per-process random 32-byte key; comparison tags the candidate and ORs `ConstantTimeEq` results over the whole list (no early exit, fixed 32-byte width). Failure returns `401` with JSON `{"error":"Unauthorized"}`. `OPTIONS` requests pass through unauthenticated. `ApiKeyConfig::new(&[&str])` errors: `EmptyKeyList`, `WhitespaceInKey`, `RngFailure(getrandom::Error)`. `ApiKeyConfig`'s `Debug` redacts keys and the HMAC seed.

### FR-007 JWT authentication (feature `http-auth-jwt`)

`JwtAuthLayer<C>` shall validate `Authorization: Bearer <jwt>` with a caller-supplied `jsonwebtoken` `DecodingKey` and `Validation`; claims are discarded; failure returns the same 401 body; `OPTIONS` passes. Status: implemented as a standalone tower layer; not accepted by `create_pjs_router_with_auth*` (typed to `ApiKeyAuthLayer`), so it must be applied manually.

## Non-Functional Requirements

- NFR-SEC-001: auth comparison is constant-time over key count and length (design in `auth.rs`; tests in `auth.rs` unit tests).
- NFR-SEC-002: rate-limit bypass via spoofed headers is closed by default (`tests/http_rate_limiting_integration.rs`: `test_rate_limit_ignores_x_forwarded_for_by_default`, `..._uses_rightmost_untrusted_entry`).
- NFR-PERF-001: HS256 is recommended over RS256 for JWT at high request rates (documented, no bench).
- NFR-REL-001: the 429/503 responses do not echo attacker-controlled input.

## Data Model

`TrustedProxyConfig`, `RateLimitMiddleware`/`RateLimitService`, `ApiKeyConfig`, `ApiKeyAuthLayer`/`ApiKeyAuthService`, `AuthConfigError`, `JwtConfig { decoding_key, validation }`, `JwtAuthLayer<C>`; core limiter types `ClientRateLimiter`, `RateLimitGuard`, `RateLimitError { LimitExceeded, ConnectionLimitExceeded, FrameTooLarge, CapacityExceeded }` in `security::rate_limit`.

## Edge Cases and Errors

| Case | Result |
|---|---|
| Rate limit exceeded | 429 + `Retry-After` |
| Limiter map full | 503 + `Retry-After` 300 |
| Missing/invalid token | 401 JSON |
| `OPTIONS` to protected route | forwarded to inner service (CORS answers earlier) |
| Unauthenticated flood | still consumes rate-limit quota (rate limit is outside auth; intentional, documented) |
| Sub-second window | `Retry-After`/`X-RateLimit-Reset` round up, never 0 |

## Success Criteria

- A wired router returns security headers on 2xx, 4xx, 429.
- 513th concurrent in-handler request waits; a request waiting more than 30 s gets 408.

## Agent Boundaries

- Always: keep `security_middleware` and the rate limit inside CORS and outside the concurrency limiter.
- Ask first: reordering layers; trusting additional headers; adding public routes.
- Never: add a path-string bypass inside an auth layer; log token material.

## Open Questions

- Unwired public items in `middleware.rs`: `PjsMiddleware` (adds `X-PJS-Duration-Ms`, `X-PJS-Version`, `X-PJS-Compression`, rejects large `Content-Length`), `compression_middleware` (header hint only, no compression), `health_check_middleware`, `content_validation_middleware`/`ContentValidationConfig`. They are referenced only by tests (`tests/http_middleware_comprehensive.rs`, `http_middleware_tests.rs`) and never by `apply_common_layers`.
- `CircuitBreakerMiddleware` stores `failure_threshold` and `recovery_timeout_seconds` but implements neither `Layer` nor `Service`; it has no behaviour.
- `websocket_upgrade_middleware` answers any `Upgrade: websocket` request with `501 "WebSocket support coming soon"`; stale relative to [[066-websocket-server-transport/spec|066]] and unused by routers.
- Stale "Phase" narrative: spec [[007-http-auth-middleware/spec|007]] describes auth as a draft; auth is implemented and wired.
- Rate limiting is opt-in; `create_pjs_router()` and `_with_config` apply none.
- Per-route rate limits or per-key quotas do not exist.

## See Also

- [[062-http-serve-connection-limits/spec|062 serve_with_limits]], [[060-http-rest-api/spec|060 REST API]], `crates/pjs-core/src/config/security.rs` (limits), `SECURITY.md`
- [[050-compression-bomb-and-rate-limiting/spec|050 rate limiting core]], [[051-security-limits-and-validated-config/spec|051 security limits]]
