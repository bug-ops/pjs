---
tags:
  - sdd
  - delta
created: 2026-11-18
status: merged
related:
  - "[[078-ci-pipeline/spec|spec]]"
---

# Delta: dependabot-npm

Merged into [[078-ci-pipeline/spec|spec]] on 2026-11-18 (#693).

## MODIFIED
- `dependabot.yml` also schedules weekly (Monday 09:00 GMT) updates for npm (`/crates/pjs-js-client`), minor and patch grouped, commit prefix `chore` with scope, labels `dependencies` + `pjs-js-client`. The auto-merge policy is unchanged (patch, or minor from >= 1.0.0, after all checks pass).
