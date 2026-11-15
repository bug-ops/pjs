---
aliases:
  - Stream Waker Contract
  - poll_next Waker Violations
tags:
  - sdd
  - spec
  - streaming
  - async
created: 2026-04-28
status: obsolete
priority: P2
issue: "#161"
pr: "#207"
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
  - "[[015-batch-stream-ndjson-format/spec|015 Batch stream NDJSON]]"
  - "[[016-async-stream-test-harness/spec|016 Async stream test harness]]"
code:
  - crates/pjs-core/src/infrastructure/http/streaming.rs
verified: 2026-11-15
---

# Feature: Stream poll_next Waker Contract (Detection and Prevention)

> [!info] Verification (2026-11-15)
> **State: obsolete (fixed, then target code removed).** The defects were fixed by replacing hand-written `poll_next` with `async_stream::try_stream!`; `PriorityFrameStream` and `AdaptiveFrameStream` were later deleted entirely. Only `BatchFrameStream` remains.
>
> **Evidence**
> - Issues #161, #162, #166 CLOSED (2026-04-28). PR #207 (`107d2ea`, "migrate stream adapters to async-stream, fix NDJSON format, add waker test harness"); CHANGELOG 0.5.2 Fixed: stream adapters migrated "to eliminate latent waker-contract bugs (#166)"; Removed: direct `Stream` impls on the three types.
> - PR #521 (`3eb708b`, "remove dead frame streams") removed `PriorityFrameStream` and `AdaptiveFrameStream` (CHANGELOG 0.7.0 **BREAKING**: only used by tests, never on a production route).
> - `crates/pjs-core/src/infrastructure/http/streaming.rs`: `BatchFrameStream::into_stream` uses `try_stream!` (`async-stream = "0.3"` workspace dep, optional in `pjs-core`, enabled by `http-server`). No manual `poll_next` remains outside the test helper `PendingThenReady`.
>
> **Drift from original spec**
> - Prevention chosen: `async-stream`, not `futures-async-stream`.
> - Regression test for `PriorityFrameStream` pending path cannot exist (type removed); `tokio_test::task::spawn` / `assert_pending!` are not used anywhere in `crates/`.
> - `streaming.rs` line numbers and `is_first_batch` cited in the original are gone.

## Overview

Original problem: `PriorityFrameStream::poll_next` returned `Poll::Ready(None)` while the inner stream was `Pending` (#161), silently dropping undelivered frames; `BatchFrameStream` kept stale `is_first_batch` state (#162). Goal was to fix and prevent waker-contract bugs structurally.

## Functional Requirements (original, current status)

### FR-001: Relay `Pending` when inner is pending and buffer empty
Status: superseded. Generator semantics of `try_stream!` make premature `Ready(None)` unrepresentable; the type that had the bug is deleted.

### FR-002: `Ready(None)` only after inner exhaustion and drained buffer
Status: superseded (same reason). `BatchFrameStream` flushes the remainder batch after `inner.next()` returns `None`.

### FR-003: Track inner termination separately
Status: superseded (no manual state machine).

### FR-004: Evaluate macro-based streams
Status: done (`async-stream::try_stream!`).

### FR-005: Pending-path regression test using `tokio_test::task::spawn`
Status: superseded. Equivalent coverage uses `PendingThenReady` + `tokio_test::block_on` (see [[016-async-stream-test-harness/spec|016]]).

## Non-Functional Requirements

- Public builder constructors unchanged; consumers use `.into_stream()` (CHANGELOG 0.5.2, breaking for direct `Stream` users).

## Success Criteria

- `test_batch_stream_emits_only_full_batches_under_pending` passes (6 frames, batch 3, 2 pending polls per item -> exactly 2 batches).

## Agent Boundaries

- Always: write new `Stream` adapters with `async-stream` macros, not hand-written `poll_next`.
- Never: add a manual `poll_next` without a `PendingThenReady`-style test.

## Open Questions

- None open; keep this spec as history of the rationale.

## See Also

- [[015-batch-stream-ndjson-format/spec|015]], [[016-async-stream-test-harness/spec|016]]
- rust-clippy#6314 (no waker-contract lint)
