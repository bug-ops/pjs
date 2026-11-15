---
aliases:
  - WASM SecurityConfig
  - pjs-wasm DoS limits
tags:
  - sdd
  - spec
  - wasm
  - security
created: 2026-11-15
status: implemented
priority: P1
related:
  - "[[070-wasm-bindings-api/spec]]"
  - "[[033-wasm-security-config-shared-reuse/spec]]"
issue: "#326"
pr: "#404"
code:
  - crates/pjs-wasm/src/security.rs
  - crates/pjs-wasm/src/parser.rs
  - crates/pjs-wasm/src/streaming.rs
  - crates/pjs-wasm/tests/streaming_comprehensive.rs
verified: 2026-11-15
---

# Spec 071: pjs-wasm security limits

Part of [[MOC-specs]]; see [[constitution]] and `SECURITY.md` (section "WASM Security Limits").

> [!info] Verification (2026-11-15)
> Implemented. Evidence: `security.rs` (`SecurityConfig`, `SecurityError`, `validate_*`), CHANGELOG entries for #326 (array/object limits enforced at every depth), #404 (shared `&SecurityConfig`), #473 (depth constant parity test with `pjs-domain`).

## Overview

**Problem.** Untrusted JSON reaches `PjsParser` and `PriorityStream` in the browser or Node.js; unbounded input can exhaust memory.

**Goal.** A JS-configurable `SecurityConfig` with four limits, checked before the expensive `JsonData` conversion, shareable between parser and stream.

**Out of scope.** Server-side limits (`pjs-core` `config/security.rs`), compression-bomb protection, rate limiting.

## Functional Requirements

### FR-001 Defaults
The system shall default to `maxJsonSize` 10 MiB (`DEFAULT_MAX_JSON_SIZE`), `maxDepth` 64, `maxArrayElements` 10 000, `maxObjectKeys` 10 000.
Code: `security.rs` constants and `Default`.
Status: done.

### FR-002 Chainable setters ignore zero
`setMaxJsonSize`, `setMaxDepth`, `setMaxArrayElements`, `setMaxObjectKeys` take and return `self`; a value of `0` leaves the previous value unchanged. Read-only getters `maxJsonSize`, `maxDepth`, `maxArrayElements`, `maxObjectKeys` exist.
Code: `security.rs`.
Status: done. See Open Questions (silent ignore).

### FR-003 Input size check
When input length in bytes exceeds `maxJsonSize`, `parse`, `generateFrames` and `start` shall fail with `SecurityError::InputTooLarge` before JSON parsing. The check is `>`: input exactly at the limit passes.
Code: `security.rs::validate_input_size`; tests `test_validate_input_size_at_limit`, `..._exceeds_limit`.
Status: done.

### FR-004 Structure check at every depth
After parsing, the system shall walk the whole `serde_json::Value` and fail with `ArrayTooLarge` / `ObjectTooLarge` when any array or object (not only the root) exceeds its limit.
Code: `security.rs::validate_json_structure`.
Status: done (#326).

### FR-005 Depth limit truncates, does not reject
`maxDepth` shall bound skeleton construction and field extraction (`frame_generation`); structure deeper than the limit is replaced with `null` / not extracted. `validate_depth` exists but is not called by any entry point.
Code: `frame_generation.rs`; `security.rs::validate_depth` (unused; documented why in its doc comment).
Status: done as documented; `validate_depth` is dead code reserved for a rejecting mode.

### FR-006 Config sharing
`PjsParser.withSecurityConfig(&SecurityConfig)` and `PriorityStream.setSecurityConfig(&SecurityConfig)` shall take the config by reference and clone it, so one JS `SecurityConfig` instance can configure both.
Code: `parser.rs::with_security_config`, `streaming.rs::set_security_config`.
Status: done (#404, see [[033-wasm-security-config-shared-reuse/spec]]).

### FR-007 Error rendering
`SecurityError` (`InputTooLarge`, `MaxDepthExceeded`, `ArrayTooLarge`, `ObjectTooLarge`, each with observed and max values) shall render through `Display` (for example `Input too large: N bytes exceeds limit of M bytes`). `parse` and `generateFrames` prefix it with `Security error: `; `start` emits the bare message to `onError` and in the thrown string.
Code: `security.rs`, `parser.rs`, `streaming.rs`.
Status: done.

## Non-Functional Requirements

- NFR-SEC-001: `DEFAULT_MAX_DEPTH` equals `pjson_rs_domain::MAX_DESERIALIZE_DEPTH` (unit test `test_default_max_depth_matches_domain_guard`).
- NFR-SEC-002: `serde_json` additionally caps parse-time recursion (about 128 levels, per the `validate_depth` doc), so worst-case depth is bounded regardless of `maxDepth`.
- NFR-SEC-003: Coverage target for the wasm component in `.github/codecov.yml` is informational only.

## Data Model

`SecurityConfig { max_json_size, max_depth, max_array_elements, max_object_keys: usize }` (`#[wasm_bindgen]`, `Clone`); `SecurityError` enum above (`Debug, Clone`, implements `std::error::Error`).

## Edge Cases and Errors

| Case | Result |
|------|--------|
| `set_max_*(0)` | ignored |
| Empty string | passes size check, fails with `Parse error` |
| Multi-byte Unicode | size counted in UTF-8 bytes (`str::len`) |
| Huge array nested in small root | rejected (FR-004) |
| Very deep nesting within serde_json recursion cap | accepted and truncated at `maxDepth` |

## Success Criteria

1. `cargo nextest run -p pjs-wasm --tests` covers every `validate_*` function.
2. One `SecurityConfig` passed to both a parser and a stream does not throw "null pointer passed to rust".

## Agent Boundaries

- Always: validate size before parsing; validate structure before `JsonData` conversion.
- Ask first: turning the depth limit into a rejecting limit (JS-visible breaking change, noted in `validate_depth` docs).
- Never: add limits that are accepted but not enforced (see the #326 regression).

## Open Questions

- Zero-ignoring setters hide misconfiguration; a validating constructor would be stricter (project type-safety rule). Limits are raw `usize`, not newtypes.
- No string-length limit exists (compare open core issue #613 about unenforced core limits).
- No limit on total node count; only per-collection counts and total bytes.
- `validate_depth` unused; `SecurityError::MaxDepthExceeded` unreachable from JS.

## See Also

[[070-wasm-bindings-api/spec]] | `SECURITY.md` | `crates/pjs-core/src/config/security.rs`
