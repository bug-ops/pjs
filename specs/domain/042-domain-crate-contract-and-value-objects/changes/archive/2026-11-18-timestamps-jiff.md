---
tags:
  - sdd
  - delta
created: 2026-11-18
status: merged
related:
  - "[[042-domain-crate-contract-and-value-objects/spec|spec]]"
---

# Delta: timestamps-jiff

Merged into [[042-domain-crate-contract-and-value-objects/spec|spec]] on 2026-11-18 (#692).

## MODIFIED
- FR-042-002 the crate shall depend on `jiff` 1.0 (`std`, `serde`; default features off) instead of `chrono`. On `all(target_arch = "wasm32", target_os = "unknown")` it also enables jiff's `js` feature, without which `Timestamp::now()` panics in the browser. The rest of the dependency list is unchanged.
- Ask-first note on `chrono` features becomes `jiff` features.

## ADDED
- Wire form of timestamps (frames, events, `DomainEvent::metadata`): RFC 3339 UTC with a `Z` suffix and the shortest exact fraction (`.1`, `.1234`, `.123456789`); fraction digits are no longer grouped by three. Inputs with an offset (`+02:00`, `+00:00`) decode and re-encode as `Z`.
- Public timestamp type is `jiff::Timestamp`; durations are `jiff::SignedDuration` (`Stream::duration`). Breaking for `pjson-rs-domain` consumers.

## Edge Cases
| Input | Re-encoded |
|---|---|
| `2024-01-02T03:04:05.100Z` | `2024-01-02T03:04:05.1Z` |
| `2024-01-02T05:04:05+02:00` | `2024-01-02T03:04:05Z` |

Pinned by `crates/pjs-domain/tests/timestamp_wire_acceptance.rs`.
