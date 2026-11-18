---
tags:
  - sdd
  - delta
created: 2026-11-18
status: merged
related:
  - "[[078-ci-pipeline/spec|spec]]"
---

# Delta: js-cross-stack-suite

Merged into [[078-ci-pipeline/spec|spec]] on 2026-11-18 (#731).

## MODIFIED
- FR-007: `js-client-test` also runs `npm run build`, builds the `js_client_ws_fixture` example (`cargo build -p pjson-rs --example js_client_ws_fixture`, `working-directory: .`), and runs `npx jest --config jest.cross-stack.config.js` with `PJS_WS_FIXTURE_BIN` pointing at the built binary (#731).
- `js-client-test` `timeout-minutes` raised from 10 to 20; no new job, `ci-success` unchanged.
