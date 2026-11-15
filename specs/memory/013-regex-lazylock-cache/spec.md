---
aliases:
  - Regex Cache
  - Validation Pattern Cache
tags:
  - sdd
  - spec
  - schema-validation
  - performance
created: 2026-04-28
status: implemented
priority: P3
issue: "#154"
pr: "#172"
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
code:
  - crates/pjs-core/src/domain/services/validation_service.rs
verified: 2026-11-15
---

# Feature: Compiled-Regex Cache in ValidationService

> [!info] Verification (2026-11-15)
> **State: implemented**, but with a different design than the one recommended. Superseded in design by a bounded, per-instance `DashMap`.
>
> **Evidence**
> - Issue #154 CLOSED (2026-04-28). First delivered by PR #172 (`8245dea`) as a process-wide static `DashMap` (CHANGELOG 0.5.2).
> - Reworked by PR #427 (`b65faa9`, "bound regex cache memory", CHANGELOG 0.7.0 Security): `ValidationService.regex_cache: DashMap<String, Arc<regex::Regex>>` (field, `schema-validation` feature), `MAX_REGEX_CACHE_ENTRIES = 64` with full flush on reaching the cap, `REGEX_SIZE_LIMIT` 1 MiB and `REGEX_DFA_SIZE_LIMIT` 64 KiB via `RegexBuilder`, `max_pattern_length` default 1024 (ceiling 8 KiB) giving `SchemaValidationError::PatternTooLong`.
> - Compile errors surface as `SchemaValidationError::InvalidPattern`; mismatches as `PatternMismatch`.
> - Test `test_regex_cache_stays_bounded_across_many_patterns` pins the bound.
>
> **Drift from original spec**
> - Option C (per-instance) was chosen as recommended, but with `DashMap<String, Arc<Regex>>` instead of `RwLock<HashMap<String, Regex>>`.
> - Cache is now bounded (spec said unbounded was acceptable).
> - Documented limitation: Unicode classes under bounded repetition (e.g. `^\p{L}{1,64}$`) exceed the 1 MiB size limit and are rejected.

## Overview

`ValidationService::validate_string` recompiled `regex::Regex` per call. Goal: compile each distinct pattern at most once per service while keeping memory bounded against attacker-supplied schemas. Out of scope: `RegexSet`, LRU eviction.

## Functional Requirements

### FR-001: Compile each pattern at most once per service (until flush)
Status: done. Code: `validate_string` in `validation_service.rs`.

### FR-002: Invalid pattern error preserved
Status: done (`SchemaValidationError::InvalidPattern { path, pattern, reason }`).

### FR-003: Thread-safe
Status: done (`DashMap` shards; `Arc` cloned before `is_match` so no shard lock is held while matching).

### FR-004: No public API change
Status: done for `validate_string`; new builder `with_max_pattern_length` added (#427).

## Non-Functional Requirements

- NFR-001: 10 patterns x 1000 validations compile <= 10 times. Status: holds below the 64-entry cap.
- NFR-002 (replaces "unbounded"): cache <= 64 entries; single-thread view ~ 64 MiB worst case (documented in the field doc comments).

## Edge Cases

| Case | Behaviour |
|------|-----------|
| Pattern longer than `max_pattern_length` | `PatternTooLong`, never compiled or cached |
| Invalid regex | `InvalidPattern` |
| Cache reaches 64 entries | flushed with `clear()`; concurrent inserters may transiently overshoot |

## Success Criteria

- Unit tests in `validation_service.rs` pass, including bounded-cache and invalid-pattern tests.

## Agent Boundaries

- Always: keep the size limits when touching `RegexBuilder` configuration.
- Ask first: raising `REGEX_SIZE_LIMIT` (memory bound trade-off).
- Never: reintroduce a process-lifetime static cache.

## Open Questions

- Flush-all eviction is coarse; an LRU would avoid recompiling hot patterns after a flush. No issue filed. [NEEDS CLARIFICATION]

## See Also

- `crates/pjs-core/src/domain/services/validation_service.rs`
