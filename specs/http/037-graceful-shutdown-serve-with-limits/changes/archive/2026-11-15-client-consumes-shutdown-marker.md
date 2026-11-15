---
tags:
  - sdd
  - delta
created: 2026-11-15
status: merged
related:
  - "[[037-graceful-shutdown-serve-with-limits/spec|spec]]"
---

# Delta: js-client-server-shutdown

Merged into [[037-graceful-shutdown-serve-with-limits/spec|spec]] on 2026-11-15 (#644).

## MODIFIED
- FR-008 notes that `pjs-js-client` consumes the terminal marker as a typed `server_shutdown` event with `SERVER_SHUTDOWN` rejection of pending streams (#644, 073 FR-013). Server behaviour unchanged.

