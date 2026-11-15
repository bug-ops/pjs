---
aliases:
  - create_streaming_response_with_content_type
  - Streaming Content-Type helper
tags:
  - sdd
  - spec
  - http
  - stream
created: 2026-04-29
status: implemented
priority: P2
issue: "#215"
pr: "#217"
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
  - "[[015-batch-stream-ndjson-format/spec|Spec 015]]"
  - "[[024-compress-owned-bytes-return/spec|Spec 024]]"
code:
  - crates/pjs-core/src/infrastructure/http/streaming.rs
  - crates/pjs-core/src/infrastructure/http/mod.rs
verified: 2026-11-15
---

# Spec 025: create_streaming_response_with_content_type Helper

> [!info] Verification (2026-11-15)
> **State: implemented.** Issue #215 CLOSED 2026-04-28; PR #217 (`d4c7bd7`).
>
> Evidence: `pub fn create_streaming_response_with_content_type<S>(stream: S, content_type: &str) -> Result<Response, StreamTransportError>` in `streaming.rs`, re-exported from `infrastructure::http` (`mod.rs` `pub use streaming::{...}`). Tests `test_create_streaming_response_with_content_type_uses_explicit_type` and `test_create_streaming_response_uses_format_content_type`. Doc comment has an `# Examples` section.
>
> Drift from original spec:
> - Signature differs: `content_type: &str` (not `&'static str`); stream item is `Result<Vec<u8>, StreamTransportError>` after #229; returns `Result<Response, StreamTransportError>`, not `Response`.
> - Helper also sets `Cache-Control: no-cache`.
> - The `GET .../frames/stream` handler (`crates/pjs-core/src/infrastructure/http/handlers/streams.rs`) calls the helper with `batch.content_type()` for `StreamFormat::Json` and `create_streaming_response` for the other formats.

## Problem

`BatchFrameStream::content_type()` returns `application/x-ndjson` for `StreamFormat::Json` (the wire format is NDJSON of objects), but `create_streaming_response(stream, format)` derives the header from `format.content_type()`, giving `application/json`.

## Functional Requirements

### FR-1 Public helper taking an explicit content type
Status: done.

### FR-2 Simplest API preferred over trait-based overload
Status: done (explicit string argument).

### FR-3 Batch-stream handlers use `batch.content_type()`
Status: done (`handlers/streams.rs`).

### FR-4 Existing `create_streaming_response` unchanged
Status: done.

## Acceptance Criteria

- [x] Test asserts `Content-Type: application/x-ndjson` for a `BatchFrameStream` with `StreamFormat::Json`.
- [x] Existing streaming response tests pass.
- [x] Function is `pub` with doc comment and `# Examples`.

## See Also

- [[015-batch-stream-ndjson-format/spec|Spec 015]], [[024-compress-owned-bytes-return/spec|Spec 024]]
