---
aliases:
  - Secure Byte Compression
  - SecureCompressor
  - ByteCodec
tags:
  - sdd
  - spec
  - compression
  - security
  - zstd
created: 2026-11-15
status: partial
issue: "#114"
related:
  - "[[050-compression-bomb-and-rate-limiting/spec]]"
  - "[[048-schema-compression-strategies/spec]]"
  - "[[002-real-compression-algorithms/spec]]"
  - "[[011-zstd-dictionary-compression/spec]]"
code:
  - crates/pjs-core/src/compression/secure.rs
  - crates/pjs-core/src/compression/zstd.rs
  - crates/pjs-core/src/domain/ports/dictionary_store.rs
  - fuzz/fuzz_targets/secure_decompress.rs
verified: 2026-11-15
---

# Spec 049: Secure Byte Compression (Layer B)

[[constitution]] | [[MOC-specs]]

> [!info] Scope
> `pjson_rs::compression::secure` (`SecureCompressor`, `ByteCodec`, `CompressionQuality`, `SecureCompressedData`, `SecureDecompressionContext`) and `pjson_rs::compression::zstd` (`ZstdDictionary`, `ZstdDictCompressor`). Real codecs were delivered under #114 ([[002-real-compression-algorithms/spec|spec 002]]); the zstd dictionary store and HTTP endpoints belong to [[011-zstd-dictionary-compression/spec|spec 011]]. Status is `partial`: the codecs and guards work and are tested, but nothing outside tests and the fuzz target calls them (see Open Questions).

## Overview

**Problem.** Byte-level compression of serialised JSON is a bandwidth win, but decoding attacker-supplied bytes must be bounded.

**Goal (as built).** A single compressor type supporting `None`, raw deflate, gzip, brotli and trained-dictionary zstd, whose decode path always streams through `CompressionBombProtector` ([[050-compression-bomb-and-rate-limiting/spec|spec 050]]).

**Out of scope.** A wire format (the codec tag is in-process only), negotiation, per-session dictionary lifecycle ([[011-zstd-dictionary-compression/spec|spec 011]]), WASM (zstd is not built for `wasm32`).

## User Stories

- As a server, I compress an outgoing buffer with a chosen codec and quality.
- As a receiver, I decompress untrusted bytes and get an error instead of exhausting memory.
- As an operator with homogeneous JSON, I train a zstd dictionary from samples and share it as `Arc<ZstdDictionary>`.

## Feature flags

- `compression` (default) enables `flate2`, `brotli`, `zstd`. Without it, `ByteCodec::{Deflate, Gzip, Brotli}` exist but `compress`/`decompress_*` return `Error::CompressionError("feature `compression` is not enabled")`.
- `ByteCodec::ZstdDict` and `compression::zstd` require `compression` and `not(target_arch = "wasm32")`.

## Functional Requirements

### FR-001 Codecs
`ByteCodec` shall be `None` (default), `Deflate`, `Gzip`, `Brotli`, `ZstdDict(Arc<ZstdDictionary>)`. It derives `Clone, PartialEq, Eq, Default` and is not `Copy`.
Status: done.

### FR-002 Quality
`CompressionQuality::{Fast, Balanced (default), Best}` shall map to deflate/gzip levels 1/6/9 (`flate2::Compression::{fast, default, best}`) and brotli quality 1/5/11. It does not affect `None` or `ZstdDict` (zstd uses `DEFAULT_LEVEL` 3).
Status: done.

### FR-003 Compress
`SecureCompressor::compress(&[u8])` shall first call `validate_pre_decompression(data.len())` (so the input size is checked against `max_compressed_size`, default 100 MiB), then encode, and return `SecureCompressedData { data, original_size, compression_ratio = original/compressed (denominator at least 1), codec }`.
Status: done.

### FR-004 Protected decompress
`decompress_protected` shall check the compressed size against `max_compressed_size`, then decode with the codec stored in the `SecureCompressedData` through `CompressionBombDetector::protect_reader` using `read_to_end`, and finally run `validate_result(compressed_len, output_len)`. Any read or limit error shall surface as `Error::SecurityError("Protected decompression failed: ...")`.
Status: done.

### FR-005 Nested decompress
`decompress_nested(compressed, depth)` shall behave as FR-004 but use `protect_nested_reader`, which fails when `depth > max_compression_depth` (default 3).
Status: done. The depth is caller-supplied; the compressor does not detect nesting itself.

### FR-006 Zstd streaming decode
`ByteCodec::ZstdDict` decoding shall use `zstd::stream::read::Decoder::with_dictionary` so every output byte passes the protector; bulk decompression is deliberately avoided on this path.
Status: done.

### FR-007 Zstd dictionary type
`ZstdDictionary::from_bytes` shall reject an empty blob, a blob longer than `MAX_DICT_SIZE` (112 KiB), and a blob not starting with the zstd dictionary magic `0xEC30A437` (bytes `37 A4 30 EC`), using `Error::CompressionError`. The inner bytes are private, so a held `ZstdDictionary` always satisfies the invariant.
Status: done.

