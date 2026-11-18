---
tags:
  - sdd
  - delta
created: 2026-11-18
status: merged
related:
  - "[[078-ci-pipeline/spec|spec]]"
---

# Delta: ci-cooldown

Merged into [[078-ci-pipeline/spec|spec]] on 2026-11-18 (#723).

## MODIFIED
- `js-client-test` installs dependencies with `npm ci --ignore-scripts` (both `npm ci` uses in `ci.yml`).
- `dependabot.yml` adds a 7-day `cooldown` (`default-days: 7`) to cargo (`/`), cargo (`/fuzz`) and github-actions; npm already had it. Security updates are not delayed.
- Open Question "Dependabot has no npm ecosystem entry" removed (stale since #693).
