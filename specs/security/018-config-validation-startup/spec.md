---
aliases:
  - PjsConfig Validation
tags:
  - sdd
  - spec
  - config
created: 2026-04-28
status: partial
priority: P3
issue: "#175"
pr: "#205"
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
code:
  - crates/pjs-core/src/config.rs
  - crates/pjs-core/src/config/security.rs
  - crates/pjs-core/src/lib.rs
verified: 2026-11-15
---

# Feature: PjsConfig Startup Validation

> [!info] Verification (2026-11-15)
> **State: partial.** `PjsConfig::validate()` and sub-config validators exist and are tested. Entry points do not call it (no component consumes `PjsConfig`), and the non-default presets are not asserted valid by a test.
>
> **Evidence**
> - Issue #175 CLOSED (2026-04-28). PR #205 (`1f1f90e`, "add PjsConfig::validate() and ApiKeyAuthLayer"); CHANGELOG 0.5.2 (closes #175).
> - `crates/pjs-core/src/config.rs`: `ConfigError { MustBePositive { section, field }, InconsistentBounds { section, message } }` (thiserror, `&'static str` fields); `PjsConfig::validate()` chains `StreamingConfig`, `ParserConfig`, `SimdConfig`, `SecurityConfig`, `CompressionConfig` validators.
> - Beyond the spec: sealed trait `Validate` with `validated()` and the `Validated<T>` proof wrapper (`ValidatedSecurityConfig`, `ValidatedCompressionConfig`), later hardening (#568); `SecurityConfig::validate` in `config/security.rs` checks session-id bounds and `max_depth <= MAX_SAFE_JSON_DEPTH`.
> - Checks present: `streaming.max_frame_size > 0`, `streaming.operation_timeout_ms > 0`, `parser.max_input_size_mb > 0`, `parser.buffer_initial_capacity > 0`, `simd.avx512_alignment` non-zero and power of two.
> - Tests: `test_pjs_config_default_validates`, `test_streaming_rejects_zero_max_frame_size`, `test_streaming_rejects_zero_operation_timeout_ms`, `test_simd_rejects_non_power_of_two_alignment`, `test_validated_presets_and_defaults_are_valid` (security presets only).
> - `rg PjsConfig crates` hits only `config.rs`, `lib.rs` (re-export) and a doc comment in a test file: nothing constructs `PjsConfig` outside its own tests; `pjs-demo` does not use it.
>
> **Drift from original spec**
> - Error type is `ConfigError` (two variants), not `PjsConfigError::InvalidField`.
> - Fields `max_concurrent_streams`, `priority_levels` do not exist in `PjsConfig`.
> - `StreamingConfig.default_chunk_size`, `max_bandwidth_bps`, `ParserConfig.simd_min_size`, `SimdConfig.batch_size/initial_capacity/vectorized_chunk_size` are not validated.
> - FR-005 (all presets valid, enforced by unit test): `PjsConfig::low_latency/high_throughput/mobile` tests assert field values only, not `validate()`. Not enforced.

## Overview

Invalid config values (`max_frame_size = 0`, `operation_timeout_ms = 0`) were accepted silently. Goal: a synchronous, structured, field-identifying validation step.

## Functional Requirements

### FR-001: `PjsConfig::validate() -> Result<(), ConfigError>`
Status: done.

### FR-002: Minimum invariants
Status: partial (see drift list for unchecked fields).

### FR-003: Structured error naming the field
Status: done (`section` + `field` / `message`; `Display` is human-readable).

### FR-004: Entry points call `validate()` before building the application
Status: open. No entry point consumes `PjsConfig`. [NEEDS CLARIFICATION: is `PjsConfig` meant to be wired into servers, or only a library-user helper?]

### FR-005: All presets satisfy `validate()` by unit test
Status: open for `PjsConfig::{low_latency, high_throughput, mobile}`; done for `PjsConfig::default()` and `ValidatedSecurityConfig` presets.

## Non-Functional Requirements

- No new dependency (hand-written checks, `thiserror`). Status: done.
- Synchronous validation. Status: done.

## Data Model

`PjsConfig { security, compression, parser, streaming, simd }`; `Validated<T>(T)` newtype with private field and `Deref`.

## Edge Cases

| Case | Error |
|------|-------|
| zero size/timeout field | `MustBePositive` |
| `avx512_alignment` not power of two | `InconsistentBounds` |
| `min_session_id_length > max_session_id_length` | `InconsistentBounds` |
| `json.max_depth > MAX_SAFE_JSON_DEPTH` | `InconsistentBounds` |

## Success Criteria

- Each preset validates; invalid values return the named field.

## Agent Boundaries

- Always: add a validator check and test when adding a config field with an invariant.
- Ask first: adopting `garde`/`validator` crates.
- Never: accept raw `SecurityConfig` in consumers where `ValidatedSecurityConfig` is available.

## Open Questions

- `PjsConfig` unused by runtime components (FR-004).
- `PjsConfig` fields are `pub`, so `Validated<PjsConfig>` is not offered (only security/compression have the proof type); illegal states remain constructible.
- Preset gap (FR-005).

## See Also

- [[026-apikey-config-debug-redaction/spec|026 ApiKeyConfig redaction]]
- `crates/pjs-core/src/config.rs`