### FR-008 Zstd training
`ZstdDictCompressor::train(samples, max_dict_size)` shall require at least 8 samples, clamp `max_dict_size` to `MAX_DICT_SIZE`, call `zstd::dict::from_samples`, and re-validate the result. `N_TRAIN` (32) is the project convention for the sample count before training.
Status: done.

### FR-009 Zstd compress / standalone decompress
`compress` and `compress_with_level` shall build a `zstd::bulk::Compressor` with the dictionary per call (default level 3). `ZstdDictCompressor::decompress(data, dict, max_output)` is a standalone, bulk, caller-capped path that is not bomb-protected and is documented for tools and tests only.
Status: done.

### FR-010 Decompression context scaffold
`SecureDecompressionContext::start_stream(compressed_size)` shall fail with `Error::SecurityError` when `active_streams >= max_concurrent_streams`, otherwise increment the counter and return a protector over an empty in-memory cursor; `finish_stream` decrements (saturating at 0); `stats()` reports depth, active and max.
Status: partial. The returned reader is an empty cursor and `current_depth` is never changed, so this is a concurrency-count scaffold only (per its own doc).

## Non-Functional Requirements

- NFR-SEC-001: decoding any codec of attacker bytes never produces more than `max_decompressed_size` bytes plus at most one read buffer, and never exceeds `max_ratio` at the check points (every `check_interval_bytes`, and at completion). Tests: bomb-detection test on a real codec in `secure.rs`; fuzz target `fuzz/fuzz_targets/secure_decompress.rs` (covers None, Deflate, Gzip, Brotli; not `ZstdDict`, tracked by #575).
- NFR-CORR-001: round trip for every codec and quality (`test_deflate_roundtrip`, `test_gzip_roundtrip`, `test_brotli_roundtrip`, `test_all_qualities_*`, zstd round trip in `zstd.rs`).
- NFR-PERF-001: `ZstdDictCompressor` creates a compressor per call (TODO(#144 follow-up) per-session cache). No benchmark.

## Data Model

`SecureCompressor { detector: CompressionBombDetector, codec: ByteCodec, quality: CompressionQuality }`; `SecureCompressedData { data: Vec<u8>, original_size: usize, compression_ratio: f64, codec: ByteCodec }` (public fields; in-process only); `ZstdDictionary(Vec<u8>)`; `DictionaryStore` port (`domain/ports/dictionary_store.rs`, boxed futures, not GAT) with `NoopDictionaryStore`.

## Edge Cases and Errors

| Case | Result |
|---|---|
| Codec tag mismatch on decode | decoder error wrapped as `SecurityError("Protected decompression failed ...")` (`test_codec_mismatch_returns_error`) |
| Raw deflate has no magic | mismatch may not fail cleanly, relies on the codec tag |
| Input larger than `max_compressed_size` | `SecurityError` before any encode/decode |
| Output exceeds size or ratio | `SecurityError` from protector or `validate_result` |
| Fewer than 8 training samples | `CompressionError("zstd: insufficient samples")` |
| Dictionary invalid | `CompressionError("zstd: ...")` |
| `compression` feature off, non-`None` codec | `CompressionError("feature `compression` is not enabled")` |

## Success Criteria

- 37 unit tests in `secure.rs` and 9 in `zstd.rs` pass with `--all-features`; fuzz target runs clean.

## Agent Boundaries

- Always: route every decode through the protector; keep `ZstdDictionary` construction behind `new_checked`.
- Ask first: introducing a wire framing header for `SecureCompressedData`; making `decompress` of `ZstdDictCompressor` reachable from network input.
- Never: use bulk zstd/flate decode on untrusted input; add `Copy` back to `ByteCodec`.

## Open Questions

1. No production caller: grep across `crates/` finds `SecureCompressor`/`decompress_protected` only in `compression/secure.rs`, `lib.rs` re-exports, tests and the fuzz target (the module doc and CHANGELOG #482 say the same). The guards are therefore defence in depth for future wiring, not active protection.
2. `SecureCompressedData` has public fields, so `original_size` and `compression_ratio` can disagree with `data`; `decompress_*` ignores them.
3. `compress` rejects inputs by `max_compressed_size`, a name that suggests compressed bytes; the check runs on the uncompressed input.
4. Protector checks run every `check_interval_bytes`, so a single read can overshoot the limit by up to one read buffer before the next check; the final `validate_result` still catches the total.
5. The `DictionaryStore` port is feature-independent and uses `TrainedDictionary` and `DomainResult` (#605); `ZstdDictionary` holds a `TrainedDictionary` plus the zstd magic check.
6. `ByteCodec::ZstdDict` is not fuzzed (#575, open).
7. `Error::CompressionError` and `SecurityError` carry `String` (#599).

## See Also

- [[049-secure-byte-compression/plan|plan]], [[050-compression-bomb-and-rate-limiting/spec|spec 050]], [[011-zstd-dictionary-compression/spec|spec 011]], [[002-real-compression-algorithms/spec|spec 002]]
- `crates/pjs-core/src/infrastructure/repositories/dictionary_store.rs`, `crates/pjs-core/src/infrastructure/http/handlers/dictionary.rs`
