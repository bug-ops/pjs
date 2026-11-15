---
aliases:
  - Graceful Shutdown
  - serve_with_limits Drain
tags:
  - sdd
  - spec
  - http
  - websocket
  - reliability
created: 2026-11-15
status: draft
priority: P2
issue: "#590"
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
  - "[[037-graceful-shutdown-serve-with-limits/plan|plan]]"
  - "[[062-http-serve-connection-limits/spec|062 serve_with_limits]]"
  - "[[066-websocket-server-transport/spec|066 WebSocket server transport]]"
code:
  - crates/pjs-core/src/infrastructure/http/serve.rs
  - crates/pjs-core/src/infrastructure/websocket/server.rs
  - crates/pjs-demo/src/servers/interactive_demo.rs
  - crates/pjs-demo/src/servers/simple_demo.rs
  - crates/pjs-demo/src/servers/websocket_streaming.rs
verified: 2026-11-15
---

# Feature: Graceful Shutdown for serve_with_limits

> [!info] Verification (2026-11-15)
> **State: not implemented (draft, still valid).**
>
> **Evidence**
> - GitHub issue #590 ("feat(http): graceful shutdown path for serve_with_limits and WebSocket tasks") exists, is OPEN, labels `enhancement`, `infrastructure`, `P2`, created 2026-11-14. It carries this spec in its body. No PR references it; `git log` and CHANGELOG `[Unreleased]` have no shutdown entry.
> - `rg -i "shutdown|cancellation"` over `crates/pjs-core/src` and `crates/pjs-demo/src` has no hit in `serve.rs`, `websocket/server.rs` or the demo servers (only unrelated "cancellation" comments in `dictionary_store.rs`).
> - Current `serve_with_limits(listener: TcpListener, router: Router, limits: ConnectionLimits) -> std::io::Result<()>` is an infinite `loop`: acquires an `Arc<Semaphore>` permit, calls `accept_with_retry`, applies the per-IP `RateLimitGuard`, then `tokio::spawn`s a fire-and-forget task (peek for first byte under `header_read_timeout`, `serve_connection_with_upgrades`, optional `tokio::time::timeout(max_connection_duration, conn)`). The only `return Ok(())` is the unreachable closed-semaphore branch.
> - `AxumWebSocketTransport::with_rate_limit_config` spawns a session-cleanup task and `handle_socket` spawns a per-connection task, with no shared cancellation signal; `handle_socket` already sends `Message::Close` frames for its own error paths.
> - Dependencies today: `hyper-util = { features = ["server-auto", "tokio", "service"] }` (no `server-graceful`); `tokio-util` is not a dependency of `pjs-core` (`rg tokio-util Cargo.toml crates/pjs-core/Cargo.toml` is empty).
> - Three callers adopt the signature: `crates/pjs-demo/src/servers/{interactive_demo,simple_demo,websocket_streaming}.rs`; tests in `crates/pjs-core/tests/http_serve_connection_limits.rs`.
>
> **Drift from original spec**
> - Source gave no issue number; now #590. Branch should follow `.claude/rules/branching.md` (`feat/issue-590/graceful-shutdown`).
> - `.local/specs/constitution.md` does not exist at the source; [[constitution]] now exists in `specs/`.
> - Related open issues touching the same function, to avoid conflicting edits: #618 (no request header size cap in `ConnectionLimits`), #602 (`AxumWebSocketTransport::new` panics outside a Tokio runtime), #572 (WebSocket session lifecycle follow-ups).
> - All FRs below are `Status: open`.

> [!info] Metadata
> **Author**: sdd-orchestrator (continuous improvement cycle)
> **Type**: enhancement, P2
> **Branch**: `feat/issue-TBD/graceful-shutdown-serve-with-limits`
> **Code**: `crates/pjs-core/src/infrastructure/http/serve.rs` (`serve_with_limits`), `crates/pjs-core/src/infrastructure/websocket/server.rs` (`handle_socket`)

## 1. Overview

### Problem Statement
`serve_with_limits` is the project's custom accept loop (replacing `axum::serve`) that enforces connection limits, per-IP caps, header-read timeouts and max connection duration. It has no exit path: its doc says it "runs until the process is torn down". Every accepted connection is a fire-and-forget `tokio::spawn`, and the WebSocket transport spawns per-connection tasks with no shared cancellation signal. PJS responses are long-lived SSE/NDJSON streams and WebSockets, so on SIGTERM or a rolling deploy every in-flight stream is cut mid-frame. A consumer cannot tell that apart from a network fault and cannot resume cleanly.

