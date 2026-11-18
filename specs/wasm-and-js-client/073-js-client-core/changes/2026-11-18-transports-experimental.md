---
tags:
  - sdd
  - delta
created: 2026-11-18
status: merged
related:
  - "[[073-js-client-core/spec|spec]]"
---

# Delta: transports-experimental

Merged into [[073-js-client-core/spec|spec]] on 2026-11-18 (#714).

## MODIFIED
- FR-011 status: experimental, external server only; wire format documented in the README and marked `@experimental`; shipped near-misses named (`POST /pjs/sessions` 422 on `client_info`, `PjsExtension` `GET /pjs/stream/{id}/sse` frame shape, `/pjs/ws` 404 vs `/ws`); the claim that no shipped server speaks the format is dropped.
- Open Questions: endpoint compatibility resolved with the true routes.
