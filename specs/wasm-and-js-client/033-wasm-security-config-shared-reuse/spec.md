---
aliases:
  - WASM SecurityConfig Reuse
  - SecurityConfig Shared Instance Bug
tags:
  - sdd
  - spec
  - wasm
  - security
  - bug
created: 2026-08-18
status: implemented
priority: P1
issue: "#404"
pr: "#407"
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
code:
  - crates/pjs-wasm/src/security.rs
  - crates/pjs-wasm/src/parser.rs
  - crates/pjs-wasm/src/streaming.rs
  - crates/pjs-wasm/src/lib.rs
  - crates/pjs-wasm/README.md
verified: 2026-11-15
---

# Feature: SecurityConfig Shareable Between PjsParser and PriorityStream in pjs-wasm

> [!info] Verification (2026-11-15)
> **State: implemented** (Option A, API fix by reference).
>
> **Evidence**
> - PR #407 (`fix(wasm): take SecurityConfig and PriorityConfigBuilder by reference`) closed issue #404. CHANGELOG entry cites #404.
> - `PjsParser::with_security_config(security_config: &SecurityConfig) -> Self` (`#[wasm_bindgen(js_name = withSecurityConfig)]`) clones internally; `PriorityStream::set_security_config(&mut self, config: &SecurityConfig)` (`js_name = setSecurityConfig`) clones internally. The rustdoc on the latter states the instance can also be applied to a `PjsParser`.
> - `crates/pjs-wasm/README.md` "Security Limits" and the `lib.rs` module docs now say both calls take the config by reference and share one instance; the `setMax*` builder methods still consume and return a new instance, so the config must be fully built before sharing.
> - Test: `streaming.rs` `test_security_config_shared_between_parser_and_stream`; parser tests call `PjsParser::with_security_config(&security)`.
> - Generated TypeScript signatures are unchanged (`withSecurityConfig(security: SecurityConfig): PjsParser`, `setSecurityConfig(config: SecurityConfig): void`).
>
> **Drift from original spec**
> - Option A chose `&SecurityConfig` plus an internal clone; no JS-visible `.clone()` method was added (FR-002 satisfied by the by-reference alternative).
> - The same PR also moved `PriorityConfigBuilder` to by-reference (`PjsParser` priority config setter), which the source spec did not cover.
> - Source spec cited `parser.rs:114` and `streaming.rs:204`; those lines shifted.
> - Unrelated to this fix: issue #613 (open) reports that `SecurityConfig` JSON limits `max_array_length` / `max_object_keys` / `max_string_length` in `pjs-core` are never enforced by any parser; the wasm `setMaxArrayElements` / `setMaxObjectKeys` enforcement was fixed separately (CHANGELOG, #355-era fix).
> - Not verified here: a runtime JS test of the exact README snippet under `wasm-pack test` (the sharing test is a native Rust test). [NEEDS CLARIFICATION]

## 1. Overview

### Problem Statement (historical)

`withSecurityConfig` and `setSecurityConfig` took `SecurityConfig` by value. Under wasm-bindgen that moves ownership out of the JS wrapper, so reusing one instance threw `Error: null pointer passed to rust` on the second call, and the README, `lib.rs` and `streaming.rs` examples demonstrated exactly that reuse.

### Goal

A JS caller can apply one `SecurityConfig` to both a `PjsParser` and a `PriorityStream` without an ownership error.

### Out of Scope

- DoS limit enforcement logic.
- Audit of other `#[wasm_bindgen]` by-value setters.
- Rust `Clone` derive (already present).

## 2. User Stories

- US-001: one policy applied to parser and stream without duplicating limit values.
- US-002: the documented example runs when copy-pasted.

## 3. Functional Requirements

| ID | Requirement | Priority | Status |
|----|------------|----------|--------|
| FR-001 | WHEN a `SecurityConfig` is passed to `withSecurityConfig` and then `setSecurityConfig` (or vice versa) THE SYSTEM SHALL NOT throw an ownership error | must | done |
| FR-002 | WHEN a caller needs an independent copy THE SYSTEM SHALL provide one or accept the config by reference | must | done (by reference; internal clone) |
| FR-003 | WHEN a developer reads README, `lib.rs` docs or `set_security_config` docs THE SYSTEM SHALL show a working example | must | done |
| FR-004 | WHEN docs describe `SecurityConfig` THE SYSTEM SHALL state ownership semantics | must | done (README and rustdoc state by-reference sharing; builder `setMax*` methods consume) |

## 4. Non-Functional Requirements

| ID | Category | Requirement | Status |
|----|----------|-------------|--------|
| NFR-001 | Usability | Examples copy-paste runnable | done for docs; no executed JS test found [NEEDS CLARIFICATION] |
| NFR-002 | Compatibility | Single-consumer usage unaffected | done (TS signatures unchanged) |
| NFR-003 | Consistency | All three doc locations updated together | done |

## 5. Data Model

| Entity | Description |
|--------|-------------|
| `SecurityConfig` | `#[derive(Debug, Clone)]` wasm-bound builder: `max_json_size`, `max_depth`, `max_array_elements`, `max_object_keys`; getters `maxJsonSize`, `maxDepth`, `maxArrayElements`, `maxObjectKeys`; defaults `DEFAULT_MAX_*` in `security.rs` |
| `PjsParser` | holds a cloned `security_config` |
| `PriorityStream` | holds a cloned `security_config` (default `SecurityConfig::default()`) |

## 6. Edge Cases and Error Handling

| Scenario | Behaviour |
|----------|-----------|
| One config applied to parser and stream | both succeed; each holds its own clone |
| Config mutated by builder after sharing | the builder methods consume and return a new instance; earlier clones are unaffected |
| Divergent limits per consumer | unchanged; use two instances |
| Never configured | `DEFAULT_MAX_*` limits apply |

## 7. Success Criteria

| ID | Metric | Target | Status |
|----|--------|--------|--------|
| SC-001 | repro script completes | no throw | done by construction (by-reference) |
| SC-002 | doc locations showing by-value reuse | 0 | done |
| SC-003 | doc-tests and rustdoc gate | pass | done in CI |
| SC-004 | existing single-consumer tests | pass | done |

## 8. Agent Boundaries

**Always**: run fmt, clippy, nextest; keep README, `lib.rs` and `streaming.rs` docs consistent.
**Ask first**: changing exported `pjs-wasm` signatures (semver-governed, consumed by `pjs-js-client`).
**Never**: alter limit enforcement as part of ownership fixes.

## 9. Open Questions

- Add a `wasm_bindgen_test` that reuses one config from JS [NEEDS CLARIFICATION].
- Other by-value `#[wasm_bindgen]` parameters outside `SecurityConfig` / `PriorityConfigBuilder` [NEEDS CLARIFICATION: not audited].

## 10. See Also

- [[constitution]], [[MOC-specs]]
- `docs/architecture/WASM_ROADMAP.md`
- Issues #404, #613; PR #407
