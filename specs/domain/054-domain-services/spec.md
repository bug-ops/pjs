---
aliases:
  - Domain Services
  - ValidationService and GatStreamingOrchestrator
tags:
  - sdd
  - spec
  - domain
  - validation
  - orchestrator
created: 2026-11-15
status: partial
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
  - "[[052-domain-gat-ports/spec|Domain GAT ports]]"
  - "[[056-application-queries-dto/spec|Application DTOs]]"
  - "[[013-regex-lazylock-cache/spec|Regex cache (013)]]"
code:
  - crates/pjs-core/src/domain/services/mod.rs
  - crates/pjs-core/src/domain/services/validation_service.rs
  - crates/pjs-core/src/domain/services/gat_orchestrator.rs
  - crates/pjs-core/tests/schema_validation_integration.rs
  - crates/pjs-core/examples/schema_validation_demo.rs
verified: 2026-11-15
---

# Spec 054: Domain Services

> [!info] Verification (2026-11-15)
> **State: partial.** `ValidationService` is implemented and exercised by an example and integration tests; no production code path calls it (`SchemaRepository` only stores schemas and `schema_dto` converts them). `GatStreamingOrchestrator` is implemented and unit tested but has **no caller** in the workspace (only re-exports in `lib.rs`); its frame processing is a simulation. A `connection_manager` service does not exist: `ConnectionManager`/`TimeoutMonitor` and `PriorityService` were removed in PR #254 (issue #251).

## 1. Overview

### Problem
Business logic that spans several values (schema validation, multi-stream orchestration) needs a home that is neither an entity nor an adapter.

### Goal
Document `domain::services::{ValidationService, GatStreamingOrchestrator, GatOrchestratorFactory, OrchestratorConfig, HealthStatus}` as built, including bounds that protect against hostile schemas and data.

### Out of scope
- `Schema`/`JsonData` value objects and `compute_priority` (crate `pjs-domain`).
- Schema registry (`infrastructure/schema_repository.rs`), which stores schemas and does not call the validator.
- Parser-level limits (`config/security.rs`).

## 2. User Stories
- As a library user, I validate a `JsonData` against a `Schema` and get a path-qualified error.
- As an operator, I rely on bounded regex compilation and bounded recursion when schemas come from clients.
- As an integrator, I can run a priority-ordered batch of frames for a session through one orchestrator call.

## 3. Functional Requirements

### FR-001 Schema validation entry point
`ValidationService::validate(&JsonData, &Schema, path: &str) -> SchemaValidationResult<()>` shall validate recursively and return the **first** error. Supported `Schema` variants: `Any`, `Null`, `Boolean`, `Integer`, `Number`, `String`, `Array`, `Object`, `OneOf`, `AllOf`. An unknown (future) variant returns `TypeMismatch { expected: "known schema variant", actual: "unknown" }`.
Code: `validation_service.rs` (`validate`, `validate_with_depth`).

### FR-002 Type and range rules
- `Integer { minimum, maximum }` accepts only `JsonData::Integer`; `Number` accepts `Float` or `Integer` and rejects NaN/infinite with `TypeMismatch { expected: "finite number" }`.
- Violations return `OutOfRange` (bounds rendered as strings, `∞`/`-∞` for missing).
Code: `validate_integer`, `validate_number`.

### FR-003 String rules
`String` shall enforce, in this order: `min_length`/`max_length` over `chars().count()` (`StringLengthConstraint`), `allowed_values` (`InvalidEnumValue`), then `pattern`.
With feature `schema-validation`: patterns longer than `max_pattern_length` (default 1024 bytes, builder ceiling 8 KiB) return `PatternTooLong`; compile errors return `InvalidPattern`; non-matches `PatternMismatch`. Regexes are compiled with `size_limit` 1 MiB and `dfa_size_limit` 64 KiB and cached in a `DashMap` capped at 64 entries (flush-all eviction when full; overshoot by the number of racing threads is accepted). PR #427.
Without feature `schema-validation`: `pattern` is ignored and validation succeeds.
Code: `validate_string`.

### FR-004 Array rules
`Array` shall enforce `min_items`/`max_items` (`ArraySizeConstraint`), `unique_items` via `HashSet` of `JsonData` (`DuplicateItems`), then validate every element against `items` with path `"{path}[{i}]"`.

### FR-005 Object rules
`Object` shall check `required` first (`MissingRequired`), then each present key: known keys validate against their property schema with path `"{path}/{key}"`; unknown keys fail with `AdditionalPropertyNotAllowed` when `additional_properties == false`, otherwise are accepted unvalidated.

### FR-006 Combinators
`OneOf` shall succeed only if exactly one sub-schema validates (`NoMatchingOneOf` for zero or more than one, early exit at the second match). `AllOf` shall evaluate every sub-schema and fail with `AllOfFailure { failures: "0, 2" }` listing failing indices.

### FR-007 Depth bound
Validation shall carry a depth counter incremented per nested array item, object property and combinator branch; when `depth > max_depth` (default 32, `with_max_depth` to change) it returns `TypeMismatch { expected: "maximum depth not exceeded", .. }`.
Code: `DEFAULT_MAX_DEPTH`.

