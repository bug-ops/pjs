---
tags:
  - sdd
  - delta
created: 2026-11-18
status: merged
related:
  - "[[059-crate-api-errors-features/spec|spec]]"
---

# Delta: error-and-api-enum-changes

Merged into [[059-crate-api-errors-features/spec|spec]] on 2026-11-18 (#698, #700, #706, #707, #718).

## MODIFIED
- FR-005 `PjsError::FrameSequence(FrameSequenceError)` is added (`UnexpectedFrameId { expected, received }`, `FramingMismatch { expected, received }`; `#[non_exhaustive]`). `InvalidStreamOptions` is removed (options are validated at decode time, #700). `is_validation_error` is `InvalidSessionId | Patch | FrameSequence`, and `category()` returns `validation` for `FrameSequence`. `FrameKind` mirrors `websocket::Framing` so the error module does not depend on features.
- FR-005 `PatchError::UnsupportedSegment` is removed (#699).
- FR-003 websocket re-exports: `MaxFrameSize`, `SendOutcome`, `DropReason`; `StreamOptions::patch()`.
- FR-002 `JsonPath` and `PathSegment` come from `pjson-rs-domain` with the infallible, any-key contract of spec 042.
