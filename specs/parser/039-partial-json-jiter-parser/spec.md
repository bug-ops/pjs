---
aliases:
  - Partial JSON Parser
  - JiterPartialParser
tags:
  - sdd
  - spec
  - parser
  - partial-parse
  - jiter
created: 2026-11-15
status: partial
priority: P4
issue: "#117"
code:
  - crates/pjs-core/src/parser/partial.rs
  - crates/pjs-core/src/parser/mod.rs
  - crates/pjs-core/Cargo.toml
  - fuzz/fuzz_targets/parser_partial.rs
related:
  - "[[004-partial-json-streaming/spec]]"
  - "[[038-zero-copy-lazy-json-parser/spec]]"
  - "[[041-simd-features-and-global-allocator/spec]]"
  - "[[046-json-reconstruction/spec]]"
verified: 2026-11-18
---

# Spec 039: Partial JSON Parser (`partial-parse` feature)

Part of [[MOC-specs]]. Rules in [[constitution]]. Plan: [[039-partial-json-jiter-parser/plan|plan]]. The research/design origin is [[004-partial-json-streaming/spec|004]]; this spec records what shipped.

> [!info] Status in one line
> The parser, its types and tests exist and are fuzzed, but nothing in the workspace calls it outside its own tests (CHANGELOG `[Unreleased]`, #485; `partial.rs` module docs). Status is therefore `partial`: capability implemented, integration into streaming ingestion not done.

## Overview

**Problem.** A streaming receiver gets a JSON document in chunks. `serde_json::from_slice` fails on a truncated prefix, so it cannot report which fields are already usable.

**Goal.** Given a possibly truncated `&[u8]`, return the largest structurally complete value, the number of bytes consumed, a completeness flag, optional stable/tentative path hints for the reconstruction layer, and non-fatal diagnostics.

**Out of scope.** Feeding the result into `PriorityStreamFrame`s or the HTTP/WebSocket ingestion path; incremental state carried between feeds (every call re-parses from byte 0); a public way to implement the trait (it is sealed).

## User Stories

- As a streaming client author, I pass a growing buffer and learn which JSON paths are already final.
- As an operator, I want a bounded parser: input size and nesting depth are capped and cannot be exceeded by input alone.

## Functional Requirements

### FR-039-001 Feature gate and dependency

Where the `partial-parse` Cargo feature is enabled, `pjson-rs` shall compile `parser::partial` and re-export `JiterConfig`, `JiterPartialParser`, `ParseDiagnostic`, `PartialJsonParser`, `PartialParseResult`, `StreamingHint` from `pjson_rs::parser`. The feature is off by default and pulls `jiter` (workspace version `0.16`).
Code: `crates/pjs-core/Cargo.toml` (`partial-parse = ["dep:jiter"]`), `parser/mod.rs`.
Status: done.

### FR-039-002 Sealed trait

`PartialJsonParser` shall be sealed (private `Sealed` supertrait) so only `pjs-core` can implement it; its single method is `parse_partial(&self, input: &[u8]) -> Result<PartialParseResult>`.
Status: done.

### FR-039-003 Result contract

`PartialParseResult` shall carry `value: JsonData` (owned), `consumed: usize` (always `<= input.len()`), `is_complete: bool`, `streaming_hint: Option<StreamingHint>`, `diagnostics: Vec<ParseDiagnostic>`.
- Empty or whitespace-only input, and input that yields no complete token (`[`, `{`), shall return `value = JsonData::Null`, `consumed = 0`, `is_complete = false`.
- A complete document shall return `is_complete = true` and `consumed` including trailing ASCII whitespace.
- A truncated document shall return the structurally closed prefix and `is_complete = false`.
Status: done. Tests: `test_empty_input_returns_consumed_zero`, `test_lone_open_bracket_consumed_zero`, `test_complete_json_is_complete_true`, `test_truncated_object_partial_recovered`.

### FR-039-004 Truncation boundaries

The parser shall tolerate truncation at: an open string (kept as `JsonData::String` with the bytes so far when `allow_trailing_strings`, else dropped), a `\u` escape cut mid-hex, a bare `-`, a partial keyword (`tr`, `nu`), and a key cut mid-string (the key is dropped).
The parser shall return `Error::InvalidJson` for non-truncation syntax errors, which includes a number cut after `.` or `e` (`1.`, `1e`): jiter reports `InvalidNumber`, which the walker treats as a hard error.
Code: `walk_string`, `walk_number`, `is_partial_tolerated`. Tests: `test_boundary_a..e_*`.
Status: done.

### FR-039-005 Bounds

When `input.len() > JiterConfig::max_input_size` (default 100 MiB), the parser shall return `Error::LimitExceeded { kind: InputSize, actual, max }`.
When nesting reaches `JiterConfig::max_depth` (type `MaxDepth`, default 64, ceiling `MAX_SAFE_JSON_DEPTH` = 512), the parser shall return `Error::InvalidJson` carrying jiter's `RecursionLimitExceeded` message.
Status: done. Note: depth overflow returns `Error::InvalidJson` (see Open Questions).

### FR-039-006 Number handling

Integers in `i64` range shall become `JsonData::Integer`; floats `JsonData::Float`. An integer outside `i64` shall become `JsonData::Float` via `f64` parsing of the decimal string and emit `ParseDiagnostic::BigIntLossyConversion { path, original, converted }`. `NaN`/`Infinity` are accepted only when `allow_inf_nan` is true (default false).
Status: done.

### FR-039-007 Duplicate keys

When an object contains the same key twice, the later value shall win and a `ParseDiagnostic::DuplicateKey { path, key }` shall be emitted.
Status: done. Test: `test_duplicate_key_last_write_wins`.

### FR-039-008 Streaming hint

When `emit_streaming_hint` is true (default) and the value is an object or array, the result shall contain `StreamingHint { stable_paths, tentative_paths }` computed from the leaf paths of `value`. For a complete parse all leaves are stable. For a truncated parse the last leaf in traversal order is tentative and all earlier leaves are stable. Scalars shall produce `None`.
Status: partial. The "last leaf" is determined by iterating `JsonData::Object` (a `HashMap`), so for objects with more than one key the tentative path is not guaranteed to be the one at the truncation boundary and may differ between runs (see Open Questions).

### FR-039-009 Never use non-partial jiter entry points

The module shall not call the `Jiter` methods that hard-code `PartialMode::Off` (`next_value`, `known_value`, `next_value_owned`, `next_skip`, `known_skip`, `JsonValue::parse*`); it shall drive jiter via `peek`, `known_array`, `known_object`, `known_str`, `known_number`, `known_bool`, `known_null`, `array_step`, `next_key`.
Status: done (documented in module docs).

### FR-039-010 Domain isolation

`jiter` types shall not appear outside `parser/partial.rs`, and `pjs-domain` shall not depend on `jiter`.
Status: done.

## Non-Functional Requirements

- NFR-SEC-039-001: `consumed <= input.len()` for every input. Evidence: `Tokens::index()` clamps to input length; unit tests `test_every_prefix_of_sample_documents_respects_consumed_bound`, `test_short_inputs_over_structural_alphabet_never_panic_or_overrun`; fuzz target `fuzz/fuzz_targets/parser_partial.rs` asserts it.
- NFR-SEC-039-002: input size and depth bounded (FR-039-005).
- NFR-PERF-039-001: none defined. Each call is O(n) over the whole buffer plus one extra traversal when hints are on; no incremental state.

## Data Model

| Type | Notes |
|---|---|
| `PartialJsonParser` (sealed trait) | `parse_partial` |
| `JiterPartialParser { config: JiterConfig }` | `new(config)`, `Default` |
| `JiterConfig` | `max_input_size: usize` (100 MiB), `max_depth: MaxDepth` (64), `emit_streaming_hint: bool` (true), `allow_trailing_strings: bool` (true), `allow_inf_nan: bool` (false) |
| `PartialParseResult` | see FR-039-003 |
| `StreamingHint` | `stable_paths: Vec<JsonPath>`, `tentative_paths: Vec<JsonPath>` |
| `ParseDiagnostic` | `DuplicateKey { path, key }`, `BigIntLossyConversion { path, original, converted }` |

## Edge Cases and Errors

| Input | Result |
|---|---|
| `""`, `"   "` | `Ok`, `consumed = 0`, `Null`, incomplete |
| `[` / `{` | `Ok`, `consumed = 0`, incomplete |
| `{"a":1,"b":[2,3` | `Ok`, `{a:1,b:[2,3]}`, incomplete, `consumed = 15` |
| `42  ` | `Ok`, complete, `consumed = 4` |
| `}` or malformed escape | `Err(Error::InvalidJson)` |
| `1.`, `1e` | `Err(Error::InvalidJson)` |
| length > `max_input_size` | `Err(Error::LimitExceeded{InputSize})` |
| depth >= `max_depth` | `Err(Error::InvalidJson)` (recursion limit) |
| integer > `i64::MAX` | `Float` plus `BigIntLossyConversion` diagnostic |

## Success Criteria

- `cargo nextest run -p pjson-rs --features partial-parse` passes (the default CI test archive does not enable this feature; the coverage job uses `--all-features`).
- The `parser_partial` fuzz target runs clean.

## Agent Boundaries

- Always: keep jiter usage inside `partial.rs`; keep the sealed trait sealed.
- Ask first: wiring the parser into HTTP/WebSocket ingestion (needs the incremental-state design in [[004-partial-json-streaming/spec|004]]); changing `ParseDiagnostic` (public enum).
- Never: call the non-partial jiter methods listed in FR-039-009.

## Open Questions

- `build_streaming_hint`/`collect_leaves` iterate a `HashMap`, so the tentative path is order-dependent. This violates the trait's own determinism contract for `streaming_hint`. Needs a follow-up decision (track insertion order in the walker rather than re-deriving it from `JsonData`).
- Resolved by #698: `collect_leaves` and the streaming-hint path builder append every key through the infallible `JsonPath::append_key`; keys with `.`, `[`, `]` or the empty key no longer collapse onto the parent path.
- `walk_number` uses `expect` on `f64` parse of a `BigInt` decimal string; documented as infallible, but it is a panic site in a parser for untrusted input.
- `ParseDiagnostic` and `PartialParseResult` have no `#[non_exhaustive]`; adding a variant is a breaking change.
- No production caller (module docs, CHANGELOG #485). Decide whether to wire it into streaming ingestion or keep it as an unexercised library capability.
- Spec [[004-partial-json-streaming/spec|004]] proposed an incremental/stateful API; the shipped API is stateless (re-parse from offset 0 each feed).

## See Also

- [[039-partial-json-jiter-parser/plan|Plan (as built)]]
- [[046-json-reconstruction/spec|046 JSON reconstruction]] (intended consumer of `StreamingHint`)
- [[076-fuzz-targets/spec|076 Fuzz targets]]
- [[051-security-limits-and-validated-config/spec|051 Security limits]] (`MaxDepth`)
