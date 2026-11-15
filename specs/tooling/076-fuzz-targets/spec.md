---
aliases:
  - cargo-fuzz targets
  - fuzzing
tags:
  - sdd
  - spec
  - fuzzing
  - security
created: 2026-11-15
status: implemented
priority: P2
related:
  - "[[078-ci-pipeline/spec]]"
issue: "#575"
pr: "#577"
code:
  - fuzz/Cargo.toml
  - fuzz/fuzz_targets/parser_zero_copy.rs
  - fuzz/fuzz_targets/parser_partial.rs
  - fuzz/fuzz_targets/secure_decompress.rs
  - fuzz/fuzz_targets/streaming_decompressor.rs
  - fuzz/fuzz_targets/ws_message_decode.rs
  - fuzz/fuzz_targets/zstd_dict_codec.rs
  - fuzz/src/lib.rs
  - fuzz/seeds
verified: 2026-11-15
---

# Spec 076: cargo-fuzz targets and CI replay

Part of [[MOC-specs]]; see [[constitution]] and `SECURITY.md`.

> [!info] Verification (2026-11-15)
> Implemented. Fuzz targets were added in #577 (`git log` on `fuzz/`). CI replays committed seeds on every relevant run and explores each target for 60 s on the weekly schedule. `zstd_dict_codec` and the production-float-path oracle for `ws_message_decode` were added for #575.

## Overview

**Problem.** Parsers, decompressors and the WebSocket message codec consume untrusted bytes; panics, hangs or unbounded allocation are DoS vectors.

**Goal.** Six libFuzzer targets over the untrusted-input entry points, deterministic seed replay on every CI run, short exploration on a schedule.

**Out of scope.** Fuzzing the WASM crate, HTTP handlers, or the TypeScript client.

## Functional Requirements

### FR-001 Standalone fuzz workspace
The `fuzz/` crate (`pjs-fuzz`, `publish = false`, edition 2024) shall be its own workspace (`[workspace] members = ["."]`), excluded from the root workspace (`exclude = ["crates/pjs-js-client", "fuzz"]`), with its own `Cargo.lock`. It depends on `libfuzzer-sys`, plain `serde_json` (no `float_roundtrip`, so the production float path runs), and `pjson-rs` by path with `default-features = false`, features `compression`, `partial-parse`, `http-server`.
Code: `fuzz/Cargo.toml`.
Status: done.

### FR-001a Shared oracle library
The fuzz crate shall also expose a crate-private library target `pjs_fuzz` (`fuzz/src/lib.rs`, `publish = false`) holding the oracles shared by targets: `approx_eq` (1-ULP float tolerance) and `assert_decode_error` (accepts only `Error::CompressionBomb` or `Error::Codec`). It exists because a `no_main` fuzz binary cannot run unit tests; the helpers' unit tests run in CI.
Code: `fuzz/src/lib.rs`.
Status: done.

### FR-002 parser_zero_copy
The target shall feed arbitrary bytes to `ZeroCopyParser::new().parse_lazy(data)` and ignore the result; the oracle is "no panic, no hang".
Code: `fuzz/fuzz_targets/parser_zero_copy.rs`.
Status: done.

### FR-003 parser_partial
The target shall feed bytes to `JiterPartialParser::default().parse_partial` and, on success, assert `result.consumed <= data.len()`.
Code: `parser_partial.rs`.
Status: done.

### FR-004 secure_decompress
The target shall take the first byte as selector: codec = `selector % 4` (`None`, `Deflate`, `Gzip`, `Brotli`), nested depth = `selector >> 2`, remainder as payload; it builds a `SecureCompressedData` and calls `SecureCompressor::with_default_security(codec)` `.decompress_protected` and `.decompress_nested`. `ByteCodec::ZstdDict` is covered by FR-009.
Code: `secure_decompress.rs`.
Status: done.

### FR-005 streaming_decompressor
The target shall build a `CompressedFrame` from `Unstructured` input (strategy `None`, `Dictionary`, `Delta`, `RunLength`, `Hybrid`; arbitrary `dictionary_map: HashMap<u16,String>`, `delta_bases: HashMap<String,f64>`; remaining bytes parsed as JSON data or a lossy string) and call `StreamingDecompressor::new().decompress_frame`.
Code: `streaming_decompressor.rs`.
Status: done.

