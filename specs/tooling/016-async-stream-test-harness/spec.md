---
aliases:
  - PendingThenReady Test Harness
tags:
  - sdd
  - spec
  - testing
  - streaming
created: 2026-04-28
status: partial
priority: P2
issue: "#168"
pr: "#207"
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
  - "[[014-stream-poll-waker-contract/spec|014 Waker contract]]"
  - "[[012-websocket-integration-test-infra/spec|012 WebSocket test infra]]"
code:
  - crates/pjs-core/src/infrastructure/http/streaming.rs
  - crates/pjs-core/Cargo.toml
verified: 2026-11-15
---

# Feature: Async Stream Test Harness

> [!info] Verification (2026-11-15)
> **State: partial.** The `PendingThenReady` helper and a pending-path test exist for `BatchFrameStream`, the only remaining stream adapter. Tests for `PriorityFrameStream`/`AdaptiveFrameStream` are moot (types removed in #521).
>
> **Evidence**
> - Issue #168 CLOSED (2026-04-28). Delivered in PR #207 (`107d2ea`); `git log -S PendingThenReady` shows `107d2ea` and `3eb708b` (#521, test cleanup).
> - `crates/pjs-core/Cargo.toml`: `tokio-test = { workspace = true }` (dev-dependency, workspace `0.4`).
> - `streaming.rs` `#[cfg(test)]`: struct `PendingThenReady<I: Iterator>` (`pending_per_item`, `done` short-circuit, `cx.waker().wake_by_ref()` before each `Poll::Pending`); test `test_batch_stream_emits_only_full_batches_under_pending` uses it via `tokio_test::block_on`.
>
> **Drift from original spec**
> - Helper lives in the existing `#[cfg(test)]` module (as specified); there is no `tokio_test::task::spawn`/`assert_pending!` usage in the repo.
> - No explicit test for partial-batch flush under pending, nor for empty input; `test_batch_frame_stream_multiple_batches` covers a remainder batch with `stream::iter` (never pending).
> - Waker-correctness is now guaranteed structurally by `async-stream` ([[014-stream-poll-waker-contract/spec|014]]), reducing the harness's importance.

## Overview

Existing tests used `futures::stream::iter`, so `poll_next` never saw `Pending`. Goal: a deterministic helper to drive adapters through pending/ready interleavings without a runtime.

## Functional Requirements

### FR-001: `tokio-test` dev-dependency
Status: done.

### FR-002: `PendingThenReady<I>` helper
Status: done.

### FR-003: PriorityFrameStream tests
Status: superseded (type removed in #521).

### FR-004: BatchFrameStream tests
Status: partial. Done: no leading commas, valid JSON per line (`test_batch_stream_ndjson_objects_per_line`), only full batches under pending. Open: explicit partial-flush-under-pending test.

### FR-005: AdaptiveFrameStream tests
Status: superseded (type removed in #521).

### FR-006: Helpers in `#[cfg(test)]`, no new public API
Status: done.

## Non-Functional Requirements

- No tokio runtime requirement: `tokio_test::block_on` is used for the pending test. Status: done.

## Success Criteria

- `cargo nextest run -p pjson-rs --all-features` runs `streaming::tests::*` green.

## Agent Boundaries

- Always: reuse `PendingThenReady` for any new stream adapter test.
- Never: assert only on `stream::iter`-backed inputs for stateful adapters.

## Open Questions

- Add the partial-flush-under-pending case to close FR-004. [NEEDS CLARIFICATION: worth a follow-up issue?]
- The helper is private to `streaming.rs`; other modules with custom streams cannot reuse it.

## See Also

- [[014-stream-poll-waker-contract/spec|014]], [[015-batch-stream-ndjson-format/spec|015]]
