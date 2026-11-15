---
aliases:
  - maybe_compress_owned Bytes
  - Streaming gzip corruption fix
tags:
  - sdd
  - spec
  - http
  - compression
  - stream
created: 2026-04-29
status: obsolete
priority: P2
issue: "#214"
pr: "#229"
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
  - "[[025-streaming-response-content-type-helper/spec|Spec 025]]"
code:
  - crates/pjs-core/src/infrastructure/http/streaming.rs
verified: 2026-11-15
---

# Spec 024: Fix maybe_compress_owned - Return Bytes Instead of Corrupted String

> [!info] Verification (2026-11-15)
> **State: obsolete (delivered, then the code was removed).** Issue #214 CLOSED 2026-04-28.
>
> Evidence:
> - PR #217 (`d4c7bd7`) first replaced `String::from_utf8_lossy` with `String::from_utf8` (explicit `StreamError::Io` instead of corruption).
> - The actual fix is PR #229 (`7d92f59`, issue #226): the whole streaming pipeline carries `Vec<u8>`; `AdaptiveFrameStream::with_compression(true)` produced decompressible gzip (CHANGELOG, round-trip tests).
> - PR #521 (`3eb708b`) then removed `AdaptiveFrameStream` and `PriorityFrameStream` (test-only, never routed). `rg maybe_compress_owned crates` has no hits. Remaining stream builder `BatchFrameStream::into_stream` yields `Result<Vec<u8>, StreamTransportError>` with no compression; its doc comment says the byte item type "leave[s] room for future per-batch compression (#226)".
>
> Drift from original spec: `Bytes` was not used, `Vec<u8>` was; the compression code path no longer exists in HTTP streaming; `StreamError` was replaced by `StreamTransportError`.

## Original intent (kept for rationale)

Compressed gzip output is binary, not UTF-8; converting it to `String` corrupts it. Requirements were: return bytes when compression is on, assemble the body as bytes, map compression failure to a distinct error, keep the uncompressed path unchanged.

## Requirements status

- FR-1 return bytes from compression: superseded (done in #229, code later removed in #521).
- FR-2 body assembled as bytes: done (`Vec<u8>` items in `create_streaming_response`).
- FR-3 string path allowed when disabled: superseded (no string path remains).
- FR-4 distinct compression error variant: superseded.
- FR-5 no regression to uncompressed path: done.

## Open Questions

- Response compression for the streaming route is not implemented in the stream builder; `middleware.rs` `compression_middleware` only adds hints for clients accepting gzip/deflate. [NEEDS CLARIFICATION: whether streaming-route compression is wanted; if so, file a new spec]

## See Also

- [[025-streaming-response-content-type-helper/spec|Spec 025]], [[002-real-compression-algorithms/spec|Spec 002]]
