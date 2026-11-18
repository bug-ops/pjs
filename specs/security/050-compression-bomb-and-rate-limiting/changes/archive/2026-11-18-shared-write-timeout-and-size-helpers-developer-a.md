---
tags:
  - sdd
  - delta
created: 2026-11-18
status: merged
related:
  - "[[050-compression-bomb-and-rate-limiting/spec|spec]]"
---

# Delta: shared-write-timeout-and-size-helpers

Merged into [[050-compression-bomb-and-rate-limiting/spec|spec]] on 2026-11-18 (#703, #704).

## MODIFIED
- FR-007/FR-010 `DEFAULT_WRITE_TIMEOUT` is defined once in `security::rate_limit` (compiled without the websocket features) and reused by the websocket protocol; behavior is unchanged.
- FR-008 the HTTP rate-limit config is the one defined here (see spec 061 delta).
