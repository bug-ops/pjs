---
aliases:
  - Transport feature flags and limits catalog
tags:
  - sdd
  - spec
  - http
  - websocket
  - features
  - security
created: 2026-11-15
status: implemented
priority: P3
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
  - "[[060-http-rest-api/spec|HTTP REST API]]"
  - "[[061-http-middleware-stack/spec|HTTP middleware stack]]"
  - "[[062-http-serve-connection-limits/spec|serve_with_limits]]"
  - "[[065-http-prometheus-metrics/spec|Metrics]]"
  - "[[066-websocket-server-transport/spec|WebSocket server]]"
  - "[[068-websocket-client/spec|WebSocket client]]"
code:
  - crates/pjs-core/Cargo.toml
  - crates/pjs-core/src/infrastructure/mod.rs
  - crates/pjs-core/src/infrastructure/http/mod.rs
  - crates/pjs-core/src/infrastructure/websocket/mod.rs
  - crates/pjs-core/src/security/rate_limit.rs
  - crates/pjs-core/src/domain/config/limits.rs
verified: 2026-11-15
---

# Transport feature flags and limits catalog

> [!info] Purpose
> One place to see which Cargo features gate which transport code, and every numeric limit in the HTTP and WebSocket layers with its owning file. Behaviour is specified in the linked specs; this note is a lookup table and a drift register.

## Overview

**Problem.** Limits and feature gates are spread over several modules, and README/CLAUDE.md drift from `Cargo.toml`.

**Goal.** A verified catalog that other specs and reviewers can cite.

**Out of scope.** Parser, compression and WASM features.

## User Stories

- As an operator I find every tunable and hard-coded bound in one table.
- As a contributor I know which feature enables which module before editing.

## Functional Requirements

### FR-001 Feature matrix