Reference capabilities available today: `axum::serve(..).with_graceful_shutdown(signal)` (axum 0.8.9), `hyper_util::server::graceful::GracefulShutdown` (hyper-util 0.1.21, feature `server-graceful`: `watch`, `shutdown`, `count`), `tokio_util::task::TaskTracker` + `CancellationToken`.

### Goal
A library user can trigger shutdown: the server stops accepting new connections, lets in-flight HTTP and WebSocket streams finish (or be told to finish with a terminal signal) within a bounded drain timeout, then `serve_with_limits` returns.

### Out of Scope
- OS signal handling (SIGTERM/Ctrl-C wiring); the caller supplies the trigger.
- Draining the demo servers beyond adopting the new signature.
- Session/stream persistence or resumption after restart.
- HTTP/3, TLS termination, load-balancer readiness endpoints.
- Changing `ConnectionLimits` semantics.

## 2. User Stories

### US-001: Stop accepting on a signal
AS A library user running PJS behind a rolling deploy
I WANT to trigger shutdown programmatically
SO THAT no new connections are accepted while existing work completes.

**Acceptance criteria:**
```
GIVEN serve_with_limits is running with open connections
WHEN shutdown is triggered
THEN the listener stops accepting and new TCP connects are refused or left unaccepted
AND serve_with_limits returns Ok after drain
```

### US-002: Let in-flight HTTP streams finish
AS A stream consumer
I WANT my in-flight SSE/NDJSON response to complete or end with a recognizable terminal marker
SO THAT I can distinguish a deploy from a network fault.

**Acceptance criteria:**
```
GIVEN an open frames/stream response mid-delivery
WHEN shutdown is triggered
THEN the stream is allowed to finish or is closed with a terminal signal within the drain timeout
AND no connection is cut before the drain timeout expires unless it completed
```

### US-003: Close WebSockets politely
AS A WebSocket client
I WANT a Close frame with code 1001 (Going Away)
SO THAT I can reconnect elsewhere without treating it as an error.

**Acceptance criteria:**
```
GIVEN an established WebSocket connection
WHEN shutdown is triggered
THEN the server sends Close(1001) and ends the connection task within the drain timeout
```

### US-004: Bounded drain
AS AN operator
I WANT a hard drain deadline
SO THAT one stuck client cannot block process exit forever.

**Acceptance criteria:**
```
GIVEN a client that never reads its response
WHEN shutdown is triggered and the drain timeout elapses
THEN remaining connections are aborted and serve_with_limits returns with a report of how many were force-closed
```

## 3. Functional Requirements

### FR-001

WHEN the shutdown signal fires THE SYSTEM SHALL stop calling `accept()` and release the listener to the caller no later than function return.

**Priority**: must

**Status**: open

### FR-002

WHEN the shutdown signal fires THE SYSTEM SHALL ask every tracked HTTP connection to shut down gracefully (finish in-flight requests, disable keep-alive, send HTTP/2 GOAWAY).

**Priority**: must

**Status**: open

### FR-003

WHEN the shutdown signal fires THE SYSTEM SHALL notify every active WebSocket connection so it sends Close code 1001 and terminates.

**Priority**: must

**Status**: open

### FR-004

WHEN the drain timeout elapses with connections still open THE SYSTEM SHALL abort them and return an outcome that reports the forced count.

**Priority**: must

**Status**: open

### FR-005

WHEN all connections finish before the drain timeout THE SYSTEM SHALL return immediately with a clean outcome.

**Priority**: must

**Status**: open

### FR-006

THE SYSTEM SHALL expose the shutdown trigger and the drain timeout as dedicated types (not raw futures-of-unit or `Duration`/`u64`), with the drain timeout validated at construction.

**Priority**: must

**Status**: open

### FR-007

WHILE shutdown is in progress THE SYSTEM SHALL keep enforcing `ConnectionLimits` (header-read timeout, max connection duration) on connections still draining.

**Priority**: must

**Status**: open

### FR-008

WHEN shutdown fires THE SYSTEM SHALL make the shutdown signal observable by request handlers (via request extensions) so SSE/NDJSON streaming handlers can end the stream with a terminal frame.

**Priority**: should

**Status**: open

### FR-009

WHEN a connection is accepted after shutdown fired but before the loop observed it THE SYSTEM SHALL close it without serving.

**Priority**: should

**Status**: open

### FR-010

WHEN the shutdown signal is already fired at call time THE SYSTEM SHALL return without accepting any connection.

**Priority**: must

**Status**: open

## 4. Non-Functional Requirements

### NFR-REL-001

No spawned connection or WebSocket task outlives `serve_with_limits` return (verified by a task tracker count of zero).

### NFR-PERF-001

