---
aliases:
  - Compression Bomb Protection and Rate Limiting
  - ClientRateLimiter
  - CompressionBombDetector
tags:
  - sdd
  - spec
  - security
  - dos
created: 2026-11-15
status: implemented
issue: "#346"
related:
  - "[[049-secure-byte-compression/spec]]"
  - "[[051-security-limits-and-validated-config/spec]]"
  - "[[017-axum-error-http-status-mapping/spec]]"
  - "[[029-websocket-stream-session-binding/spec]]"
code:
  - crates/pjs-core/src/security/compression_bomb.rs
  - crates/pjs-core/src/security/rate_limit.rs
  - crates/pjs-core/src/infrastructure/http/middleware.rs
  - crates/pjs-core/src/infrastructure/http/serve.rs
  - crates/pjs-core/src/infrastructure/websocket/security.rs
verified: 2026-11-18
---

# Spec 050: Compression-Bomb Protection and Per-IP Rate Limiting

[[constitution]] | [[MOC-specs]]

> [!info] Scope
> `pjson_rs::security::{CompressionBombConfig, CompressionBombDetector, CompressionBombProtector, CompressionBombError, CompressionStats}` and `pjson_rs::security::{ClientRateLimiter, RateLimitConfig, RateLimitError, RateLimitGuard, RateLimitStats}`. Validation limits and config newtypes are in [[051-security-limits-and-validated-config/spec|spec 051]]; the codecs that use the bomb guard are in [[049-secure-byte-compression/spec|spec 049]].

## Overview

**Problem.** Attacker-controlled compressed input can expand without bound; attacker-controlled clients can exhaust connections, request budget or tracking memory.

**Goal (as built).** A streaming reader wrapper that aborts decompression on size/ratio/depth limits, and a concurrent per-IP limiter providing request-window, connection-count, message token-bucket and frame-size checks with bounded memory.

**Out of scope.** HTTP status mapping (`RateLimitMiddleware` maps `CapacityExceeded` to 503; see [[017-axum-error-http-status-mapping/spec|spec 017]]), WebSocket session binding, authentication.

## User Stories

- As a decoder, I wrap any `Read` with `protect_reader` and cannot be driven past configured limits.
- As an HTTP/WS server, I call `check_request` / `RateLimitGuard::new` / `check_message` per client IP.
- As an operator, I choose a preset (`high_security`, `low_memory`, `high_throughput`, `high_traffic`, `low_resource`).

## Functional Requirements: compression bomb

### FR-001 Configuration and presets
`CompressionBombConfig { max_ratio: f64, max_decompressed_size, max_compressed_size, max_compression_depth, check_interval_bytes }` (serde `Serialize + Deserialize`). Defaults: ratio 300.0, 100 MiB, 100 MiB, depth 3, interval 64 KiB. Presets: `high_security` (20.0, 10 MiB, 10 MiB, depth 2, 32 KiB), `low_memory` (50.0, 5 MiB, 5 MiB, 2, 16 KiB), `high_throughput` (1000.0, 500 MiB, 500 MiB, 5, 128 KiB).
Status: done.

### FR-002 Pre-decompression gate
`validate_pre_decompression(n)` shall fail with `Error::CompressionBomb(CompressedSizeExceeded { size, max_size })` when `n > max_compressed_size`; it does not look at `max_decompressed_size`.
Status: done.

### FR-003 Streaming protector
`CompressionBombProtector::read` shall add bytes read to the decompressed total and, every `check_interval_bytes`, fail with an `io::ErrorKind::InvalidData` error wrapping the typed `CompressionBombError` (downcastable via `get_ref`) if the total exceeds `max_decompressed_size` or the ratio `decompressed/compressed` (when both non-zero) exceeds `max_ratio`.
Status: done. Overshoot is bounded by one read call plus the interval.

### FR-004 Depth
`with_depth` / `protect_nested_reader` shall return `Error::CompressionBomb(CompressionBombError::DepthExceeded)` when `depth > max_compression_depth`.
Status: done.

### FR-005 Post-hoc validation
`validate_result(compressed, decompressed)` shall fail with `Error::CompressionBomb(SizeExceeded | RatioExceeded)`.
Status: done.

### FR-006 Statistics
`CompressionBombProtector::stats()` shall return `CompressionStats { compressed_size, decompressed_size, ratio (0.0 if compressed size is 0), compression_depth }`; re-exported at the crate root as `BombCompressionStats`.
Status: done.

## Functional Requirements: rate limiting

### FR-007 Configuration
`RateLimitConfig { max_requests_per_window: u32, window_duration: Duration, max_connections_per_ip: usize, max_frame_size: usize, max_messages_per_second: u32, burst_allowance: u32, write_timeout: Duration }`. Defaults: 100 per 60 s, 10 connections, 1 MiB frame, 30 msg/s, burst 5, write timeout 10 s (`DEFAULT_WRITE_TIMEOUT`, defined once in `security::rate_limit`, compiled without the websocket features, and reused by the websocket protocol). `high_traffic`: 1000 / 50 / 100 / 20. `low_resource`: 20 / 2 / 256 KiB / 5 / 2 / 3 s.
Status: done.

### FR-008 Request window
`check_request(ip)` shall keep a sliding window of request timestamps; when the retained count in `window_duration` has reached `max_requests_per_window` it shall return `RateLimitError::LimitExceeded` without recording the request, else record it. `remaining_for` and `reset_after` report the remaining budget and time until the oldest active request leaves the window.
Status: done.

### FR-009 Connections
`check_connection(ip)` shall increment the per-IP connection count or return `ConnectionLimitExceeded`; `close_connection` decrements with saturation. `RateLimitGuard::new` performs `check_connection` and its `Drop` calls `close_connection`, so a connection slot is released on every exit path.
Status: done.