| Feature | Enables | Notes |
|---|---|---|
| `http-server` (default) | `infrastructure::http` (router, handlers, middleware, `auth`, `serve`, `streaming`, `axum_extension`), deps `axum` (with `ws`), `tokio-util`, `tower`, `tower-http`, `hyper`, `hyper-util`, `subtle`, `hmac`, `sha2`, `getrandom`, `headers-accept`, `mediatype`, `async-stream` | |
| `websocket-server` (default) | `http-server` | Gates `infrastructure::websocket` server module and `SecureWebSocketHandler` (re-exported at crate root); test target `websocket_security` (#598) |
| `websocket-client` (default) | `tokio-tungstenite`, `url`; `websocket::client` | Builds on its own (#598); wire test target requires `websocket-server` + `websocket-client` |
| `http-auth-jwt` | `http-server` + `jsonwebtoken`; `JwtAuthLayer`, `JwtConfig` | Not accepted by router constructors |
| `metrics` | `http-server` + `metrics` + `metrics-exporter-prometheus`; `/metrics` route; `pjs_frames_total` | Sets `feature(once_cell_try)` |
| `compression` (default) | dictionary route `GET /pjs/sessions/{id}/dictionary` (not wasm32) | zstd dictionary training |
| `http-client` | none | Listed in README.md and `.claude/CLAUDE.md`; absent from `Cargo.toml` and code |

### FR-002 HTTP limits

| Limit | Value | Where |
|---|---|---|
| Request body | 10 MiB | `apply_common_layers` (`DefaultBodyLimit`) |
| Request timeout | 30 s -> 408 | `REQUEST_TIMEOUT` |
| Response body idle | 60 s | `RESPONSE_BODY_IDLE_TIMEOUT` |
| In-flight handlers | 512 | `MAX_CONCURRENT_REQUESTS` |
| CORS preflight max-age | 3600 s | `build_cors_layer_from_origins` |
| Session timeout (1..=upper bound, typed `SessionTimeout`) | 604 800 s | `MAX_SESSION_TIMEOUT_SECONDS` |
| Frames per request | 1000 | `MAX_FRAMES_PER_REQUEST` |
| `generate-frames` default frames | 16 | `handlers/streams.rs` |
| Default session streams / timeout | 10 / 3600 s | `handlers/sessions.rs` |
| `Accept` entries parsed | 16 | `MAX_ACCEPT_ENTRIES` |
| Pagination limit / offset | 1000 / 1 000 000 | `MAX_PAGINATION_LIMIT`, `MAX_PAGINATION_OFFSET` (`domain::config::limits`) |
| Adapter scan/results | 10 000 / 10 000 | `infrastructure/adapters/limits.rs` |
| HTTP rate limit | 100 requests / 60 s per IP | `RateLimitConfig` (HTTP) |
| Tracked clients | 100 000 | `MAX_TRACKED_CLIENTS` |
| Limiter cleanup | 300 s (middleware), 60 s (accept-level) | `DEFAULT_CLEANUP_INTERVAL`, `serve.rs` |

### FR-003 Connection limits

See [[062-http-serve-connection-limits/spec|062]]: header timeout 10 s, duration 300 s, 1024 connections, 128 h2 streams, 64 per IP, h2 keep-alive 30 s / 20 s, header block cap off by default (`max_header_bytes`, minimum 8192). Drain timeout on shutdown: 30 s default, 600 s maximum ([[037-graceful-shutdown-serve-with-limits/spec|037]]).

### FR-004 WebSocket limits

| Limit | Value | Where |
|---|---|---|
| Inbound frame/message cap | 1 MiB default; 256 KiB `low_resource` | `RateLimitConfig::max_frame_size` |
| Connections per IP | 10 default; 50 `high_traffic`; 2 `low_resource` | `RateLimitConfig` |
| Upgrade requests per window (60 s) | 100 default; 1000 `high_traffic`; 20 `low_resource` | `RateLimitConfig` |
| Messages per second (+burst) | 30 (+5) default; 100 (+20) `high_traffic`; 5 (+2) `low_resource` | `RateLimitConfig` |
| Write timeout | 10 s default; 3 s `low_resource` | `RateLimitConfig::write_timeout`, `WRITE_TIMEOUT` |
| Outgoing queue (server and client) | 1000 messages, 16 MiB | `OUTGOING_QUEUE_CAPACITY`, `MAX_QUEUED_OUTGOING_BYTES`, `MESSAGE_QUEUE_CAPACITY`, `MAX_QUEUED_MESSAGE_BYTES` |
| Live sessions per connection (server) | 64 default; `429` above | `DEFAULT_MAX_SESSIONS_PER_CONNECTION`, `with_max_sessions_per_connection` |
| Client sessions | 1024 default (`NonZeroUsize`); 16 MiB per session | `WebSocketClientConfig` |
| Session sweep | every 60 s, max age 3600 s | `SESSION_CLEANUP_INTERVAL`, `SESSION_MAX_AGE` |
| Adaptive delay | 10 ms fast; 0.5 x avg ack ms if avg > 100 ms; max 1 s | `ClientMetrics`, `MAX_ADAPTIVE_FRAME_DELAY` |
| Client sessions | 1024; idle TTL 300 s; 16 MiB per session | `WebSocketClientConfig` |

### FR-005 Re-export surface

`infrastructure` (feature `http-server`) re-exports the HTTP items listed in [[060-http-rest-api/spec|060]] plus `ConnectionLimits`, `HeaderSizeLimit`, `serve_with_limits`, `ShutdownConfig`, `ShutdownSignal`, `DrainTimeout`, `ShutdownOutcome`, `RateLimitConfig`, `RateLimitMiddleware`, `TrustedProxyConfig`, `BatchFrameStream`, `StreamFormat`, `StreamTransportError`, `HttpExtensionConfig`, `PjsExtension`, and WebSocket items `AdaptiveStreamController`, `AxumWebSocketTransport`, `ClientMetrics`, `ConnectionId`, `CreateSessionError`, `SecureWebSocketHandler`, `StreamOptions`, `WebSocketStreamSession`, `WebSocketTransport`, `WsMessage`, `create_websocket_router`; with `websocket-client`: `ClosedSession`, `PjsWebSocketClient`, `SessionIdleTtl`, `StreamStats`.

## Non-Functional Requirements

- NFR-DOC-001: README feature table must match `Cargo.toml` (currently violated for `http-client`).
- NFR-TEST-001: `Cargo.toml` test-target entries gate `websocket_security` (`websocket-server`) and `websocket_wire_level` (`websocket-server` + `websocket-client`).

## Data Model

Not applicable; constants and config structs are listed in their owning specs.

## Edge Cases and Errors

| Case | Behaviour |
|---|---|
| Build with `websocket-client` only | builds (#598) |
| Build with `metrics` on stable | fails (`once_cell_try`); project is nightly-only anyway |
| `jemalloc`/`mimalloc` | unrelated to this layer; mutually exclusive per CLAUDE.md |

## Success Criteria

- Every value in FR-002 to FR-004 matches its source constant.

## Agent Boundaries

- Always: update this catalog when changing a limit.
- Ask first: raising any default limit.
- Never: document a feature that `Cargo.toml` does not define.

## Open Questions

- `http-client` documented but nonexistent: either remove from README/CLAUDE.md or implement; no spec covers a Rust HTTP client.
- Duplicate write-timeout constants (`WRITE_TIMEOUT` and `RateLimitConfig::write_timeout`) must be kept in sync by hand.
- Two near-duplicate `RateLimitConfig` types: `security::rate_limit::RateLimitConfig` (limiter) and `infrastructure::http::RateLimitConfig` (HTTP wrapper), and `config::security::NetworkLimits.max_websocket_frame_size` (16 MiB default, used by `security.rs` validation) which is separate from the 1 MiB transport cap [NEEDS CLARIFICATION: confirm which cap governs what].
- No consolidated runtime config object binds these limits (`PjsConfig` validation does not cover them).

## See Also

- `crates/pjs-core/Cargo.toml`, `README.md` feature table, `.claude/CLAUDE.md`
- [[059-crate-api-errors-features/spec|059 crate API, errors, features]], [[051-security-limits-and-validated-config/spec|051 security limits]]
