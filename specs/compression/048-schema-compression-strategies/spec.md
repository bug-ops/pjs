---
aliases:
  - Schema Compression Strategies
  - SchemaCompressor
tags:
  - sdd
  - spec
  - compression
created: 2026-11-15
status: implemented
issue: "#333"
related:
  - "[[047-streaming-frame-compression/spec]]"
  - "[[049-secure-byte-compression/spec]]"
  - "[[002-real-compression-algorithms/spec]]"
  - "[[018-config-validation-startup/spec]]"
code:
  - crates/pjs-core/src/compression/mod.rs
  - crates/pjs-core/src/compression/savings.rs
  - crates/pjs-core/src/config.rs
verified: 2026-11-15
---

# Spec 048: Schema Compression Strategies (Layer A)

[[constitution]] | [[MOC-specs]]

> [!info] Scope
> `pjson_rs::compression::{SchemaAnalyzer, SchemaCompressor, CompressionStrategy, CompressionConfig, CompressedData}` and the private `savings` module. These operate on `serde_json::Value` and produce a smaller `serde_json::Value` plus a side-channel. Byte codecs (gzip/brotli/zstd) are Layer B: [[049-secure-byte-compression/spec|spec 049]]. Decoding lives in [[047-streaming-frame-compression/spec|spec 047]]. Spec 002 (real algorithms) is about Layer B.

## Overview

**Problem.** JSON streams contain repeated strings, monotone-ish integer arrays and runs of equal values that can be shrunk before any byte codec runs.

**Goal (as built).** Choose, per payload, the strategy with the greatest *exact* wire-byte saving, apply it losslessly, and report the measured wire size.

**Out of scope.** Lossy transforms, byte-level compression, string interning in memory.

## User Stories

- As a server author, I call `analyze_and_optimize(&sample)` then `compress(&data)` and get `CompressedData` with a true `compressed_size`.
- As a maintainer, I tune thresholds through a validated `CompressionConfig`.

## Functional Requirements

### FR-001 Strategies
`CompressionStrategy` shall be one of `None`, `Dictionary{dictionary: HashMap<String,u16>}`, `Delta`, `RunLength`, `Hybrid{string_dict}`.
Status: done.

### FR-002 Strategy selection
`SchemaAnalyzer::analyze` shall walk the payload once, model the net wire-byte saving for Dictionary, Delta, RunLength and Hybrid (= dictionary + delta, only when both are positive), discard candidates below `min_net_savings` (default 10), and return the candidate with the largest net saving, or `None`. Ties keep the first candidate in the order Dictionary, Delta, RunLength, Hybrid.
Status: done.
Code: `SchemaAnalyzer::{analyze, walk, determine_strategy}`.

### FR-003 Dictionary construction
A string qualifies when its count is greater than `min_frequency_count` (default 1) and its byte length is greater than `min_string_length` (default 3). Candidates are sorted by `count * len` descending with lexicographic tie-break (deterministic), indices are assigned from 0, an entry is kept only if modelled gain exceeds its cost (`len + 3`), and assignment stops at index `u16::MAX`. The fixed `{"dict":[]}` envelope (10 bytes) is subtracted once.
Status: done.
Code: `build_dictionary`.

### FR-004 Dictionary encoding
Substitution shall replace each matching string *value* (object keys are untouched) with `U+007F` followed by the decimal index. A string value that itself starts with `U+007F` and is not in the dictionary shall be escaped by prefixing a second sentinel. The dictionary travels as metadata key `"dict"`: a JSON array whose position is the index.
Status: done.

### FR-005 Delta encoding
An array of at least `min_numeric_sequence_size` (default and validated minimum 3) integers shall be encoded as `[{"delta_base": <min>, "delta_type": "numeric_sequence"}, d0, d1, ...]` where each `d` is `value - min` as `u64`; the array is encoded only if it saves bytes. Arrays with floats, non-numbers, offsets that do not fit `u64`, or length >= `MAX_DELTA_ARRAY_SIZE` (1_000_000) stay raw. Delta encoding recurses into arrays that are not themselves encoded.
Status: done.
Code: `savings::delta_plan`, `SchemaCompressor::apply_delta_compression`.

### FR-006 Run-length encoding
For arrays longer than 2 elements and not longer than `MAX_DECOMPRESSED_ELEMENTS` (10_485_760), runs of identical consecutive values (structural equality, `0.0` distinct from `-0.0`) shall be encoded as `{"rle_value": v, "rle_count": n}` only when this saves bytes (`run_saving > 0`); runs longer than `MAX_RLE_COUNT` (100_000) are split. Other elements are emitted verbatim. The analyzer models RLE only for arrays the encoder actually reaches.
Status: done.

