---
tags:
  - sdd
  - delta
created: 2026-11-18
status: merged
related:
  - "[[066-websocket-server-transport/spec|spec]]"
---

# Delta: sync-controller-send-outcome-spans

Merged into [[066-websocket-server-transport/spec|spec]] on 2026-11-18 (#705, #706, #708, #711, #715).

## MODIFIED
- FR-004/FR-007/FR-008/FR-009 `AdaptiveStreamController::{start_streaming, handle_frame_ack, remove_session, cleanup_expired_sessions}` and `AxumWebSocketTransport::active_connection_count` are plain `fn` (no await points).
- FR-008 `send_frame` is a plain `fn` returning `SendOutcome { Queued, Dropped(DropReason) }` with `DropReason { NoConnection, Unserializable, OverBudget, ChannelUnavailable }`; it never blocks, and every drop is still logged.
- FR-010 the unused `rate_limit_guard`, `set_rate_limit_guard` and `validate_message` on the transport are removed; `SecureWebSocketHandler` is the standalone helper.
- FR-004/FR-007 each connection runs in a `ws_connection` span (`connection_id`, `client_ip`) and each streaming task in a `ws_session` span (`session_id`, `generation`); the offloaded planner runs inside the caller's span. Websocket log sites use structured fields; peer-supplied text stays length-bounded.
- FR-011 the inline-plan gate adds `NESTED_COST` = 4 once per non-empty container below the document root (empty `{}`/`[]` weigh their bytes; `SIZE_BOUND` 0), measured on the nested-object shapes: they planned 1.17x-1.41x the 32 KiB mixed reference without it and within 0.97x with it (#715). Patch framing is not calibrated.
- Internal: a single admission check enforces the per-connection session cap; the adaptive minimum frame delay is one constant (no behavior change, #711).
- FR-005 `StreamInit` (spec.md:90): the "validates the options" step and the `Error{400, "invalid stream options"}` answer are removed. Invalid options (`max_frame_size` below 1024, `client_fps` 0) fail `WsMessage` decoding and are answered `Error{400, "malformed message"}` with `session_id` null and no session (see the 067 delta); `create_session` only plans and registers.
- Public surface (spec.md:143): `CreateSessionError` is `{ AlreadyExists, TooManySessions, PlanRejected(PlanError) }`; `InvalidOptions` is removed, and so is the conversion into `Error::InvalidStreamOptions`, which no longer exists.
- Error table (spec.md:155) and plan.md (lines 51, 59): the "Invalid `StreamOptions`" row is removed; the `create_session` contract has no `InvalidOptions` outcome and "options valid" is no longer an insert condition.
- Connection and session logs use structured fields (`connection_id`, `client_ip`, `session_id`, `error`) with static messages; peer-supplied text (Origin) is escaped and length-bounded.
