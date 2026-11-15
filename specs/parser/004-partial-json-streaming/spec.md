---
aliases:
  - Partial JSON Parsing for Streaming Frames
  - Spec 004
tags:
  - sdd
  - spec
  - parser
  - streaming
created: 2026-04-28
status: partial
priority: P4
issue: "#117"
pr: "#220"
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
code:
  - crates/pjs-core/src/parser/partial.rs
  - crates/pjs-core/src/parser/mod.rs
  - crates/pjs-core/Cargo.toml
  - fuzz/fuzz_targets/parser_partial.rs
verified: 2026-11-15
---

# Spec 004: Partial JSON Parsing for Streaming Frames

> [!info] Verification (2026-11-15)
> **State: partial.** The parser exists and is tested; it is not integrated with stream reconstruction and has no production caller.
>
> Evidence: issue #117 closed 2026-04-28 by PR #220 (`6566692`, "add partial JSON parsing foundation behind `partial-parse` feature"). `crates/pjs-core/src/parser/partial.rs` provides sealed trait `PartialJsonParser`, `JiterPartialParser`, `JiterConfig`, `PartialParseResult`, `StreamingHint`, `ParseDiagnostic`. Feature: `partial-parse = ["dep:jiter"]` (not default); workspace `jiter = "0.16"`. Module docs state no production code calls `parse_partial` (only unit tests and `no_run` doc examples). Fuzz target `fuzz/fuzz_targets/parser_partial.rs` exists. Hardening: #577 (consumed bound, open-string truncation).
>
> Drift from original spec:
> - Built on `jiter`'s per-token API, not `simd-json` tape or `serde_json`; there is no `ParseResult::Partial` variant with `remainder: &[u8]`. Result is `PartialParseResult { value: JsonData, consumed: usize, is_complete: bool, streaming_hint: Option<StreamingHint>, diagnostics: Vec<ParseDiagnostic> }`; the unparsed tail is `&input[consumed..]`.
> - Value is owned (`JsonData`), not zero-copy, so NFR "zero-copy where possible" is not met.
> - `HybridParser` fallback to `serde_json` is moot (parser removed, see [[001-simd-backend-implementation/spec|001]]).
> - The `Parser::parse_partial` facade referenced by older docs was removed with `Parser` in #495.

**Type**: research / enhancement | **Priority**: P4 | **Status**: partial

## Functional requirements

### FR-001: Partial result carrying parsed fields and unparsed tail
Status: done (different shape, see drift). Code: `parser/partial.rs::PartialJsonParser::parse_partial`. Returns the largest structurally complete prefix; `consumed == 0` with `JsonData::Null` when nothing is recoverable. Truncation alone is not an error; `Error::InvalidJson` for syntactically invalid input, `Error::LimitExceeded { kind: InputSize, .. }` when `input.len() > JiterConfig::max_input_size` (default 100 MiB); nesting beyond `max_depth` (default 64, equals `MaxDepth::default()`) is `Error::InvalidJson`.

### FR-002: Mergeable with continuation frames in stream reconstruction
Status: open. `StreamingHint { stable_paths, tentative_paths }` is produced for the reconstruction path but nothing under `crates/pjs-core/src/stream/` consumes it (`rg partial crates/pjs-core/src/stream` is empty).

### FR-003: Fallback to full parse for complete frames
Status: superseded. `PartialParseResult::is_complete` is `true` when the whole document parsed; no separate fallback parser exists.

## Non-functional requirements

- Zero-copy: not met (owned `JsonData`).
- Priority ordering untouched: met trivially (parser is not wired into the priority queue).
- DoS bounds: input size and depth limits (`JiterConfig`); `config/security.rs` has a test keeping the default depth aligned with `pjs-domain`.

## Open questions

- Which ingestion path (WebSocket frames, chunked HTTP) will call `parse_partial`? [NEEDS CLARIFICATION: no issue tracks wiring].
- `partial-parse` is not exercised by default CI features: [NEEDS CLARIFICATION: confirm CI feature matrix].

## See Also

- `crates/pjs-core/src/parser/partial.rs` module docs ("not yet wired into production")
- `jiter`: `https://github.com/pydantic/jiter`
