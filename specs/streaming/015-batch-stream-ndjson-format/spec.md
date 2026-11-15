---
aliases:
  - BatchFrameStream NDJSON
tags:
  - sdd
  - spec
  - streaming
  - http
created: 2026-04-28
status: implemented
priority: P2
issue: "#167"
pr: "#207"
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
  - "[[014-stream-poll-waker-contract/spec|014 Waker contract]]"
code:
  - crates/pjs-core/src/infrastructure/http/streaming.rs
verified: 2026-11-15
---

# Feature: BatchFrameStream NDJSON Output Format

> [!info] Verification (2026-11-15)
> **State: implemented.** `StreamFormat::Json` and `StreamFormat::NdJson` produce identical NDJSON-of-objects bytes in `BatchFrameStream`; `is_first_batch` no longer exists.
>
> **Evidence**
> - Issues #162 and #167 CLOSED (2026-04-28). PR #207 (`107d2ea`), CHANGELOG 0.5.2 "**BREAKING** (#167): `BatchFrameStream` with `StreamFormat::Json` now emits one JSON object per line per frame".
> - `streaming.rs`: private `format_batch_owned(frames, format)` - `Json | NdJson` -> `sonic_rs::to_vec(frame)` + `\n` per frame; `ServerSentEvents` -> `data: <json>\n\n`; `Binary` -> one `sonic_rs::to_vec(frames)` JSON array.
> - `BatchFrameStream::content_type()` promotes `Json` to `application/x-ndjson`; `StreamFormat::content_type()` still returns `application/json` for `Json`.
> - Tests: `test_batch_frame_stream_multiple_batches`, `test_batch_stream_ndjson_objects_per_line`, `test_batch_stream_emits_only_full_batches_under_pending`.
> - The type is now live: route `GET /pjs/sessions/{session_id}/streams/{stream_id}/frames/stream` (#511/#512, CHANGELOG 0.7.0) uses `BatchFrameStream`.
>
> **Drift from original spec**
> - `format_batch` became the free function `format_batch_owned` returning `Vec<u8>` (bytes, for future compression, #226), not `String`.
> - Spec said Binary is out of scope; the current `Binary` variant emits a JSON array (not a binary protocol) despite the `application/octet-stream` content type. [NEEDS CLARIFICATION: intended?]
> - `StreamFormat::Json` itself is not redefined; only `BatchFrameStream` promotes the content type.
> - `AdaptiveFrameStream` and `PriorityFrameStream` referenced in the original no longer exist (#521).

## Overview

The old chunked-array format (first batch `[...]`, later `,{...}`) was unparseable incrementally. Goal: one JSON object per line, standard `application/x-ndjson`.

## Functional Requirements

### FR-001: Json emits NDJSON-of-objects
Status: done.

### FR-002: Json and NdJson converge
Status: done (identical bytes; content type differs only in `StreamFormat::content_type()`, equal in `BatchFrameStream::content_type()`).

### FR-003: `is_first_batch` removed
Status: done (`BatchFrameStream` fields: `inner`, `format`, `batch_size`).

### FR-004: SSE unchanged (`data: {...}\n\n`)
Status: done.

### FR-005: Regression test for multi-batch validity
Status: done (`test_batch_frame_stream_multiple_batches` parses every line as a JSON object).

## Non-Functional Requirements

- `BatchFrameStream::new(stream, format, batch_size)` signature preserved. Status: done.
- Empty batches: the final flush is guarded by `!batch.is_empty()`, so no empty item is yielded. Status: done.
- Breaking content-type change documented in CHANGELOG. Status: done.

## Data Model

`StreamFormat { Json, NdJson, ServerSentEvents, Binary }` (`Copy`), `StreamTransportError { Serialization, Io, BufferOverflow, StreamClosed }`.

## Success Criteria

- Each non-empty line of any Json/NdJson batch parses as a JSON object; N frames produce N lines across `ceil(N/batch_size)` items.

## Agent Boundaries

- Always: keep Json/NdJson wire bytes identical.
- Ask first: changing the `Binary` encoding.
- Never: emit leading commas or array wrapping for Json/NdJson.

## Open Questions

- `batch_size == 0` is not guarded in `BatchFrameStream::new`: `batch.len() >= 0` is true after the first push, so each frame becomes its own batch (no panic, but undocumented). [NEEDS CLARIFICATION]
- `StreamFormat::Json` `content_type()` is `application/json` outside `BatchFrameStream`; `create_streaming_response` callers must use the right helper.

## See Also

- [[014-stream-poll-waker-contract/spec|014]]
- [[035-streaming-module-unreachable/spec|035 Streaming module reachability]]
- NDJSON: https://ndjson.com/
