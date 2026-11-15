---
aliases:
  - StringArena Safe Interning
  - Spec 005
tags:
  - sdd
  - spec
  - memory
  - unsafe
  - obsolete
created: 2026-04-28
status: obsolete
priority: P1
issue: "#124"
pr: "#128"
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
code: []
verified: 2026-11-15
---

# Spec 005: StringArena Safe Interning

> [!info] Verification (2026-11-15)
> **State: obsolete (fixed, then the whole module was removed).**
>
> Evidence: issue #124 closed 2026-04-28 by PR #128 (`1bfc905`): CHANGELOG "`StringArena::intern()` now stores raw pointers instead of `&'static str` transmutes, eliminating potential use-after-free UB (#124)". The entire `memory` module (`memory::arena`: `StringArena`, `ValueArena<T>`, `JsonArena`, `ArenaStats`, `CombinedArenaStats`) and the `typed-arena` dependency were later removed as dead public API in #430/#444 (`ae919f8`). `crates/pjs-core/src/memory/` does not exist; `rg 'StringArena|transmute' crates` is empty. The actual hot-path allocator is `parser::buffer_pool::{AlignedBuffer, BufferPool}`.
>
> Drift from original spec:
> - The chosen fix was a raw-pointer variant of Option A, not a lifetime-parameterised struct.
> - Nothing remains to specify. `.claude/CLAUDE.md` still lists `memory/` (arena allocator, buffer pool) in the architecture diagram; that line is stale (buffer pool lives in `parser/buffer_pool.rs`).

**Type**: bug (P1 memory safety) | **Status**: obsolete

## Original problem (historical)

`StringArena::intern` used `std::mem::transmute` to extend `&str` to `&'static str`; after the arena dropped, callers held dangling references (use-after-free, public API).

## Functional requirements (final disposition)

| FR | Requirement | Status |
|----|-------------|--------|
| 1 | Returned lifetime tied to `&self` | superseded: type removed |
| 2 | Reference invalid (compile error) after drop | superseded |
| 3 | Dedup returns same pointer | superseded |
| 4 | No `transmute` to `'static` | done by removal (no `transmute` in `crates`) |

## Lesson kept

Public APIs that fabricate `'static` lifetimes must not ship; prefer lifetime-parameterised handles or typed indices. See [[constitution]] on strict typing and `unsafe` justification.
