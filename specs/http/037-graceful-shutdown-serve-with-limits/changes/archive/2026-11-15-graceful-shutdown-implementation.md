---
tags:
  - sdd
  - delta
created: 2026-11-15
status: merged
related:
  - "[[037-graceful-shutdown-serve-with-limits/spec|spec]]"
---

# Delta: graceful-shutdown-implementation

Merged into [[037-graceful-shutdown-serve-with-limits/spec|spec]] on 2026-11-15.

## MODIFIED
- Status: draft -> implemented (#590, #618, #611). FR-001..FR-010 implemented; FR-008 as a `ShutdownSignal` request extension only.
- Data model: `ShutdownSignal` has no `child()`; `ShutdownOutcome::Forced { aborted: NonZeroUsize }`; `ShutdownConfig` added.
- Out of Scope: draining the demo servers is replaced by DV2.

## ADDED
- `ConnectionLimits.max_header_bytes` (`HeaderSizeLimit`, minimum 8192, #618), documented in 062.

## REMOVED
- Open questions on drain defaults (30 s, max 600 s), signature break (in place), dropped trigger (never shuts down) and `tokio-util` (approved).

## Deviations
- DV1: no `child()` and no hyper-util `GracefulShutdown`; per-connection `select!` plus `graceful_shutdown()`, one shared signal, `TaskTracker`.
- DV2: Ctrl-C triggers `serve_with_limits` shutdown in all three demos; `with_shutdown` on the WebSocket transport in `interactive-demo` only.
- Deferred: terminal SSE/NDJSON frame (#625).
