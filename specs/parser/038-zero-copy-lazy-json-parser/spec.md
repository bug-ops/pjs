---
aliases:
  - Zero-Copy Lazy JSON Parser
  - ZeroCopyParser
tags:
  - sdd
  - spec
  - parser
  - zero-copy
  - security
created: 2026-11-15
status: implemented
priority: P2
code:
  - crates/pjs-core/src/parser/zero_copy.rs
  - crates/pjs-core/src/parser/mod.rs
  - crates/pjs-core/tests/zero_copy_integration_test.rs
  - crates/pjs-bench/benches/zero_copy_bench.rs
  - fuzz/fuzz_targets/parser_zero_copy.rs
related:
  - "[[039-partial-json-jiter-parser/spec]]"
  - "[[040-buffer-pool-aligned-alloc/spec]]"
  - "[[041-simd-features-and-global-allocator/spec]]"
  - "[[006-sonic-rs-primary-parser-path/spec]]"
  - "[[034-fused-utf8-parse-validation/spec]]"
verified: 2026-11-15
---

# Spec 038: Zero-Copy Lazy JSON Parser

Part of [[MOC-specs]]. Rules in [[constitution]]. Plan: [[038-zero-copy-lazy-json-parser/plan|plan]].

> [!info] Scope note
> This is the only validating JSON scanner that remains in `pjson-rs` after the `Parser`/`SimpleParser`/`SonicParser`/`HybridParser` removals (CHANGELOG `[Unreleased]` / Removed, issues #486, #488). The earlier specs that describe those parsers ([[001-simd-backend-implementation/spec|001]], [[006-sonic-rs-primary-parser-path/spec|006]], [[034-fused-utf8-parse-validation/spec|034]]) concern code that no longer exists; see their own verification notes.

## Overview

**Problem.** Streaming code needs to inspect JSON values without allocating a tree. A scanner that returns sub-slices of the input buffer avoids copies and lets callers decide later what to materialise.

**Goal.** `ZeroCopyParser` scans exactly one JSON value from a `&[u8]`, validates its syntax and bounds (input size, nesting depth), and returns a `LazyJsonValue` that borrows from the input wherever no unescaping is needed.

**Out of scope.** Building a DOM (see `JsonData` in [[042-domain-crate-contract-and-value-objects/spec|042]]); partial/truncated input ([[039-partial-json-jiter-parser/spec|039]]); SIMD scanning (the scanner is a plain byte loop); the `sonic-rs` based classifier in `parser::simd` ([[041-simd-features-and-global-allocator/spec|041]]).

## User Stories

- As a library user, I parse a byte slice and read a string field as `&[u8]` without allocation.
- As an operator, I want oversized or deeply nested payloads rejected with a security error before they consume stack or memory.
- As a benchmark author, I measure allocated versus referenced bytes via `MemoryUsage`.

## Functional Requirements

### FR-038-001 Single-value scan

When `LazyParser::parse_lazy(input)` is called, the parser shall reset its position and depth, then parse one JSON value starting at offset 0 after skipping leading whitespace (` `, `\t`, `\n`, `\r`).
Code: `parser/zero_copy.rs` `ZeroCopyParser`, `impl LazyParser for ZeroCopyParser`.
Status: done.

### FR-038-002 Input size bound

When `parse_lazy` receives input longer than `json.max_input_size` of the active `ValidatedSecurityConfig` (default 100 MiB), the parser shall return `DomainError::SecurityViolation` before reading any byte.
Code: `parse_lazy` -> `SecurityValidator::validate_input_size`.
Status: done.

### FR-038-003 Depth bound

When an object or array would open at depth greater than `json.max_depth` (default 64; validated config ceiling `MAX_SAFE_JSON_DEPTH` = 512), the parser shall return `DomainError::SecurityViolation`.
Code: `parse_object`, `parse_array` -> `SecurityValidator::validate_json_depth`.
Status: done.

### FR-038-004 Borrowed versus owned strings

When a string contains no backslash, the parser shall return `LazyJsonValue::StringBorrowed(&[u8])` referencing the input (without the quotes). When it contains a backslash, the parser shall return `LazyJsonValue::StringOwned(String)` with escapes decoded.
Code: `parse_string`, `unescape_string`.
Status: done.

### FR-038-005 Escape decoding

The decoder shall support `\" \\ \/ \b \f \n \r \t` and `\uXXXX`, combine a high/low surrogate pair into one scalar value, and return `DomainError::InvalidInput` for: an unknown escape, a non-hex or short `\u` sequence (including a leading `+`), an unpaired high or low surrogate, a high surrogate followed by a non-low-surrogate, and an invalid scalar value.
Code: `unescape_string`, `parse_hex4`. History: #443.
Status: done.

### FR-038-006 Containers are sliced, not materialised

The parser shall validate an object (string keys, `:` and `,` separators) or array recursively and return `ObjectSlice(&[u8])` / `ArraySlice(&[u8])` covering the whole container text including brackets. It shall not expose children.
Code: `parse_object`, `parse_array`.
Status: done.

### FR-038-007 Scalars

The parser shall return `Boolean(bool)`, `Null`, and `NumberSlice(&[u8])`. Number syntax shall follow RFC 8259 (optional `-`, `0` or non-zero-led digits, optional fraction with at least one digit, optional exponent with at least one digit). Non-matching input shall return `DomainError::InvalidInput`.
Code: `parse_boolean`, `parse_null`, `parse_number`.
Status: done.

### FR-038-008 Accessors on `LazyJsonValue`

`LazyJsonValue` shall provide `value_type() -> ValueType`, `as_str()` (UTF-8 validated at call time, borrowed for `StringBorrowed`), `as_number()` (parses as `f64`), `as_boolean()`, `is_null()`, `as_bytes()` (None for `StringOwned`, `Boolean`, `Null`), `to_string_lossy()`, and `memory_usage() -> MemoryUsage { allocated_bytes, referenced_bytes }` with `total()` and `efficiency()`.
Code: `impl LazyJsonValue`, `MemoryUsage`.
Status: done.

### FR-038-009 Parser state accessors

The parser shall expose `remaining()` (unparsed tail), `is_complete()` (`position >= input.len()`), and `reset()`; `with_security_config(ValidatedSecurityConfig)` shall install custom limits, and `new()`/`Default` shall use `SecurityValidator::default()`.
Status: done.

### FR-038-010 Incremental parser is a stub

`IncrementalParser` shall accept bytes via `feed`, but `parse_available` shall always return an empty `Vec` (it clears the buffer when a value parses and keeps it otherwise) and `has_complete_value` shall return `!buffer.is_empty()`.
Code: `IncrementalParser`.
Status: open (stub, see Open Questions).

## Non-Functional Requirements

- NFR-PERF-038-001: strings without escapes shall not allocate (`MemoryUsage::allocated_bytes == 0`). Test: `tests/zero_copy_integration_test.rs` `test_memory_efficiency_comparison`; bench `crates/pjs-bench/benches/zero_copy_bench.rs`.
- NFR-SEC-038-001: no panic on arbitrary bytes. Evidence: fuzz target `fuzz/fuzz_targets/parser_zero_copy.rs` (CI replays seeds with `-runs=0` and runs a 60 s budget in the fuzz job of `.github/workflows/ci.yml`).
- NFR-SEC-038-002: nesting depth and input size are the only resource bounds enforced; array length, object key count and string length limits from `JsonLimits` are not consulted by this parser.

## Data Model

| Type | Kind | Notes |
|---|---|---|
| `ZeroCopyParser<'a>` | struct | fields: `input`, `position`, `depth`, `validator: SecurityValidator` |
| `LazyParser<'a>` | trait | `Output`, `Error`, `parse_lazy`, `remaining`, `is_complete`, `reset` |
| `LazyJsonValue<'a>` | enum | `StringBorrowed`, `StringOwned`, `NumberSlice`, `Boolean`, `Null`, `ObjectSlice`, `ArraySlice` |
| `MemoryUsage` | struct | `allocated_bytes`, `referenced_bytes` |
| `ValueType` | enum | in `parser/mod.rs`: `Object`, `Array`, `String`, `Number`, `Boolean`, `Null` |
| `IncrementalParser<'a>` | struct | `Vec<u8>` buffer, 8 KiB initial capacity |

Re-exports: `pjson_rs::{LazyParser, ZeroCopyParser}` at crate root; `parser::{IncrementalParser, LazyJsonValue, MemoryUsage}`.

## Edge Cases and Errors

All errors are `pjson_rs::domain::DomainError` (re-export of `pjson_rs_domain::DomainError`).

| Input | Result |
|---|---|
| empty or whitespace only | `InvalidInput("Unexpected end of input")` |
| unterminated string | `InvalidInput("Unterminated string")` |
| trailing backslash inside string | loop exits, `Unterminated string` |
| unexpected leading byte | `InvalidInput("Unexpected character: c")` |
| `tru`, `nul` | `InvalidInput("Invalid boolean value")` / `Invalid null value` |
| `1.`, `1e`, `-` | `InvalidInput` (missing digits / invalid number) |
| nesting deeper than `max_depth` | `SecurityViolation` |
| input larger than `max_input_size` | `SecurityViolation` |
| bytes after the first value | not an error; visible through `remaining()` |
| invalid UTF-8 inside a borrowed string | not detected at parse time; `as_str()` returns `InvalidInput` |
| leading zeros (`01`) | `0` is returned, the `1` remains in `remaining()` |

## Success Criteria

- All unit tests in `zero_copy.rs` and the integration tests pass under `cargo nextest run -p pjson-rs`.
- The fuzz target runs without a crash for 60 s.
- `cargo doc` for the crate has no broken links.

## Agent Boundaries

- Always: keep `unsafe` out of `zero_copy.rs` (none present today); route every new limit through `SecurityValidator`.
- Ask first: changing `LazyJsonValue` variants (public enum, not `#[non_exhaustive]`); making `parse_lazy` reject trailing bytes (behaviour change).
- Never: add a second hand-rolled JSON scanner next to this one (see CHANGELOG precedent: #233, #245, #251, #255, #257, #261).

## Open Questions

- `IncrementalParser::parse_available` is a stub that never yields values, and `has_complete_value` is a non-empty check. It is public API but cannot be used for streaming; test `test_incremental_parsing` only exercises `feed`. Decide: implement, or remove.
- The `PhantomData<&'a ()>` field in `ZeroCopyParser` and `IncrementalParser` is redundant for `ZeroCopyParser` (it already holds `&'a [u8]`) and unused for `IncrementalParser` (its lifetime parameter is not tied to anything).
- `ZeroCopyParser` does not enforce `max_array_length`, `max_object_keys` or `max_string_length` from `JsonLimits`.
- `parse_lazy` ignores trailing garbage after the first value; callers must check `is_complete()`/`remaining()`. Not documented on the trait.
- `parse_string` does not validate UTF-8 or reject raw control characters (< 0x20) inside strings, so it accepts text that RFC 8259 forbids. `NumberSlice` for `01` splits as noted above.
- `LazyJsonValue` is not `#[non_exhaustive]` while most other public enums in the workspace are.
- Errors are `DomainError` string variants (`InvalidInput(String)`), i.e. stringly typed; no dedicated parse-error enum with offsets exists.
- `[NEEDS CLARIFICATION: the claim in the older 006 spec that sonic-rs is the primary parser is now moot; confirm no consumer other than benches/examples/tests uses ZeroCopyParser in production code]`. Verified with `rg`: only `pjs-bench`, `examples/zero_copy_demo.rs`, the fuzz target and tests reference it.

## See Also

- [[038-zero-copy-lazy-json-parser/plan|Plan (as built)]]
- [[040-buffer-pool-aligned-alloc/spec|040 Buffer pool]] (used by the zero-copy demo)
- `crates/pjs-core/examples/zero_copy_demo.rs`
- [[051-security-limits-and-validated-config/spec|051 Security limits]], [[075-bench-suite/spec|075 Bench suite]], [[076-fuzz-targets/spec|076 Fuzz targets]]
- `docs/architecture/SPECIFICATION.md`
