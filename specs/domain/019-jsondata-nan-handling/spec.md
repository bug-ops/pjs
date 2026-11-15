---
aliases:
  - JsonData NaN Handling
  - JsonData non-finite floats
tags:
  - sdd
  - spec
  - domain
  - type-safety
created: 2026-04-28
status: implemented
priority: P3
issue: "#176"
pr: "#211"
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
code:
  - crates/pjs-domain/src/value_objects/json_data.rs
  - crates/pjs-domain/tests/json_data_comprehensive.rs
verified: 2026-11-15
---

# Spec 019: JsonData NaN/Infinity Handling

> [!info] Verification (2026-11-15)
> **State: implemented** (Strategy A, constructor guard, plus a defensive serialize-time mapping). Issue #176 CLOSED 2026-04-28; delivered by PR #211 (`346ee77`).
>
> Evidence: `JsonData::float(f64) -> DomainResult<Self>` rejects NaN/infinite with `DomainError::InvalidInput`; `impl From<f64> for JsonData` is removed (CHANGELOG, "BREAKING `JsonData::float(f64)` now returns `DomainResult<Self>`"); the serde `Deserialize` visitor (`visit_f64`) rejects non-finite values; tests `test_json_data_float_nan`, `test_json_data_float_rejects_*` in `crates/pjs-domain/tests/json_data_comprehensive.rs`.
>
> Drift from original spec:
> - The spec offered A or B; A was chosen. The enum variant `JsonData::Float(f64)` is still `pub`, so `JsonData::Float(f64::NAN)` remains constructible directly (the type does not make the state fully unrepresentable).
> - `Serialize` for `JsonData::Float` calls `serialize_f64`, so a directly constructed non-finite value serializes via `serde_json` to `null` (unit test `test_non_finite_float_serializes_as_json_null`). This is a safety net, not the contract.
> - `Display` for `JsonData::Float` still writes `{float_val}` (prints `NaN`/`inf` for a directly constructed non-finite value). FR-4 below is open.
> - The second conversion path that mapped NaN to `0` (`JsonAdapter::to_serde_value`) was removed in #460; non-finite policy is now single-sourced in `JsonData`'s `Serialize`.

## Problem

RFC 8259 section 6 forbids NaN and infinity in JSON numbers. `JsonData::Float(f64)` must never carry them, otherwise serialized streams can be invalid or silently corrupted.

## User Stories

- As a library user, I can trust that a `JsonData` built through the public API serializes to valid JSON.
- As a library user, constructing a float from NaN/infinity gives a clear error.

## Functional Requirements

### FR-1 Non-finite floats are rejected at construction
Status: done.
`JsonData::float(f)` returns `Err(DomainError::InvalidInput)` when `f.is_nan() || f.is_infinite()`. `From<f64>` no longer exists.

### FR-2 Deserialization rejects non-finite floats
Status: done (not in original spec). `JsonData`'s `Visitor::visit_f64` returns a custom error for NaN/infinite.

### FR-3 Round-trip test coverage
Status: done. Tests cover finite, zero, negative, and non-finite inputs (`json_data_comprehensive.rs`, and unit tests in `json_data.rs`).

### FR-4 `Display` emits valid JSON for floats
Status: open. `Display` writes the raw `f64`; reachable only by bypassing `JsonData::float`. [NEEDS CLARIFICATION: is fixing `Display` still wanted, or is making the `Float` variant non-constructible (private payload) the preferred fix?]

## Acceptance Criteria

- [x] `JsonData::float(f64::NAN).is_err()`, same for both infinities.
- [x] Finite values (zero, negative, large, small) round-trip.
- [x] Serialization of a directly constructed non-finite `Float` yields `null`, not an error.
- [ ] `Display` of a non-finite `Float` yields valid JSON.

## See Also

- [RFC 8259 section 6](https://datatracker.ietf.org/doc/html/rfc8259#section-6)
- [[023-implementation-roadmap/spec|Spec 023 - roadmap]]
- Source: `crates/pjs-domain/src/value_objects/json_data.rs`
