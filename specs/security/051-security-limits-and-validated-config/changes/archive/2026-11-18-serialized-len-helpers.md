---
tags:
  - sdd
  - delta
created: 2026-11-18
status: merged
related:
  - "[[051-security-limits-and-validated-config/spec|spec]]"
---

# Delta: serialized-len-helpers

Merged into [[051-security-limits-and-validated-config/spec|spec]] on 2026-11-18 (#703).

## MODIFIED
- Serialized-size estimation uses one `json_str_len` escape table (and `decimal_digits`) shared by the websocket planner, the client and the compression savings estimate; it must equal `serde_json` output length for every string (proptest). No limit value changes.
