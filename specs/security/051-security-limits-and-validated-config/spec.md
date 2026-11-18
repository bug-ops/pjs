---
aliases:
  - Security Limits and Validated Config
  - SecurityValidator
  - Validated Config
tags:
  - sdd
  - spec
  - security
  - config
  - type-safety
created: 2026-11-15
status: partial
issue: "#613"
related:
  - "[[050-compression-bomb-and-rate-limiting/spec]]"
  - "[[018-config-validation-startup/spec]]"
  - "[[026-apikey-config-debug-redaction/spec]]"
  - "[[033-wasm-security-config-shared-reuse/spec]]"
code:
  - crates/pjs-core/src/security.rs
  - crates/pjs-core/src/config/security.rs
  - crates/pjs-core/src/config.rs
  - crates/pjs-core/src/parser/zero_copy.rs
  - crates/pjs-core/src/parser/buffer_pool.rs
verified: 2026-11-18
---

# Spec 051: Security Limits and Validated Configuration

[[constitution]] | [[MOC-specs]]

> [!info] Scope
> `pjson_rs::security::{SecurityValidator, DepthTracker}`, `pjson_rs::config::{SecurityConfig, JsonLimits, BufferLimits, NetworkLimits, RateLimitingConfig, SessionLimits, MaxDepth, MAX_SAFE_JSON_DEPTH, Validate, Validated, ValidatedSecurityConfig, ValidatedCompressionConfig, ConfigError}`. Startup validation of the whole `PjsConfig` is [[018-config-validation-startup/spec|spec 018]]. Rate limiting and bomb limits: [[050-compression-bomb-and-rate-limiting/spec|spec 050]]. Status is `partial`: the validated-config typing and depth guards are enforced, but most of the numeric limits are defined and never enforced (#613) and the validator accepts degenerate values (#614).

## Overview

**Problem.** Untrusted JSON, buffers and sessions need bounded resource use, and a misconfigured limit (for example an unbounded depth with a recursion-limit-free parser) must be impossible to construct.

**Goal (as built).** One serde-friendly `SecurityConfig` tree, a validation step that yields a proof type `Validated<T>`, and consumers (`SecurityValidator`, `DepthTracker`, `ZeroCopyParser`, `BufferPool`, `SchemaAnalyzer`/`SchemaCompressor`) that accept only the proof type.

**Out of scope.** `pjs-wasm` has its own `SecurityConfig` ([[033-wasm-security-config-shared-reuse/spec|spec 033]]); API-key handling ([[026-apikey-config-debug-redaction/spec|spec 026]]).

## User Stories

- As an operator, I pick `SecurityConfig::default()`, `high_throughput()`, `low_memory()` or `development()` and tune fields.
- As a library user, I cannot hand a raw, unvalidated config to a consumer: the compiler requires `ValidatedSecurityConfig`.
- As a parser, I get a depth limit that is statically within `1..=512`.

## Functional Requirements

### FR-001 Config tree and defaults
`SecurityConfig { json: JsonLimits, buffers: BufferLimits, network: NetworkLimits, sessions: SessionLimits }` (`Serialize + Deserialize + Default`). Defaults: `json` input 100 MiB, depth 64, 10_000 keys, 1_000_000 array elements, 10 MiB string; `buffers` 256 MiB buffer, pool 1000, 512 MiB total, TTL 300 s, 50 per bucket; `network` 16 MiB WS frame, 10_000 connections, 30 s timeout, 100 req/s, 50 MiB HTTP payload, plus `RateLimitingConfig` and `CompressionBombConfig` defaults; `sessions` id length 8..=128, 100 streams, 3600 s timeout, 100 MiB data. Presets: `high_throughput` (depth 128), `low_memory` (depth 32), `development`.
Status: done.

### FR-002 Depth ceiling type
`MaxDepth::new(n)` shall return `ConfigError::MustBePositive` for 0 and `ConfigError::InconsistentBounds` above `MAX_SAFE_JSON_DEPTH` (512); `MaxDepth::get()` returns the value; `Default` equals `JsonLimits::default().max_depth`. `JiterConfig.max_depth` (`parser/partial.rs`) is a `MaxDepth`.
Status: done (#456, #568).

### FR-003 Validation
`SecurityConfig::validate` shall fail if `min_session_id_length > max_session_id_length`, if `json.max_depth > 512`, if any limit, count, rate or timeout field is zero (including `rate_limiting.max_requests_per_window` and `max_messages_per_second` and the compression-bomb `max_decompressed_size`/`max_compressed_size`), if the compression-bomb `max_ratio` is non-finite or not positive (#627), or if `buffers.max_buffer_size > buffers.max_total_memory`, `network.max_websocket_frame_size > json.max_input_size`, `sessions.max_session_data_size > buffers.max_total_memory`, or the compression-bomb `check_interval_bytes > max_decompressed_size` (which would skip every streaming size check; `InconsistentBounds`). It returns the first error as `ConfigError::{MustBePositive{section, field}, InconsistentBounds{section, message}}`.
Status: done (#614).

### FR-004 Proof type
`Validate::validated(self) -> Result<Validated<Self>, ConfigError>` shall be the only way to build `Validated<T>`; the trait is sealed (implemented for `SecurityConfig` and `CompressionConfig` only); `Validated<T>` has a private field, `Deref<Target = T>`, and `into_inner(self) -> T`. `Default`, `high_throughput`, `low_memory`, `development` exist directly on `ValidatedSecurityConfig`, `Default` on `ValidatedCompressionConfig`.
Status: done.

### FR-005 Consumers require the proof
`SecurityValidator::new`, `DepthTracker::from_config`, `MaxDepth::from(&ValidatedSecurityConfig)` accept only `ValidatedSecurityConfig`; `SchemaAnalyzer::with_config`/`SchemaCompressor::with_config` accept only `ValidatedCompressionConfig`.
Status: done.

### FR-006 Validator methods
`SecurityValidator` shall provide `validate_input_size`, `validate_json_depth(depth)`, `validate_json_depth_bytes(&[u8])`, `validate_array_length`, `validate_object_keys`, `validate_string_length`, `validate_session_id`, `validate_websocket_frame_size`, `validate_buffer_size`; each compares against the matching limit with `>` and returns `crate::Error::LimitExceeded { kind: LimitKind, actual: u64, max: u64 }` on violation (`DepthTracker::enter` included); `validate_session_id` returns `Error::InvalidSessionId(SessionIdViolation::{TooShort, TooLong, InvalidCharacter, Nil})`. `validate_session_id` also requires every char to be ASCII alphanumeric, `-` or `_`.
Status: done.

### FR-007 Byte-level depth scan
`validate_json_depth_bytes` shall scan raw bytes tracking in-string and escape state so brackets inside string literals are not counted, shall fail as soon as the running depth exceeds `max_depth`, and shall saturate (not underflow) on unmatched closers.
Status: done.

### FR-008 Incremental depth tracker
`DepthTracker::enter` shall fail when `current_depth >= max_depth` before incrementing; `exit` saturates at 0; `current_depth()` reads it. Constructors: `from_config(&Validated)`, `with_max_depth(MaxDepth)`, `Default`.
Status: done.

### FR-009 Enforcement points
Production code shall call: `SecurityValidator::validate_input_size`, `validate_json_depth`, `validate_string_length`, `validate_object_keys` and `validate_array_length` from `ZeroCopyParser` (input size once, the others at each string, object and array, #613), and `validate_buffer_size` from `BufferPool`; `BufferPool` also enforces `max_total_memory`.
Status: partial. The remaining unread config fields are listed in Open Questions 1.

### FR-010 Config accessors
`NetworkLimits::connection_timeout`, `BufferLimits::buffer_ttl`, `SessionLimits::session_timeout` shall return `Duration` from the `*_secs` fields.
Status: done.

### FR-011 Serialized-size estimation
Serialized-size estimation shall use one `json_str_len` escape table (and `decimal_digits`) shared by the websocket planner, the websocket client and the compression savings estimate; it shall equal `serde_json` output length for every string (proptest). No limit value changes (#703).
Status: done.

## Non-Functional Requirements

- NFR-SEC-001: a deserialised config with `json.max_depth` above 512 cannot become a `Validated`, so `ZeroCopyParser`/`SecurityValidator` cannot be given it. Tests: `test_rejects_max_depth_above_hard_ceiling`, `test_max_depth_exactly_at_hard_ceiling_validates`, doc-tests on `MaxDepth` and `Validate::validated`.
- NFR-TYPE-001: `Validated<T>` cannot be forged or mutated outside `config.rs` (private field, sealed trait, no `DerefMut`).
- NFR-TEST-001: `security.rs` (26 unit tests), `config/security.rs` (11), `config.rs` (16), `tests/security_comprehensive.rs` (14), `tests/security_bounded_iteration_integration.rs` (17).

## Data Model

Types listed in the Scope callout; `ConfigError { MustBePositive{section, field}, InconsistentBounds{section, message} }` (both `&'static str`); `SecurityValidator { config: SecurityConfig }`; `DepthTracker { current_depth, max_depth: MaxDepth }`.

## Edge Cases and Errors

| Case | Result |
|---|---|
| `max_depth = 0` | `MustBePositive{security.json, max_depth}` |
| `max_depth = 513` | `InconsistentBounds` |
| `min_session_id_length > max` | `InconsistentBounds{security.sessions}` |
| any limit exceeded in a validator method | `Error::LimitExceeded { kind, actual, max }` |
| unmatched `}` in byte scan | depth saturates at 0 |
| session id with other characters | `Error::InvalidSessionId(SessionIdViolation::InvalidCharacter)` |

## Success Criteria

- Config validation and validator tests pass; doc-tests for `MaxDepth`/`validated` pass.

## Agent Boundaries

- Always: take `ValidatedSecurityConfig` (or `MaxDepth`) in any new consumer; add a hard ceiling for any new recursion-driving limit.
- Ask first: changing defaults (behavioural for all users), adding fields to `SecurityConfig` (breaking for struct-literal users).
- Never: add `DerefMut` or a public constructor for `Validated`; read a raw `SecurityConfig` in a consumer.

## Open Questions

1. Not read anywhere in production (by grep over `crates/pjs-core/src`): `network.{max_concurrent_connections, max_requests_per_second, rate_limiting, compression_bomb}`, `sessions.{max_streams_per_session, session_timeout_secs, max_session_data_size}`, `buffers.max_pool_size`, `max_http_payload_size`. `validate_session_id`, `validate_websocket_frame_size`, `DepthTracker` and `validate_json_depth_bytes` have no production callers (CHANGELOG records the removal of the parsers that called the byte scan). `ZeroCopyParser` now enforces `max_array_length`, `max_object_keys` and `max_string_length` (#613).
2. Resolved (#614): `validate` rejects zero for every limit, count and timeout and checks `max_buffer_size <= max_total_memory`, `max_websocket_frame_size <= json.max_input_size` and `max_session_data_size <= max_total_memory`.
3. Resolved by #599: violations are typed `Error::LimitExceeded` / `Error::InvalidSessionId`.
4. `SecurityConfig` and its sub-structs have public fields and `Deserialize`, so invalid values are representable until `validated()` is called; only the wrapper types make them unreachable by consumers.
5. `Validated<T>::into_inner` gives up the proof and the inner value is then mutable; re-validation is the caller's job.
6. `Duration`-like fields are raw `u64` seconds (`*_secs`), sizes raw `usize`.
7. Duplicate rate-limit config (`RateLimitingConfig` vs `RateLimitConfig`) and bomb config embedded but unused: see [[050-compression-bomb-and-rate-limiting/spec|spec 050]].

## See Also

- [[051-security-limits-and-validated-config/plan|plan]], [[018-config-validation-startup/spec|spec 018]], [[050-compression-bomb-and-rate-limiting/spec|spec 050]]
- `SECURITY.md`, `CHANGELOG.md` entries for #456, #549, #568
