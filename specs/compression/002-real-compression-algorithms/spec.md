---
aliases:
  - Real Compression Algorithm Backends
  - Spec 002
tags:
  - sdd
  - spec
  - compression
  - security
created: 2026-04-28
status: implemented
priority: P2
issue: "#114"
pr: "#148"
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
code:
  - crates/pjs-core/src/compression/secure.rs
  - crates/pjs-core/src/security/compression_bomb.rs
  - crates/pjs-core/Cargo.toml
verified: 2026-11-15
---

# Spec 002: Real Compression Algorithm Backends

> [!info] Verification (2026-11-15)
> **State: implemented.** Issue #114 closed 2026-04-28 by PR #148 (`2b4e57a`, "replace simulated compression with real flate2/brotli"). Zstd was added later by #222 (`8b15839`, `ByteCodec::ZstdDict`).
>
> Evidence: `crates/pjs-core/src/compression/secure.rs` defines `ByteCodec { None, Deflate, Gzip, Brotli, ZstdDict(Arc<ZstdDictionary>) }`, `CompressionQuality { Fast, Balanced, Best }`, `SecureCompressor::{compress, decompress_protected, decompress_nested}`. `compress_with_strategy` no longer exists (`rg compress_with_strategy crates` is empty). The `compression` feature is `["dep:brotli", "dep:flate2", "dep:zstd"]`. Unit tests: `test_deflate_roundtrip`, `test_gzip_roundtrip`, `test_brotli_roundtrip` (asserts size reduction), `test_bomb_detection_on_real_codec`, zstd-dict tests.
>
> Drift from original spec:
> - Entry point is `SecureCompressor::compress(&[u8])` with a `ByteCodec`, not `compress_with_strategy` with `CompressionStrategy`. `CompressionStrategy` now belongs to the structural `SchemaCompressor` (`compression/mod.rs`, "Layer A"); `ByteCodec` is "Layer B".
> - `async-compression` was not adopted. Decoders are synchronous `flate2::read::*`, `brotli::Decompressor`, `zstd::stream::read::Decoder`, wrapped by `CompressionBombProtector` and run in a read loop.
> - Zstd (originally optional/follow-on) is implemented, non-wasm only (`cfg(not(target_arch = "wasm32"))`).
> - `SecureCompressedData` is in-process only (carries its codec tag); not a wire format. The module docs state `decompress_protected`/`decompress_nested` have no production callers; only outbound `compress` is used.

**Type**: enhancement | **Priority**: P2 | **Status**: implemented

## Problem (resolved)

`compress_with_strategy` simulated compression (entropy-based ratio, byte copy) while `--features compression` advertised gzip/brotli. Callers received synthetic data.

## Functional requirements

### FR-001: Real codecs behind `compression`
When the `compression` feature is enabled, `SecureCompressor::compress` shall encode with `flate2` (Deflate, Gzip) or `brotli`; without the feature, non-`None` codecs return `Error::CodecUnavailable { codec }`.
Status: done. Code: `compression/secure.rs::SecureCompressor::encode`.

### FR-002: Output smaller than input for repetitive JSON
Status: done. Test `test_brotli_roundtrip` asserts reduction; deflate/gzip round-trip tests exist.

### FR-003: Round-trip via `decompress_protected`
Status: done. `decode_with_protection` streams decoder output through `CompressionBombProtector`.

### FR-004: Bomb detection works with real decompression
Status: done. `test_bomb_detection_on_real_codec`, `test_zstd_dict_bomb_detection`; limits from `CompressionBombConfig { max_ratio, max_decompressed_size, max_compressed_size, max_compression_depth, check_interval_bytes }` (`security/compression_bomb.rs`).

## Non-functional requirements

- Feature gating: all codec dependencies are `optional` and enabled only by `compression` (met).
- Async-safe streaming via `async-compression`: not adopted (superseded).
- Thresholds re-validated against real expansion: [NEEDS CLARIFICATION: no recorded re-validation; defaults live in `CompressionBombConfig`].

## Open questions

- `decompress_protected`/`decompress_nested` are unused in production (documented in module docs); keep as tested API or remove.

## See Also

- [[011-zstd-dictionary-compression/spec|011 zstd dictionary]]
- `crates/pjs-core/src/security/compression_bomb.rs`