### FR-006 ws_message_decode
The target shall decode bytes as `pjson_rs::infrastructure::WsMessage` and, on success, assert a re-encode/decode round-trip yields an approximately equal `serde_json::Value`: exact for everything except floats, where two non-integer numbers are equal when their ULP distance is at most 1 (`+0 == -0`). A drift above 1 ULP fails the oracle (#575 states the loss as 1 ULP). `expect` calls are also part of the oracle.
Code: `ws_message_decode.rs`.
Status: done. The fuzz crate does not enable `float_roundtrip` / `pjson-rs/float-roundtrip`, so the target runs the production (default) float path with the 1-ULP tolerance. The opt-in feature path is covered by `crates/pjs-core/tests/float_roundtrip.rs` under `--all-features`. The comparison helper `approx_eq` lives in the fuzz crate's library target (`fuzz/src/lib.rs`, because a `no_main` fuzz binary cannot run unit tests) with unit tests (1 ULP passes, 2 ULP fails, signed zeros).

### FR-007 Seed corpora
Committed seeds shall live in `fuzz/seeds/<target>/` (3 for `parser_partial`, 3 `parser_zero_copy`, 4 `secure_decompress` (one per codec), 3 `streaming_decompressor`, 5 `ws_message_decode`, 5 `zstd_dict_codec`). The first byte of `secure_decompress` seeds is the selector; the first bytes of `streaming_decompressor` seeds select the strategy. `corpus/`, `artifacts/`, `coverage/`, `target/` are git-ignored.
Status: done.

### FR-009 zstd_dict_codec
The target shall build a `ZstdDictionary` once (`LazyLock`) by `ZstdDictCompressor::train` over `N_TRAIN` deterministic JSON samples. The first byte is the selector. Bit 0 = 0: decode the remaining bytes as arbitrary input through `SecureCompressor(ByteCodec::ZstdDict)` `.decompress_protected`, `.decompress_nested` (depth `selector >> 1`) and `ZstdDictCompressor::decompress(payload, dict, 1 MiB)`; the oracle is "no panic, no hang". Bit 0 = 1: compress the payload with `compress_with_level` (level `1 + (selector >> 1) % 3`) and decompress with `decompress_protected`; when it returns `Ok(out)`, `out` shall equal the payload.
Code: `zstd_dict_codec.rs`.
Status: done.

### FR-008 CI integration
CI job `fuzz` (on rust changes, push, schedule, manual) shall: format-check and clippy the fuzz crate, run `cargo test --manifest-path fuzz/Cargo.toml --lib` (unit tests of the shared oracle helpers), `cargo fuzz build --target x86_64-unknown-linux-gnu`, then for every `cargo fuzz list` target replay `fuzz/seeds/$target` with `-runs=0`. Job `fuzz-explore` (schedule or manual only) shall run each target for `-max_total_time=60` against the seeds and upload `fuzz/artifacts` on failure (14 days). `fuzz/Cargo.toml` is also checked by `cargo-deny` (advisories, bans, sources) and `osv-scanner` and covered by dependabot.
Code: `.github/workflows/ci.yml` (`fuzz`, `fuzz-explore`, `security-deny`), `.github/dependabot.yml`.
Status: done.

## Non-Functional Requirements

- NFR-SEC-001: a crash, panic, OOM or timeout reproduced by any target is a P0/P1 security finding (see `SECURITY.md`; do not file publicly).
- NFR-CI-001: `fuzz` job timeout 30 min; exploration budget 60 s per target.

## Data Model

Inputs are raw `&[u8]`; structured targets derive values via `arbitrary::Unstructured`.

## Edge Cases

| Case | Handling |
|------|----------|
| empty input to `secure_decompress` | early return (`split_first` is `None`) |
| non-JSON bytes in `streaming_decompressor` | lossy UTF-8 string value |
| `decompress_nested` depth | `selector >> 2` (0..=63) |

## Success Criteria

1. `cargo fuzz build` and seed replay succeed in CI (`fuzz` job).
2. Scheduled `fuzz-explore` finishes without artifacts.

## Agent Boundaries

- Always: add a seed for each new input shape; keep seeds small.
- Ask first: raising exploration time or adding `fuzz-explore` to PR runs.
- Never: commit `corpus/` or `artifacts/`; enable library features in the fuzz crate that change production behaviour unnoticed (this includes `float_roundtrip`, which unifies into the library).

## Open Questions

- No fuzz target for `pjs-domain` deserialisation (`MAX_DESERIALIZE_DEPTH` guard), `SecurityValidator`, schema validation, or the WASM crate.
- The seed replay validates only that targets do not crash on seeds; no coverage threshold.
- Fuzz crate declares no `rust-version`; CI uses nightly (required by cargo-fuzz).

## See Also

[[plan]] | [[078-ci-pipeline/spec]] | `crates/pjs-core/src/compression/secure.rs` | `crates/pjs-core/src/parser/`
