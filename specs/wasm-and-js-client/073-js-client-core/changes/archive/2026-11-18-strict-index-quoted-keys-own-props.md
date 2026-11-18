---
tags:
  - sdd
  - delta
created: 2026-11-18
status: merged
related:
  - "[[073-js-client-core/spec|spec]]"
---

# Delta: strict-index-quoted-keys-own-props

Merged into [[073-js-client-core/spec|spec]] on 2026-11-18 (#713, #698, #712).

## MODIFIED
- FR-004 (frame validation) and FR-009 path grammar: `$` then any of `.plain`, `[index]`, `["quoted"]`. `plain` is non-empty without `.`, `[`, `]`. `index` is `0` or `[1-9][0-9]*` (ASCII, no sign, no leading zeros) up to `Number.MAX_SAFE_INTEGER`; Rust's bound is `usize` (documented divergence; no real array reaches either). `quoted` is the canonical `JSON.stringify` literal of a key that is not plain and has no lone surrogates; non-canonical escapes, plain keys in quotes and a missing `]` are rejected. The FrameProcessor path regex is replaced by the shared scanner (`isValidJsonPath`).
- FR-010 Prototype-pollution defence: keys `__proto__`, `constructor`, `prototype` are ordinary data. The deny-list (`UNSAFE_KEYS`, `assertSafeKey`) is removed; every read on the walk and at the tail (set/merge/append/delete) goes through an own-property check, writes use `Object.defineProperty`, intermediates are `Object.create(null)`; no patch can reach a prototype.
- FR-005 `PatchApplied` event payload gains `appliedPatches` and `skippedPatches`.
- Edge Cases: "Unsafe key in path" row removed (the key is stored as data); the bad-index row includes leading zeros.

## REMOVED
- Open Questions bullet "No test asserts the prototype-pollution defence": covered by `tests/core/reconstructor-own-keys-acceptance.test.ts`.
