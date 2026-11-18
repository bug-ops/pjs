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
issue: "#575, #660"
pr: "#577"
code:
  - fuzz/Cargo.toml
  - fuzz/fuzz_targets/parser_zero_copy.rs
  - fuzz/fuzz_targets/parser_partial.rs
  - fuzz/fuzz_targets/secure_decompress.rs
  - fuzz/fuzz_targets/streaming_decompressor.rs
  - fuzz/fuzz_targets/ws_message_decode.rs
  - fuzz/fuzz_targets/zstd_dict_codec.rs
  - fuzz/fuzz_targets/client_apply_patches.rs
  - fuzz/fuzz_targets/planner_round_trip.rs
  - crates/pjs-core/src/infrastructure/websocket/fuzzing.rs
  - CONTRIBUTING.md
  - fuzz/src/lib.rs
  - fuzz/seeds
verified: 2026-11-18
---

# Spec 076: cargo-fuzz targets and CI replay

Part of [[MOC-specs]]; see [[constitution]] and `SECURITY.md`.

> [!info] Verification (2026-11-15)
> Implemented. `client_apply_patches` and `planner_round_trip` were added for #660. Fuzz targets were added in #577 (`git log` on `fuzz/`). CI replays committed seeds on every relevant run and explores each target for 60 s on the weekly schedule. `zstd_dict_codec` and the production-float-path oracle for `ws_message_decode` were added for #575.

## Overview

**Problem.** Parsers, decompressors and the WebSocket message codec consume untrusted bytes; panics, hangs or unbounded allocation are DoS vectors.

**Goal.** Eight libFuzzer targets over the untrusted-input entry points, deterministic seed replay on every CI run, short exploration on a schedule.

**Out of scope.** Fuzzing the WASM crate, HTTP handlers, or the TypeScript client.

## Functional Requirements

### FR-001 Standalone fuzz workspace
The `fuzz/` crate (`pjs-fuzz`, `publish = false`, edition 2024) shall be its own workspace (`[workspace] members = ["."]`), excluded from the root workspace (`exclude = ["crates/pjs-js-client", "fuzz"]`), with its own `Cargo.lock`. It depends on `libfuzzer-sys`, plain `serde_json` (no `float_roundtrip`, so the production float path runs), and `pjson-rs` by path with `default-features = false`, features `compression`, `partial-parse`, `http-server`, `websocket-client`, `websocket-server`.
Code: `fuzz/Cargo.toml`.
Status: done.

