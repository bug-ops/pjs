---
tags:
  - sdd
  - delta
created: 2026-11-15
status: merged
related:
  - "[[073-js-client-core/spec|spec]]"
---

# Delta: js-client-server-shutdown

Merged into [[073-js-client-core/spec|spec]] on 2026-11-15 (#644).

## ADDED
- FR-013 server shutdown notification: SSE `shutdown` event and NDJSON `{"event":"shutdown"}` marker, `isShutdownMarker`, transport `shutdown` event, `PJSEvent.ServerShutdown`, `PJSErrorType.ServerShutdown` for a pending `stream()`, no auto-reconnect.

## MODIFIED
- FR-011 SSE and HTTP transports and the `Transport` base class emit `shutdown`; `PJSErrorType` now has 10 members; Edge Cases gain the shutdown row.

