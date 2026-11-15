---
aliases:
  - Streaming Frame Compression
  - StreamingCompressor
tags:
  - sdd
  - spec
  - stream
  - compression
created: 2026-11-15
status: implemented
issue: "#603"
related:
  - "[[048-schema-compression-strategies/spec]]"
  - "[[045-priority-stream-planning/spec]]"
  - "[[002-real-compression-algorithms/spec]]"
  - "[[024-compress-owned-bytes-return/spec]]"
code:
  - crates/pjs-core/src/stream/compression_integration.rs
  - crates/pjs-core/src/stream/mod.rs
verified: 2026-11-15
---

# Spec 047: Streaming Frame Compression

[[constitution]] | [[MOC-specs]]

> [!info] Scope
> Layer A (JSON-structural) compression applied per `StreamFrame` by `StreamingCompressor` and reversed by `StreamingDecompressor`, including decoder-side resource guards. The strategies themselves are in [[048-schema-compression-strategies/spec|spec 048]]; byte-level codecs (Layer B) are in [[049-secure-byte-compression/spec|spec 049]]. Issue #603 (open) tracks known API defects listed under Open Questions.

## Overview

**Problem.** Frames of different priority benefit from different compression state, and a receiving side must undo compression without trusting the sender (decompression-amplification DoS).

**Goal (as built).** Two `SchemaCompressor`s (one for CRITICAL/HIGH frames, one for the rest) compress each frame's `data`; a `CompressedFrame` carries the data, the strategy and the side-channel dictionary; `StreamingDecompressor` restores the frame under a fixed byte budget.

**Out of scope.** Wire framing, transport negotiation, byte codecs.

## User Stories

- As a server, I call `compress_frame` per frame and track savings through `CompressionStats`.
- As a client, I call `decompress_frame` and get the original `StreamFrame` back, or an error on malformed input.
- As an operator, I want a malicious frame never to allocate beyond a bounded amount.

## Functional Requirements

### FR-001 Compressor selection by priority
`StreamingCompressor::compress_frame` shall use `skeleton_compressor` for `Priority::CRITICAL` and `Priority::HIGH`, and `content_compressor` for every other priority.
Status: done.
Code: `select_compressor_for_priority`.

### FR-002 Strategy configuration
`StreamingCompressor::new` shall start both compressors with `CompressionStrategy::None`; `with_strategies` sets explicit strategies; `optimize_for_data(skeleton, samples)` runs `analyze_and_optimize` on the skeleton compressor and, if samples are non-empty, on the content compressor using the samples as one array. `compress_frame` itself never re-analyses.
Status: done.

### FR-003 Compressed frame contents
`compress_frame` shall return `CompressedFrame { frame, compressed_data, decompression_metadata }` where `frame` is the original, uncompressed frame, `compressed_data` is the `CompressedData` from the chosen compressor, and `decompression_metadata` holds the strategy, `dictionary_map: HashMap<u16,String>` rebuilt from the `"dict"` metadata array, and `delta_bases` rebuilt from `base_*` metadata keys.
Status: done. `[NEEDS CLARIFICATION]` whether the original `frame.data` should travel with the compressed copy; it does in the in-memory struct.

### FR-004 Statistics
Each compressed frame shall add its serialised original size and `compressed_size` to `total_input_bytes`/`total_output_bytes`, increment `frames_processed`, and store the frame's ratio in `priority_ratios[priority.value()]`. `CompressionStats` shall provide `overall_compression_ratio`, `priority_compression_ratio(u8)` (1.0 default), `bytes_saved` and `percentage_saved`.
Status: done (with defects, see Open Questions).

### FR-005 Decompression dispatch
`StreamingDecompressor::decompress_frame` shall dispatch on `DecompressionMetadata.strategy`: `None` -> clone; `Dictionary` -> dictionary expansion; `Delta` -> delta decode; `RunLength` -> RLE expansion; `Hybrid` -> delta decode first, then dictionary expansion. The returned `StreamFrame` keeps the compressed frame's priority and metadata.
Status: done.

