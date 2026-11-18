---
tags:
  - sdd
  - delta
created: 2026-11-18
status: merged
related:
  - "[[076-fuzz-targets/spec|spec]]"
---

# Delta: ws-decode-oracle

Merged into [[076-fuzz-targets/spec|spec]] on 2026-11-18 (#690).

## MODIFIED
- FR-006 `ws_message_decode` shall assert exact equality between the value of the re-decoded message and `pjs_fuzz::reparsed(&value)` (one serialize/parse cycle from the first decode), instead of the 1-ULP `approx_eq`. Both sides then carry the same production float re-parse drift, so a benign large integer no longer fails the oracle; this is the oracle `planner_round_trip` already uses (#684).
- Seeds for `ws_message_decode` gain `huge_integer` (a `StreamInit` whose `data` is 199 `1` digits): red on the old oracle ("round trip drifted"), green on the new one.
- FR-001a `approx_eq` is no longer used by any target.
