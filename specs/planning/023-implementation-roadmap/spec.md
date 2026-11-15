---
aliases:
  - Implementation Roadmap
  - P1/P2 PR Groupings Retrospective
tags:
  - sdd
  - spec
  - roadmap
  - retrospective
created: 2026-04-28
status: implemented
priority: P1
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
  - "[[014-stream-poll-waker-contract/spec|Spec 014]]"
  - "[[015-batch-stream-ndjson-format/spec|Spec 015]]"
  - "[[016-async-stream-test-harness/spec|Spec 016]]"
  - "[[017-axum-error-http-status-mapping/spec|Spec 017]]"
  - "[[020-ts-client-wasm-api-sync/spec|Spec 020]]"
  - "[[021-bench-crate-workspace-isolation/spec|Spec 021]]"
  - "[[022-ci-js-client-enforcement/spec|Spec 022]]"
verified: 2026-11-15
---

# Spec 023: Implementation Roadmap - P1/P2 Triage and PR Groupings (Retrospective)

> [!info] Verification (2026-11-15)
> **State: implemented (roadmap fully executed; nothing remains).** Every issue in the 2026-04-28 matrix is CLOSED (verified with `gh issue view`), and all seven PR groupings landed on 2026-04-28 (merge times from `gh pr view`). The original forward-looking plan is converted here into a retrospective record.
>
> Drift from original plan:
> - PR-E and PR-F were merged as one PR (#192) instead of E then F, so the `continue-on-error` transitional measure was never needed.
> - PR-G (#207) delivered #166, #167 and #168 together.
> - README #186: the "remove unverifiable numbers" part landed (#201), but the current `README.md` states "1.82x faster than async_trait virtual calls" again with `cargo bench -p pjs-bench` as the reproduction. [NEEDS CLARIFICATION: whether that figure was re-measured after #115 and #179]
> - The spec's estimate of 4-5 days of effort is not verifiable from history.

## Purpose

Record how the open P1/P2 issues of CI cycle r9 (2026-04-28) were grouped into PRs and what landed. Kept as history and as a template for future roadmap specs.

## Issue Matrix and Outcome

| Issue | Title (short) | Spec | Outcome | PR |
|-------|---------------|------|---------|----|
| #178 | JS client 71/113 tests fail (P1) | 020 | closed 2026-04-28 | #192 |
| #166 | async-stream migration | 014 | closed 2026-04-28 | #207 |
| #167 | BatchFrameStream NDJSON | 015 | closed 2026-04-28 | #207 |
| #168 | tokio_test waker harness | 016 | closed 2026-04-28 | #207 |
| #173 | ApplicationError to HTTP status | 017 | closed 2026-04-28 | #187 |
| #179 | pjs-bench not compilable | 021 | closed 2026-04-28 | #190 |
| #180 | JS client not in CI | 022 | closed 2026-04-28 | #192 |
| #182 | SAFETY comments (AlignedBuffer) | none | closed 2026-04-28 | #191 |
| #183 | README feature table | none | closed 2026-04-28 | #194 |
| #184 | README Quick Start | none | closed 2026-04-28 | #194 |
| #186 | README performance claims | none | closed 2026-04-28 | #201 |

## PR Groupings: Planned vs Landed

| Group | Planned scope | Landed |
|-------|---------------|--------|
| PR-A README accuracy | #183, #184, #186 | #194 (#183, #184), #201 (#186) |
| PR-B SAFETY comments | #182 | #191 (`parser/buffer_pool.rs`, also removed dead code) |
| PR-C HTTP error mapping | #173 | #187 (also renamed `HttpExtensionConfig`) |
| PR-D bench workspace | #179 | #190 (bundled with #181 `process_frame` fix) |
| PR-E CI JS enforcement | #180 | #192 |
| PR-F JS client API fix | #178 | #192 |
| PR-G stream rewrite | #166, #168, #167 | #207 |

Remaining work from this roadmap: none.

## Backlog Items (P3/P4 listed as deferred) - Current State

All were CLOSED by 2026-04-28 or shortly after (verified with `gh issue view`): #112, #117, #131, #135, #136, #138, #140, #143, #144, #164, #174, #175, #176, #181, #185. Related PRs seen in history: #199 (tsify, #143), #205 (`PjsConfig::validate()`, #175), #206 (bounded session pagination, #136), #211 (#176), #212 (`ApiKeyAuthLayer`, #135), #220 (partial JSON parsing, #117), #222 (zstd dictionary, #144). #112 regressed and was re-fixed by #318 (see [[028-pjs-demo-workspace-build/spec|Spec 028]]).

## Follow-ups Spawned by the Roadmap

- #214, #215, #216 (found after #207/#212) were closed the same day: see [[024-compress-owned-bytes-return/spec|Spec 024]], [[025-streaming-response-content-type-helper/spec|Spec 025]], [[026-apikey-config-debug-redaction/spec|Spec 026]].
- #226 (gzip in streaming pipeline) closed via PR #229; the compression code was later removed with `AdaptiveFrameStream` in PR #521.

## Dependency Graph (as realized)

```
#179 bench fix ----> enables bench-based perf claims (#186)
#178 JS API fix ---> no continue-on-error needed for #180 (same PR)
#166 rewrite ------> #168 harness, #167 NDJSON (single PR #207)
```

## Lessons

- Bundling interdependent fixes (E+F, G) avoided transitional CI flags.
- Quick-win docs/comments PRs merged first, matching the planned Phase 1 order.
- Performance numbers removed for being unverifiable re-appeared in the README; keep claims tied to a reproducible bench.

## See Also

- [[019-jsondata-nan-handling/spec|Spec 019]], [[020-ts-client-wasm-api-sync/spec|Spec 020]], [[021-bench-crate-workspace-isolation/spec|Spec 021]], [[022-ci-js-client-enforcement/spec|Spec 022]]
