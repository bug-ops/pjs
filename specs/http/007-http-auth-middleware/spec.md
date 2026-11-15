---
aliases:
  - HTTP Authentication Middleware
  - Spec 007
tags:
  - sdd
  - spec
  - http
  - security
created: 2026-04-28
status: partial
priority: P3
issue: "#135"
pr: "#205"
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
code:
  - crates/pjs-core/src/infrastructure/http/auth.rs
  - crates/pjs-core/src/infrastructure/http/axum_adapter.rs
  - crates/pjs-core/src/infrastructure/http/mod.rs
  - crates/pjs-core/tests/http_middleware_tests.rs
  - crates/pjs-core/Cargo.toml
verified: 2026-11-15
---

# Spec 007: HTTP Authentication Middleware for PJS Endpoints

> [!info] Verification (2026-11-15)
> **State: partial.** API-key auth is fully implemented; JWT layer exists but diverges from the spec (claims discarded, no router factory accepts it, no tests).
>
> Evidence: issue #135 closed 2026-04-28 (CHANGELOG "closes #135"); PRs #205 (`1f1f90e`, `ApiKeyAuthLayer` + `JwtAuthLayer`), #212 (`b392632`, fixes + tests), #221 (`2f38c2a`, factory integration tests). Code: `infrastructure/http/auth.rs` (`ApiKeyConfig`, `ApiKeyAuthLayer`, `ApiKeyAuthService`, `AuthConfigError`; under `http-auth-jwt`: `JwtConfig`, `JwtAuthLayer<C>`, `JwtAuthService`). Factories in `axum_adapter.rs`: `create_pjs_router_with_auth`, `create_pjs_router_with_rate_limit_and_auth`; both re-exported from `infrastructure::http`. The stale "TODO: Implement authentication" comment was removed (CHANGELOG, "Removed stale TODO comments").
>
> Drift from original spec:
> - Constant-time comparison is stronger than specified: keys are HMAC-SHA256-tagged with a per-process random `hmac_key` (`getrandom`), all 32-byte tags compared with `subtle::ConstantTimeEq`, full list always scanned (no length or key-index leak). Raw keys are not retained.
> - `ApiKeyAuthLayer::new(ApiKeyConfig)` replaces the spec's `new(Arc<str>)`; `ApiKeyConfig::new(&[&str])` rejects an empty list and any key containing ASCII whitespace.
> - 401 body is `{"error":"Unauthorized"}` (capital U), `Content-Type: application/json`.
> - Router factories take the concrete `ApiKeyAuthLayer`, not `impl Layer<...>`; custom/JWT layers must be applied by the caller on their own router. `create_pjs_router` signature unchanged (met).
> - JWT: `JwtAuthLayer` validates `Authorization: Bearer` only (`X-PJS-API-Key` not accepted) and discards decoded claims; it does not forward them as request extensions.
> - `OPTIONS` requests bypass auth (CORS preflight); not in the original spec.
> - Auth wraps only the protected sub-router, so `/pjs/health` (and `/metrics` under `metrics`) stay public.

**Type**: enhancement | **Priority**: P3 | **Status**: partial

## Functional requirements

### FR-001: Ready-to-use auth middleware layerable onto the router
Status: done (`ApiKeyAuthLayer` via `create_pjs_router_with_auth`).

### FR-002: Static API-key middleware
The layer shall read `Authorization: Bearer <token>` (first) or `X-PJS-API-Key: <token>`, trim ASCII whitespace, compare in constant time, and answer `401` with JSON `{"error":"Unauthorized"}` on missing or non-matching token.
Status: done. Tests in `auth.rs`: `valid_bearer_token_returns_200`, `valid_x_pjs_api_key_returns_200`, `wrong_token_returns_401`, `missing_header_returns_401`, `options_bypasses_auth`, `extract_token_bearer_preferred_over_x_pjs`.

### FR-003: Optional JWT middleware (`http-auth-jwt`)
Verify `Authorization: Bearer <jwt>` via `jsonwebtoken`; reject invalid/expired with 401; forward claims as request extensions.
Status: partial. Verification and 401 done (`Validation` supplied by caller decides expiry/audience/algorithm); claims forwarding open. No JWT unit or integration tests found (`rg Jwt crates/pjs-core/tests` is empty; module tests in `auth.rs` cover API key only).

### FR-004: Tower `Layer` composable with rate limit and tracing
Status: done. `ApiKeyAuthLayer: Clone + Layer<S>`; `create_pjs_router_with_rate_limit_and_auth` puts rate limit outside auth (documented trade-off: unauthenticated floods consume rate-limit quota).

### FR-005: Optional auth parameter without breaking `create_pjs_router`
Status: done via separate factories.

### FR-006: `/pjs/health` stays unauthenticated
Status: done (public sub-router merged without the auth layer; covered by `tests/http_middleware_tests.rs`).

## Non-functional requirements

- Constant-time comparison: met (`subtle::ConstantTimeEq`, HMAC tags).
- No heap allocation for token comparison: token is borrowed from the header; each check allocates nothing beyond the HMAC state [NEEDS CLARIFICATION: not benchmarked].
- `jsonwebtoken` only with `http-auth-jwt`: met (`http-auth-jwt = ["http-server", "dep:jsonwebtoken"]`).
- `Clone`: met.

## Open questions

- Should factories accept a generic `Layer` so `JwtAuthLayer` can be used with `create_pjs_router_with_auth`?
- Claims forwarding for JWT, and JWT tests (feature not exercised by default features).
- `ApiKeyAuthService::Future` is a boxed `dyn Future` per request.

## See Also

- `crates/pjs-core/src/infrastructure/http/auth.rs` module docs (security design)
- [[009-realtime-metrics-observability/spec|009]] (`/metrics` is public and unauthenticated)