### FR-001a Shared oracle library
The fuzz crate shall also expose a crate-private library target `pjs_fuzz` (`fuzz/src/lib.rs`, `publish = false`) holding the oracles shared by targets: `reparsed` (the document after one serialize/parse round trip, the exact oracle of `planner_round_trip`) and `assert_decode_error` (accepts only `Error::CompressionBomb` or `Error::Codec`). It exists because a `no_main` fuzz binary cannot run unit tests; the helpers' unit tests run in CI.
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
The target shall decode bytes as `pjson_rs::infrastructure::WsMessage` and, on success, assert that the value of the re-decoded message equals exactly `pjs_fuzz::reparsed(&value)`, i.e. the value after one serialize/parse cycle through the production float path (#690, the oracle `planner_round_trip` uses, #684). Both sides carry the same float re-parse drift, so a benign large integer no longer fails the oracle. `expect` calls are also part of the oracle.
Code: `ws_message_decode.rs`.
Status: done. The fuzz crate does not enable `float_roundtrip` / `pjson-rs/float-roundtrip`, so the target runs the production (default) float path. The opt-in feature path is covered by `crates/pjs-core/tests/float_roundtrip.rs` under `--all-features`. The helper `reparsed` lives in the fuzz crate's library target (`fuzz/src/lib.rs`, because a `no_main` fuzz binary cannot run unit tests). The former `approx_eq` oracle (1 ULP tolerance) was removed in #721 because no target used it.

### FR-007 Seed corpora
Committed seeds shall live in `fuzz/seeds/<target>/` (3 for `parser_partial`, 3 `parser_zero_copy`, 4 `secure_decompress` (one per codec), 3 `streaming_decompressor`, 9 `ws_message_decode` (including `huge_integer`, a `StreamInit` whose `data` is a 199-digit integer: red on the former 1-ULP oracle, green on the exact one), 5 `zstd_dict_codec`, 5 `client_apply_patches`, 9 `planner_round_trip`). The first byte of `secure_decompress` seeds is the selector; the first bytes of `streaming_decompressor` seeds select the strategy. `corpus/`, `artifacts/`, `coverage/`, `target/` are git-ignored.
Status: done.

### FR-009 zstd_dict_codec
The target shall build a `ZstdDictionary` once (`LazyLock`) by `ZstdDictCompressor::train` over `N_TRAIN` deterministic JSON samples. The first byte is the selector. Bit 0 = 0: decode the remaining bytes as arbitrary input through `SecureCompressor(ByteCodec::ZstdDict)` `.decompress_protected`, `.decompress_nested` (depth `selector >> 1`) and `ZstdDictCompressor::decompress(payload, dict, 1 MiB)`; the oracle is "no panic, no hang". Bit 0 = 1: compress the payload with `compress_with_level` (level `1 + (selector >> 1) % 3`) and decompress with `decompress_protected`; when it returns `Ok(out)`, `out` shall equal the payload.
Code: `zstd_dict_codec.rs`.
Status: done.

### FR-010 client_apply_patches
The target shall take byte 0 as a mode (bit 0 = 0: one patch per call, bit 0 = 1: the whole `Vec<WirePatch>` in one call), the next two bytes as a little-endian `u16` cap (the byte cap is the value times 16) and decode the rest as `Vec<WirePatch>` JSON. It applies the patches to a `fuzzing::ClientPatchHarness` (the production `ClientStreamSession::apply_patches` accounting). One patch per call: after every `Ok`, `tracked_bytes >= serde_json::to_vec(reconstructed).len()` and `tracked_bytes <= cap`; a `LimitExceeded` refusal must leave `reconstructed` unchanged. Whole batch: `tracked_bytes <= cap` after every outcome, and `tracked_bytes >= serde_json::to_vec(reconstructed).len()` after `Ok` and after `LimitExceeded` (a failing patch may be partly applied, so other errors skip the undercount check). Any other error ends the run (the production session is dropped then). The oracle includes "no panic, no hang".
Code: `client_apply_patches.rs`.
Status: done.

### FR-011 planner_round_trip
The target shall take byte 0 (bit 0 selects `Framing::Merge` or `Patch`) and bytes 1-3 (little-endian 24-bit `max_frame_size`, which covers every size up to the planner's 16 MiB cap), and decode the rest as a JSON document. It builds `StreamOptions` through `StreamOptions::default()` and field assignment (the type is `#[non_exhaustive]`), takes `max_frame_size` through `MaxFrameSize::new` and ends the run when it is below 1024. It calls `fuzzing::plan_messages(document, &options)`, which fails only with `CreateSessionError::PlanRejected` (`InvalidOptions` no longer exists); a rejected plan ends the run. Every planned message shall decode as `WsMessage`, frame ids shall be sequential and only the last message shall set `is_complete`. Every message shall be at most 16 MiB and, unless it carries a single unit (a lone patch or a lone top-level entry), at most `max_frame_size`. Replaying the plan shall reproduce the document exactly after one serialize/parse round trip (`==` against `reparsed(document)`, #684): `StreamPatch` messages through `ClientPatchHarness` with no cap (Patch framing), or `StreamFrame` object payloads merged shallowly onto `{}` (Merge framing, object roots). A non-object root under Merge framing shall plan exactly one complete `StreamFrame` whose payload is the document (compared the same way). For every successful plan in both framings, the summed message lengths shall be at least the compact serialized document length (#687).
Code: `planner_round_trip.rs`.
Status: done.

### FR-012 Fuzzing entry point
The crate-private planner and client session shall be reached only through `#[doc(hidden)] pub mod fuzzing` in `pjson_rs::infrastructure::websocket` (`fuzzing.rs`, compiled when `websocket-client` or `websocket-server` is enabled and `--cfg fuzzing` (set by `cargo fuzz`) or `--cfg pjs_bench` (the `client_apply_patches` bench, #670) is passed, so release builds do not carry it; both cfgs are registered in the workspace `unexpected_cfgs` check-cfg and a `compile_fail` doctest asserts the module is absent without them): `ClientPatchHarness` (feature `websocket-client`; `new`, `apply_patches`, `reconstructed`, `tracked_bytes`) and `plan_messages` (feature `websocket-server`). It adds no behaviour and carries no stability guarantee.
Code: `crates/pjs-core/src/infrastructure/websocket/fuzzing.rs`.
Status: done.

### FR-008 CI integration
CI job `fuzz` (on rust changes, push, schedule, manual) shall: format-check and clippy the fuzz crate, run `cargo test --manifest-path fuzz/Cargo.toml --lib` (unit tests of the shared oracle helpers), `cargo fuzz build --target x86_64-unknown-linux-gnu`, then for every `cargo fuzz list` target replay `fuzz/seeds/$target` with `-runs=0`. Job `fuzz-explore` (schedule or manual only) shall run each target for `-max_total_time=60` against the seeds and upload `fuzz/artifacts` on failure (14 days). `fuzz/Cargo.toml` is also checked by `cargo-deny` (advisories, bans, sources) and `osv-scanner` and covered by dependabot.
Code: `.github/workflows/ci.yml` (`fuzz`, `fuzz-explore`, `security-deny`), `.github/dependabot.yml`.
Status: done.

### FR-013 Running locally
Local exploration shall use the git-ignored scratch corpus `fuzz/corpus/<target>` as the first (writable) corpus directory, because libFuzzer writes new inputs into the first directory it is given: `cargo fuzz run <target> fuzz/corpus/<target> fuzz/seeds/<target> -- -max_total_time=60`. Passing `fuzz/seeds/<target>` first adds generated files to the tracked seeds; only CI (ephemeral checkout) and `-runs=0` replay do that safely. New seeds are minimized with `cargo fuzz cmin` and copied into `fuzz/seeds/<target>` deliberately (#678). Building the fuzz crate outside `cargo fuzz` needs `RUSTFLAGS="--cfg fuzzing"`.
Code: `CONTRIBUTING.md`.
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
| fewer than 3 bytes (`client_apply_patches`) or 4 bytes (`planner_round_trip`) | early return |
| non-object document under Merge framing (`planner_round_trip`) | exactly one complete `StreamFrame` carrying the document |
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