### FR-006 Dictionary decoding
A string starting with the sentinel `U+007F` shall be decoded as: sentinel + sentinel -> literal string with one sentinel removed; sentinel + decimal digits -> dictionary entry for that `u16` index from the same frame's `dictionary_map`; anything else after the sentinel, or an index absent from the map, shall be `DomainError::CompressionError`. Strings not starting with the sentinel pass through. Dictionary state is never carried across frames.
Status: done (#333).

### FR-007 Run-length decoding guards
For an element object containing `rle_value`/`rle_count`: both keys must be present (else error); `rle_count` must be a `u64` <= `MAX_RLE_COUNT` (100_000); the expansion shall be charged to a per-call byte budget before allocation.
Status: done.

### FR-008 Delta decoding guards
A delta-encoded array (first element an object with `delta_base` and `delta_type`) shall be rejected if longer than `MAX_DELTA_ARRAY_SIZE` (1_000_000), if `delta_base` is not a number, or if an element is not a number. Integer sums use `i128` and must fit `i64` or `u64`, otherwise error; float bases/deltas fall back to `f64` addition. The first element is the header and is not emitted.
Status: done.

### FR-009 Byte budget
Dictionary expansion and RLE expansion in one decompress call shall together be limited to `MAX_DECOMPRESSED_SIZE` = 10_485_760 estimated heap bytes (`ByteBudget`, using `heap_estimate`: string length + `size_of::<JsonValue>()`, object node 640 bytes + key length + `size_of::<String>()` per entry). Exceeding it shall fail with `CompressionError("Decompressed size exceeds maximum ...")` before the allocation.
Status: done. The delta path is not charged to the budget (bounded by FR-008).

### FR-010 Decompression statistics
`StreamingDecompressor` shall count frames, sum serialised decompressed bytes, and keep a running average of decompression time in microseconds.
Status: done.

## Non-Functional Requirements

- NFR-SEC-001: decoding of any single frame shall not allocate more than roughly `MAX_DECOMPRESSED_SIZE` for dictionary/RLE expansion. Test coverage: `crates/pjs-core/tests/compression_integration_phase2.rs`, unit tests in `compression_integration.rs` (around the "exceed MAX_DECOMPRESSED_ELEMENTS" case), fuzz target `fuzz/fuzz_targets/streaming_decompressor.rs`.
- NFR-CORR-001: for every strategy, `decompress(compress(x)) == x` including sentinel-led strings, `-0.0`, integers beyond `i64`. Verified by unit and integration tests (93 unit tests in the file; 3 integration files with 35, 48, 83 tests).
- NFR-PERF-001: `[NEEDS CLARIFICATION]` no benchmark in `pjs-bench` targets this path.

## Data Model

`StreamingCompressor { skeleton_compressor, content_compressor: SchemaCompressor, stats: CompressionStats }`; `CompressedFrame`; `DecompressionMetadata { strategy, dictionary_map, delta_bases }`; `CompressionStats { total_input_bytes, total_output_bytes, frames_processed: u32, priority_ratios: HashMap<u8,f32> }`; `StreamingDecompressor { stats: DecompressionStats }`; private `ByteBudget`.

## Edge Cases and Errors

All errors are `DomainError::CompressionError(String)`:

| Case | Message start |
|---|---|
| dictionary marker not numeric | `malformed dictionary marker` |
| marker index missing | `dictionary marker index N not found in active dictionary` |
| RLE key pair incomplete | `Malformed RLE object` |
| RLE count not `u64` / too large | `Invalid rle_count` / `RLE count N exceeds maximum` |
| budget exceeded | `Decompressed size exceeds maximum` |
| delta array too long / bad base / bad element / overflow | `Delta array size`, `Missing or invalid delta_base`, `Invalid delta value`, `Delta-decoded value overflows u64` |
| original JSON cannot be serialised in `compress_frame` | `JSON serialization failed` |

## Success Criteria

- Round-trip and rejection tests above pass; fuzz target runs without panics.

## Agent Boundaries

- Always: charge any new expansion path to `ByteBudget`; keep decoding per-frame stateless.
- Ask first: changing `DecompressionMetadata` shape (breaking for clients), raising `MAX_DECOMPRESSED_SIZE`.
- Never: resolve a dictionary marker against anything other than the frame's own dictionary; decode with `unwrap`/panics.

## Open Questions

Tracked by #603 (open):
1. Metadata is a magic-key protocol: dictionary under `"dict"`, delta bases under a `base_` prefix inside `HashMap<String, JsonValue>`.
2. `delta_bases` is dead: no encoder emits `base_*` keys and the decoder never reads the field.
3. `priority_ratios` keeps only the latest frame's ratio per priority, not an aggregate; keyed by raw `u8` instead of `Priority`.
4. Counters use unchecked `+=` and `as isize` casts (`bytes_saved`).
5. `compress_frame` calls `serde_json::to_string` only to measure the original size (allocation per frame).
6. Delta decoding is not covered by the byte budget.
7. Spec 024 concerns `maybe_compress_owned` in the HTTP layer, a different code path.

## See Also

- [[047-streaming-frame-compression/plan|plan]], [[048-schema-compression-strategies/spec|spec 048]], [[049-secure-byte-compression/spec|spec 049]]
- [[002-real-compression-algorithms/spec|Spec 002]]
- `fuzz/fuzz_targets/streaming_decompressor.rs`