The shutdown mechanism adds no measurable overhead to the accept hot path: < 2% change on `cargo bench -p pjs-bench` throughput benchmarks; no new allocation per accepted connection beyond a cheap handle clone.

### NFR-SEC-001

All drain waiting is bounded: `DrainTimeout` has a validated non-zero upper bound; no unbounded wait on untrusted peers. A slow-reading client cannot extend the drain past the deadline.

### NFR-MNT-001

The adapter stays in `infrastructure/`; no shutdown types are imported by `domain/` or `application/` (hexagonal rule). The WebSocket transport receives the signal through its infrastructure-level API only.

### NFR-COM-001

MSRV 1.89 and nightly toolchain; new dependencies limited to what is justified in the plan.

## 5. Data Model

| Entity | Description | Key Attributes |
|--------|-------------|----------------|
| `ShutdownSignal` | Cloneable, typed handle: one side triggers, many observe | triggered state, child-derivable |
| `DrainTimeout` | Validated drain deadline newtype | duration, bounded (non-zero, max cap) |
| `ShutdownOutcome` | Result of a completed serve | `Drained` or `Forced { aborted: usize }` |

## 6. Edge Cases and Error Handling

| Scenario | Expected Behavior |
|----------|-------------------|
| Signal fires while the loop waits on the connection semaphore | The wait is raced with the signal; loop exits without accepting |
| Signal fires during `accept_with_retry` backoff sleep | Backoff is raced with the signal; exits promptly |
| Signal fires before first request on a connection (header-read peek pending) | Connection dropped immediately; nothing to drain |
| Client keeps an SSE stream open forever | Terminal frame requested (FR-008); aborted at drain deadline |
| Upgraded WebSocket (hyper no longer tracks it after upgrade) | Tracked separately via the WS task tracker (FR-003) |
| `max_connection_duration` shorter than drain timeout | Connection-duration deadline still wins (FR-007) |
| Signal triggered twice | Idempotent |
| Signal handle dropped without triggering | Defined behavior needed: [NEEDS CLARIFICATION: does dropping the last trigger handle count as shutdown, or never-shutdown?] |
| Drain timeout zero | Rejected at construction |

## 7. Success Criteria

| ID | Metric | Target |
|----|--------|--------|
| SC-001 | In-flight stream completion on shutdown within the timeout | 100% in integration test |
| SC-002 | Leaked tasks after return | 0 |
| SC-003 | Return time after shutdown with a stalled client | <= drain timeout + 100 ms |
| SC-004 | Accept-path benchmark delta | < 2% |
| SC-005 | WebSocket close code observed by client | 1001 |

## 8. Agent Boundaries

### Always (without asking)
- Run fmt, clippy (`-D warnings`), nextest and doc-tests per `.claude/rules/branching.md`.
- Add `///` docs with doc-tests for every new `pub` item; update CHANGELOG.md `[Unreleased]` with PR link.
- Keep every wait bounded.

### Ask First
- Adding `tokio-util` or enabling hyper-util `server-graceful` (new dependency or feature).
- Changing the public signature of `serve_with_limits` vs adding a sibling function.
- Defining the wire format of the terminal frame for SSE/NDJSON.

### Never
- Put shutdown types in `domain/` or `application/`.
- Use raw `Duration`/`bool`/`Notify` in public signatures where a newtype fits.
- Add `unwrap`/`expect` on reachable paths.
- Touch `domain/ports/gat/` (critical perf path).

## 9. Open Questions

> [!question] Open items
> - [NEEDS CLARIFICATION: terminal frame format for SSE (named event?) and NDJSON (final line object?) and whether it is part of the public protocol]
> - [NEEDS CLARIFICATION: default `DrainTimeout` value and max cap (suggest 30 s default, 10 min cap)]
> - [NEEDS CLARIFICATION: break `serve_with_limits` signature or add a sibling `serve_with_graceful_shutdown`? Pre-1.0 permits a break]
> - [NEEDS CLARIFICATION: semantics when the trigger handle is dropped (see edge cases)]
> - [NEEDS CLARIFICATION: should `AxumWebSocketTransport` shutdown be usable without `serve_with_limits` (e.g. embedded under `axum::serve`)?]

## 10. See Also

- [[constitution]] — project principles
- [[MOC-specs]] — all specifications
- [[037-graceful-shutdown-serve-with-limits/plan|plan]] — technical plan
- [[037-graceful-shutdown-serve-with-limits/tasks|tasks]] — implementation tasks
- [[062-http-serve-connection-limits/spec|062 serve_with_limits connection limits]], [[066-websocket-server-transport/spec|066 WebSocket server transport]]
- Issue #590; related #618, #602, #572
