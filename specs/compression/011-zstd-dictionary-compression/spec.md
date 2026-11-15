---
aliases:
  - Zstd Dictionary Compression
  - ZstdDictCompressor
tags:
  - sdd
  - spec
  - compression
  - http
created: 2026-04-28
status: partial
priority: P3
issue: "#144"
pr: "#222"
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
  - "[[002-real-compression-algorithms/spec|002 Real compression algorithms]]"
code:
  - crates/pjs-core/src/compression/zstd.rs
  - crates/pjs-core/src/compression/secure.rs
  - crates/pjs-core/src/domain/ports/dictionary_store.rs
  - crates/pjs-core/src/infrastructure/repositories/dictionary_store.rs
  - crates/pjs-core/src/infrastructure/http/handlers/dictionary.rs
verified: 2026-11-15
---

# Feature: Zstd Trained-Dictionary Compression for Frame Transport

> [!info] Verification (2026-11-15)
> **State: partial.** The compressor, `SecureCompressor` integration, store port/adapter and HTTP dictionary endpoint are implemented. The `X-PJS-Dictionary-URL` header and the performance NFRs (training time, ratio vs gzip) are not implemented/measured.
>
> **Evidence**
> - Issue #144 CLOSED (2026-04-28); prerequisite #114 (real flate2/brotli codecs) CLOSED. Delivered by PR #222 (`8b15839`, "add ZstdDictCompressor ... (#144)"), CHANGELOG 0.5.2.
> - `compression/zstd.rs`: `ZstdDictionary` newtype (invariant `len() <= MAX_DICT_SIZE` = 112 KiB, `from_bytes`, `as_bytes`), `ZstdDictCompressor::{train, compress, compress_with_level, decompress}`, consts `MAX_DICT_SIZE`, `N_TRAIN` (32), `DEFAULT_LEVEL` (3). `train` rejects < 8 samples and calls `zstd::dict::from_samples`.
> - `compression/secure.rs`: `ByteCodec::ZstdDict(Arc<ZstdDictionary>)`; decode routes through the streaming decoder + bomb detector.
> - `compression/mod.rs`: `#[cfg(all(feature = "compression", not(target_arch = "wasm32")))] pub mod zstd;`. `Cargo.toml`: `compression = ["dep:brotli", "dep:flate2", "dep:zstd"]`; `zstd = "0.14"` with `zdict_builder`.
> - Port `DictionaryStore` / `NoopDictionaryStore` (`domain/ports/dictionary_store.rs`), adapter `InMemoryDictionaryStore` (`infrastructure/repositories/dictionary_store.rs`), handler `get_session_dictionary` (`http/handlers/dictionary.rs`, `Content-Type: application/zstd-dictionary`, `Cache-Control: private, max-age=300`, 404 until trained). `PjsAppState::with_dictionary_store` wires it; `SessionCommandHandler::with_dictionary_store` feeds frame payloads as samples.
> - `rg "Dictionary-URL" crates` finds no hits; no zstd benchmark exists in `crates/pjs-bench`.
>
> **Drift from original spec**
> - Types are `ZstdDictionary` newtype + stateless `ZstdDictCompressor` (typed, with `train(&[Vec<u8>], max_dict_size)`), not a `Vec<u8>`-only API.
> - `zstd` crate 0.14, not 0.13 (CHANGELOG) or unspecified.
> - Training is triggered by the store after `N_TRAIN` = 32 samples, not "first N frames" configured by callers.
> - `decompress` in `zstd.rs` takes an explicit `max_output`; the bomb-protected path is in `SecureCompressor`.

## Overview

`SchemaCompressor` works at JSON level; byte-level transport compression needed a trained-dictionary option because PJS frames are small (50-500 bytes) and share field names within a session. Goal: per-session zstd dictionary, trained from early frames, served over HTTP, with bomb protection intact. Out of scope: client-side decoding library.

## User Stories

- As an operator, I want repeated field names not retransmitted on every frame.
- As a client developer, I want to fetch the session dictionary once over HTTP.

## Functional Requirements

### FR-001: ZstdDictCompressor
Train from samples, compress and decompress with a dictionary.
Status: done. Code: `compression/zstd.rs`.

### FR-002: SecureCompressor backend
Status: done (`ByteCodec::ZstdDict`). Code: `compression/secure.rs`.

### FR-003: Dictionary HTTP endpoint
`GET /pjs/sessions/{session_id}/dictionary` returns raw bytes as `application/zstd-dictionary`.
Status: done. Code: `http/handlers/dictionary.rs`, route in `http/axum_adapter.rs`. Error text is generic (no `Display` leak; see CHANGELOG 0.6.3).

### FR-004: `X-PJS-Dictionary-URL` header on the SSE stream
Status: open. No code emits this header. [NEEDS CLARIFICATION: still wanted given the #511 streaming route negotiates NDJSON/SSE?]

### FR-005: Feature and target gating
Status: done. `compression` feature and `not(target_arch = "wasm32")`.

### FR-006: Bomb protection on decompression
Status: done. `ByteCodec::ZstdDict` decode counts bytes via the bomb detector; `InMemoryDictionaryStore::new` takes a `CompressionBombDetector`.

## Non-Functional Requirements

- NFR-001: dictionary <= 112 KiB. Status: enforced by `ZstdDictionary` type invariant.
- NFR-002: train < 10 ms for 1000 x 1 KB frames. Status: not benchmarked (no zstd bench in `pjs-bench`).
- NFR-003: >= 20% smaller than gzip. Status: not measured.

## Data Model

`ZstdDictionary(Vec<u8>)`, `DictionaryStore` trait (resolve/accumulate per `SessionId`), `InMemoryDictionaryStore` (DashMap-backed per-session state with TTL cleanup via `cleanup_expired`).

## Edge Cases

| Case | Behaviour |
|------|-----------|
| < 8 training samples | `Error::CompressionError` |
| Dictionary > 112 KiB | rejected by `ZstdDictionary::from_bytes` |
| Not yet trained (< `N_TRAIN` samples) | HTTP 404 |
| Malformed `session_id` | `PjsError::InvalidSessionId` -> 400 |

## Success Criteria

- Round-trip through `ZstdDictCompressor` and `SecureCompressor` passes; endpoint is 404 before and 200 after training (`dictionary_endpoint_becomes_reachable_after_training`).

## Agent Boundaries

- Always: keep zstd code behind `compression` + non-wasm32.
- Ask first: changing `MAX_DICT_SIZE`, `N_TRAIN`.
- Never: decompress without the bomb detector on request paths.

## Open Questions

- FR-004 and NFR-002/003 are unverified.
- `DictionaryStore` is a boxed-future port deliberately outside `ports::gat`.

## See Also

- [[002-real-compression-algorithms/spec|002 Real compression algorithms]]