### FR-010 Messages
`check_message(ip, frame_size)` shall reject `frame_size > max_frame_size` with `FrameSizeExceeded`, then consume one token from a token bucket (refill `max_messages_per_second` per second, capacity `max_messages_per_second + burst_allowance`, initial tokens = `burst_allowance`) or return `LimitExceeded` with a 1-second window. For an IP with no tracked entry the token check is skipped.
Status: done.

### FR-011 Bounded tracking
The limiter shall track at most `MAX_TRACKED_CLIENTS` (100_000) IPs. At capacity, `check_request` and `check_connection` for an untracked IP shall return `RateLimitError::CapacityExceeded`; tracked IPs are unaffected (reject-new, never evict-to-admit).
Status: done.

### FR-012 Cleanup
`cleanup_expired` shall remove entries with zero connections whose last request is older than `2 * window_duration`. `spawn_cleanup_task(period)` shall spawn at most one background task per limiter (flag claimed only on success), hold only a `Weak` reference, and degrade to a logged warning outside a Tokio runtime so a later call can retry. Default interval `DEFAULT_CLEANUP_INTERVAL` = 300 s.
Status: done.

### FR-013 Clock-underflow safety
`check_request` and `cleanup_expired` shall use `Instant::checked_sub`; on underflow `check_request` skips trimming (fail closed) and `cleanup_expired` skips the pass.
Status: done.

### FR-014 Statistics
`stats()` returns `RateLimitStats { total_clients, active_clients (connections > 0), total_connections }`.
Status: done.

## Non-Functional Requirements

- NFR-SEC-001: limiter memory is bounded by `MAX_TRACKED_CLIENTS` entries, each holding at most `max_requests_per_window` timestamps.
- NFR-CONC-001: state in a `DashMap`; per-IP updates go through entry locks. No global lock.
- NFR-TEST-001: 36 unit tests in `compression_bomb.rs`, 33 in `rate_limit.rs`; integration: `crates/pjs-core/tests/http_rate_limiting_integration.rs` (18), `websocket_security.rs`, `http_serve_connection_limits.rs`.
- NFR-PERF-001: `[NEEDS CLARIFICATION]` no benchmark for the limiter.

## Data Model

`CompressionBombProtector<R: Read>`, `CompressionBombDetector { config }`, `ClientRateLimiter { config, clients: Arc<DashMap<IpAddr, ClientRateLimit>>, cleanup_spawned: AtomicBool }`, `RateLimitGuard { rate_limiter: Arc<ClientRateLimiter>, client_ip: IpAddr }`.

## Edge Cases and Errors

`RateLimitError`: `LimitExceeded{limit, window}`, `ConnectionLimitExceeded{current, max}`, `FrameSizeExceeded{size, max}`, `CapacityExceeded{max}`. `CompressionBombError`: `RatioExceeded`, `SizeExceeded`, `DepthExceeded`, `CompressedSizeExceeded{size,max_size}`, `TooManyStreams{active,max}`; surfaced as `crate::Error::CompressionBomb(CompressionBombError)`.

| Case | Behaviour |
|---|---|
| IPv6 / IPv4 | keyed by `IpAddr` exactly; no prefix aggregation |
| Compressed size 0 | ratio check skipped |
| `window_duration` larger than host uptime | see FR-013 |
| Limiter dropped | cleanup task exits |

## Success Criteria

- Unit and integration tests above pass; sustained distinct-IP flood leaves the map at or below `MAX_TRACKED_CLIENTS`.

## Agent Boundaries

- Always: release slots through `RateLimitGuard` drop; keep new limits bounded.
- Ask first: switching to evict-to-admit, aggregating IPv6 by prefix, changing the 503 mapping of `CapacityExceeded`.
- Never: use `Instant - Duration` without `checked_sub`; spawn per-call cleanup tasks.

## Open Questions

1. Two parallel configs exist: `security::RateLimitConfig` (used by the limiter) and `config::security::RateLimitingConfig` inside `SecurityConfig.network`; the latter and `NetworkLimits.compression_bomb` are not read by any limiter or detector (grep: no consumers outside `config/`). `[NEEDS CLARIFICATION: intended wiring]`.
2. `CompressionBombConfig` has `validate()` (#627), also run by `SecurityConfig::validate`: it rejects non-finite or non-positive `max_ratio`, zero `max_decompressed_size`/`max_compressed_size` (`ConfigError::MustBePositive`) and `check_interval_bytes > max_decompressed_size` (`ConfigError::InconsistentBounds`). `RateLimitConfig` remains `Deserialize`-able with no validation.
3. Resolved by #599: `CompressionBombError` is carried typed inside `Error::CompressionBomb`.
4. `CompressionBombDetector` has one production caller: `InMemoryDictionaryStore` uses `validate_pre_decompression` as a dictionary size gate. `SecureCompressor` decode paths have none ([[049-secure-byte-compression/spec|spec 049]]).
5. `RateLimitConfig.write_timeout` lives in the rate-limit config but is a WebSocket sink concern; since #703 the WebSocket module reuses `DEFAULT_WRITE_TIMEOUT` instead of an independent constant. The HTTP middleware uses this `RateLimitConfig` directly (the duplicate HTTP type was removed in #704).
6. A `ClientRateLimit.requests` is a `Vec` with `retain` on each request: O(window count) per call.

## See Also

- [[050-compression-bomb-and-rate-limiting/plan|plan]], [[049-secure-byte-compression/spec|spec 049]], [[051-security-limits-and-validated-config/spec|spec 051]]
- `SECURITY.md`; CHANGELOG entry for #346
