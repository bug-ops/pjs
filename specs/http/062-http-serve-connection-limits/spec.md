---
aliases:
  - serve_with_limits
  - HTTP connection limits
tags:
  - sdd
  - spec
  - http
  - security
  - dos
created: 2026-11-15
status: implemented
priority: P2
issue: "#523"
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
  - "[[061-http-middleware-stack/spec|HTTP middleware stack]]"
  - "[[037-graceful-shutdown-serve-with-limits/spec|Graceful shutdown (open)]]"
  - "[[066-websocket-server-transport/spec|WebSocket transport]]"
code:
  - crates/pjs-core/src/infrastructure/http/serve.rs
  - crates/pjs-core/tests/http_serve_connection_limits.rs
verified: 2026-11-15
---

# Connection-level protection: `serve_with_limits`

## Overview

**Problem.** Tower layers cannot detect a client that never finishes its headers or stops reading the response: hyper stops polling the body when its write buffer fills, so poll-driven timeouts never fire (documented on `RESPONSE_BODY_IDLE_TIMEOUT`, issues #515, #523, #525, #526).

**Goal.** `serve_with_limits(listener, router, ConnectionLimits)` replaces `axum::serve` and owns the accept loop and connection lifetime.

**Out of scope.** Graceful shutdown (open: [[037-graceful-shutdown-serve-with-limits/spec|037]], issue #590); TLS; request header size cap (open issue #618).

Feature gate: `http-server`.

## User Stories

- As an operator I want slow-header and non-reading clients dropped without hand-rolling an accept loop.
- As a WebSocket host I want to disable the connection-duration ceiling.
- As an operator behind a proxy I want to disable the per-IP connection cap.

## Functional Requirements

### FR-001 Limits and defaults

`ConnectionLimits` (`#[non_exhaustive]`, `Default`) shall hold:

| Field | Default | Meaning |
|---|---|---|
| `header_read_timeout: Option<Duration>` | 10 s | HTTP/1 header deadline, plus the preface `peek` wait for the first byte (h1 and h2) |
| `max_connection_duration: Option<Duration>` | 300 s | Hard lifetime ceiling per connection, including upgraded protocols |
| `max_connections: usize` | 1024 | Global connection semaphore, acquired before `accept()` |
| `max_concurrent_streams: Option<u32>` | `Some(128)` | HTTP/2 stream cap; `None` leaves hyper's default (200) |
| `max_connections_per_ip: Option<usize>` | `Some(64)` | Accept-level per-source-IP cap; `None` constructs no limiter |

### FR-002 Serving behaviour

The function shall: build a hyper-util `auto::Builder` with Tokio timers; set the HTTP/2 keep-alive PING interval 30 s and timeout 20 s (constants); serve each connection on its own task via `serve_connection_with_upgrades` (so WebSocket upgrades work); inject `ConnectInfo<SocketAddr>` with the peer address into each request; run until the process stops (no exit path today).

### FR-003 Accept loop

`accept` errors shall not end the loop: `ConnectionRefused`/`ConnectionAborted`/`ConnectionReset` retry immediately; other errors log and retry after 1 s. A connection whose per-IP guard returns `ConnectionLimitExceeded` is dropped immediately with a warning; any other limiter error (such as `CapacityExceeded`) admits the connection (fail open), since `max_connections` already bounds the worst case.

### FR-004 Per-IP keying

Accept-level per-IP keys shall be: IPv4 unchanged; IPv4-mapped IPv6 unwrapped to IPv4; IPv6 loopback unchanged; any other IPv6 masked to its /64 prefix. This differs deliberately from `RateLimitMiddleware`, which keys on the exact IP. The per-IP limiter is a private `ClientRateLimiter` (cleanup task every 60 s), never shared with the request-level middleware.

### FR-005 Zero-byte clients

When `header_read_timeout` is `Some`, a connection that sends no byte within it shall be dropped (`TcpStream::peek`), closing the gap before hyper's version sniffing. A client that sends one byte then stalls is only bounded by `max_connection_duration`.

### FR-006 Duration ceiling

When `max_connection_duration` is `Some`, the connection future shall be cancelled at the deadline; this also terminates stalled response readers and any WebSocket on that connection.

## Non-Functional Requirements

- NFR-SEC-001: worst-case in-flight streams bounded by `max_connections * max_concurrent_streams`, then by `MAX_CONCURRENT_REQUESTS` (512) in [[061-http-middleware-stack/spec|061]].
- NFR-SEC-002: with `header_read_timeout: None` and `max_connection_duration: None`, a silent connection holds its slots indefinitely; at least one should stay `Some` (documented).
- NFR-TEST-001: `tests/http_serve_connection_limits.rs` (9 tests) covers header timeout, duration ceiling, stalled body reader, zero-byte clients, per-IP cap, permit release after per-IP rejection, global backpressure, `ConnectInfo` availability, normal request.

## Data Model

`ConnectionLimits` (public), private constants `H2_KEEP_ALIVE_INTERVAL` (30 s), `H2_KEEP_ALIVE_TIMEOUT` (20 s); helpers `accept_rate_limit_key`, `accept_with_retry`, `is_connection_error`. Uses `security::rate_limit::{ClientRateLimiter, RateLimitConfig, RateLimitError, RateLimitGuard}`.

## Edge Cases and Errors

| Case | Behaviour |
|---|---|
| >64 connections from one IP | extras closed immediately, warning logged |
| Pool full (1024) | accept loop blocks on the semaphore (backpressure) |
| EMFILE or transient accept error | log, 1 s backoff, continue |
| Dual-stack IPv4 peer | keyed as IPv4, not `::/64` |
| Reverse proxy | all connections share the proxy IP; set `max_connections_per_ip: None` |
| WebSocket listener | set `max_connection_duration: None` or sessions die at 300 s |
| Per-connection error | logged at debug, loop continues |

## Success Criteria

- Stalled-header, silent and non-reading clients are closed within their configured deadlines.
- Handlers can extract `ConnectInfo<SocketAddr>` without `into_make_service_with_connect_info`.

## Agent Boundaries

- Always: keep the per-IP limiter private to this function.
- Ask first: changing defaults; adding a shutdown path (see 037).
- Never: share the accept-level limiter with request middleware.

## Open Questions

- No graceful shutdown or cancellation token: the function returns only if the semaphore closes (never) ([[037-graceful-shutdown-serve-with-limits/spec|037]], #590).
- Residual: single-byte-preface stall and `permit` acquired before the per-IP check (documented in code); no explicit request header size cap (#618).
- `ConnectionLimits` is not part of `config::` validation (`PjsConfig`); values are code-only [NEEDS CLARIFICATION: confirm no config-file binding].
- The doc names `/pjs/ws/{session_id}` as the crate's upgrade route, but `create_websocket_router` mounts `/ws`; see [[066-websocket-server-transport/spec|066]].

## See Also

- [[061-http-middleware-stack/spec|061]], [[037-graceful-shutdown-serve-with-limits/spec|037]]
- Demo servers bind `127.0.0.1` and share one per-IP budget.