### FR-008 Orchestrated priority streaming
`GatStreamingOrchestrator<R, P>::stream_with_priority(session_id, stream_id, frames)` shall (1) fail with `DomainError::SessionNotFound` if `find_session` returns `None`, (2) sort frames by priority descending and process them in batches of `batch_size` (doubled when the session has more than 5 streams), counting frames and estimated JSON bytes, (3) publish one `StreamCompleted` event, (4) return `StreamingStats { frames_processed, bytes_written, processing_time, cache_hits, cache_misses }`.
Status: implemented as a **simulation**: frames are not written to any sink, the priority boost (`>= priority_boost_threshold` becomes `CRITICAL`) only affects a debug log, and `cache_hits`/`cache_misses` are always 0.
Code: `gat_orchestrator.rs` (`stream_with_priority`, `process_frames_optimized`).

### FR-009 Concurrent orchestration
`process_concurrent_streams(Vec<(SessionId, StreamId, Vec<Frame>)>)` shall take at most `max_concurrent_streams` entries (default 10), drive them with `try_join_all`, and fail fast on the first error. Entries beyond the cap are dropped with a `warn!` and no error.

### FR-010 Orchestrator health
`health_check()` shall call `find_active_sessions`, publish a probe `StreamCompleted` event with random ids, and return `HealthStatus { active_sessions, repository_latency, event_publisher_latency, total_latency, status }` with `status == "healthy"` iff repository latency < 100 ms and publisher latency < 50 ms, else `"degraded"`.

### FR-011 Construction
`GatStreamingOrchestrator::new(Arc<R>, Arc<P>, OrchestratorConfig)`, `with_default_config`, and `GatOrchestratorFactory::{create, create_default}` shall build the service. `OrchestratorConfig::default()`: `batch_size` 100, `cache_ttl` 300 s, `max_concurrent_streams` 10, `priority_boost_threshold` `Priority::HIGH`.

## 4. Non-Functional Requirements
- NFR-SEC-001: regex cache holds at most ~64 compiled patterns; single pattern bytes bounded (FR-003); tests `test_regex_cache_stays_bounded_across_many_patterns`, `test_validate_string_pattern_too_long_rejected`.
- NFR-SEC-002: recursion depth bounded (FR-007); test `test_validate_max_depth_exceeded`.
- NFR-PERF-001: path strings reuse one buffer per array/object level; the regex cache avoids per-call compilation (see [[013-regex-lazylock-cache/spec|013]]); the cache guard is released before `is_match`.

## 5. Data Model
`ValidationService { max_depth, max_pattern_length*, regex_cache* }` (`*` only with `schema-validation`). Errors: `SchemaValidationError` variants `TypeMismatch`, `MissingRequired`, `OutOfRange`, `StringLengthConstraint`, `PatternMismatch`, `InvalidPattern`, `PatternTooLong`, `ArraySizeConstraint`, `DuplicateItems`, `InvalidEnumValue`, `AdditionalPropertyNotAllowed`, `NoMatchingOneOf`, `AllOfFailure`.

## 6. Edge Cases and Errors

| Case | Result |
|---|---|
| `Integer` schema given `Float(1.0)` | `TypeMismatch` |
| `Number` schema given NaN/inf (direct `JsonData::Float`) | `TypeMismatch` |
| pattern longer than limit | `PatternTooLong`, regex not compiled or cached |
| `OneOf` with two matching branches | `NoMatchingOneOf` |
| orchestrator, unknown session | `DomainError::SessionNotFound` |
| orchestrator, more than `max_concurrent_streams` inputs | excess silently dropped |

## 7. Success Criteria
- `cargo nextest run -p pjson-rs validation_service gat_orchestrator` and `tests/schema_validation_integration.rs` pass.

## 8. Agent Boundaries
- Always: bound every recursion and cache reachable from client-supplied schemas; keep `ValidationService` free of I/O.
- Ask first: changing error variants (public, used by `ValidationErrorDto`); wiring the orchestrator into HTTP/WebSocket.
- Never: compile client regexes without the size limits.

## 9. Open Questions
- `ValidationService` has no production caller (no HTTP endpoint or handler validates data against a stored schema); `ValidationRequestDto`/`ValidationResultDto` exist without a consumer. [NEEDS CLARIFICATION: intended wiring.]
- `GatStreamingOrchestrator` has no caller and simulates processing. [NEEDS CLARIFICATION: wire to a real sink or remove before v1.0.0.]
- `health_check` publishes a synthetic `StreamCompleted` event to the real publisher (random `SessionId`/`StreamId`), which pollutes subscribers and event logs.
- `HealthStatus.status` is a `String`; should be an enum. `StreamingStats` has dead `cache_*` fields; `OrchestratorConfig.cache_ttl` is unused.
- `process_concurrent_streams` truncates silently.
- Without `schema-validation`, `Schema::String.pattern` is silently ignored (validation passes), which is a security-relevant default for schemas that rely on patterns.
- `ValidationService::with_max_depth` has no upper bound.
- Depth violation is reported as `TypeMismatch` with a synthetic `expected` text; there is no dedicated variant.
- The `connection_manager` named in earlier planning does not exist (removed in #254).

## 10. See Also
[[054-domain-services/plan|plan]], [[056-application-queries-dto/spec|056]] (`ValidationErrorDto`), `crates/pjs-core/src/infrastructure/schema_repository.rs`.
