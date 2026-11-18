---
tags:
  - sdd
  - delta
created: 2026-11-18
status: merged
related:
  - "[[078-ci-pipeline/spec|spec]]"
---

# Delta: drop-nested-js-workflow

Merged into [[078-ci-pipeline/spec|spec]] on 2026-11-18 (#732).

## REMOVED
- `crates/pjs-js-client/.github/workflows/ci.yml` (never executed by GitHub, referenced a missing `test:ci` script, used unpinned actions); the directory held nothing else (#732).
- Open Question "nested workflow GitHub never runs".