### FR-007 Hybrid
Hybrid shall apply dictionary substitution first and delta encoding second; the decoder reverses the order. RLE is not part of Hybrid.
Status: done.

### FR-008 Measured size
`CompressedData::compressed_size` shall equal the compact serialisation length of `data` plus, when `compression_metadata` is non-empty, the serialisation length of the metadata map. It is measured, not modelled. `compression_ratio(original)` = compressed/original (1.0 for zero original); `compression_savings` returns a signed byte difference.
Status: done.
Code: `wire_size`.

### FR-009 Validated configuration
`SchemaAnalyzer::with_config` and `SchemaCompressor::with_config` shall accept only `ValidatedCompressionConfig`. `CompressionConfig::validate` rejects `min_numeric_sequence_size < 3` with `ConfigError::InconsistentBounds`.
Status: done. Other fields are unvalidated (no zero check).

### FR-010 Fixed strategy
`SchemaCompressor::with_strategy(s)` shall use `s` without analysis; `compress` dispatches on the current strategy. A fixed `Dictionary` whose map has wrong indices produces garbage-in-garbage-out with no validation.
Status: done.

## Non-Functional Requirements

- NFR-CORR-001: lossless round trip for all strategies (see [[047-streaming-frame-compression/spec|spec 047]] tests), including integers beyond `i64`, `u64`, sign of zero, sentinel-led strings.
- NFR-DET-001: dictionary indices are deterministic for identical input.
- NFR-BOUND-001: the encoder never emits an array the decoder would refuse (`rle_len_within_cap`, `delta_len_within_cap`; boundary test `caps_apply_at_exact_boundaries`).
- NFR-PERF-001: `[NEEDS CLARIFICATION]` no benchmark; `analyze` is one walk but `delta_plan` re-serialises each element to count bytes.

## Data Model

`CompressionConfig { min_string_length: usize, min_frequency_count: u32, min_net_savings: usize, min_numeric_sequence_size: usize }`; `CompressedData { strategy, compressed_size, data, compression_metadata: HashMap<String, JsonValue> }`; crate constants `MAX_RLE_COUNT`, `MAX_DELTA_ARRAY_SIZE`, `MAX_DECOMPRESSED_ELEMENTS`, `DICT_SENTINEL`, `MIN_DELTA_SEQUENCE_SIZE` (all `pub(crate)`).

## Edge Cases and Errors

| Case | Behaviour |
|---|---|
| `"active"` repeated twice only | per-entry gate fails, strategy `None` |
| more than 65535 qualifying strings (indices 0..=65534 used) | extras left raw |
| array with negative ints | offset from minimum, never negative |
| mixed int/float array | not delta-encoded |
| `compress` on non-serialisable data | `DomainError::CompressionError("JSON serialization failed")` (practically unreachable for `Value`) |
| `analyze` | returns `Ok` always |

## Success Criteria

- 36 unit tests in `compression/mod.rs`, 5 in `savings.rs`, plus the integration suites in spec 047 pass.

## Agent Boundaries

- Always: keep analyzer model and encoder in lockstep via `savings`; add a boundary test for any new cap.
- Ask first: changing the marker format (breaking for decoders), adding strategies (decoder dispatch must follow).
- Never: let an encoder emit what the decoder rejects.

## Open Questions

1. `compression_metadata` is `HashMap<String, JsonValue>` (stringly typed; #603).
2. `CompressionConfig` fields other than `min_numeric_sequence_size` are not validated; `min_string_length`/`min_frequency_count` of 0 are accepted.
3. Documented imprecision: the dictionary model ignores JSON escaping and sentinel-escape cost, so a crafted payload can make a selected Dictionary net-negative (doc on `min_net_savings`).
4. `Hybrid` ignores RLE, so a payload that mixes strings and runs may pick Dictionary or Hybrid and forgo RLE savings by design.
5. `SchemaAnalyzer::analyze` returns `DomainResult` but cannot fail.

## See Also

- [[048-schema-compression-strategies/plan|plan]], [[047-streaming-frame-compression/spec|spec 047]], [[049-secure-byte-compression/spec|spec 049]]
- [[018-config-validation-startup/spec|spec 018]] (`PjsConfig::validate` chain includes `CompressionConfig`)
- CHANGELOG entries for #333
