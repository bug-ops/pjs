---
aliases:
  - pjson-rs-domain Crate Contract and Value Objects
  - Domain Value Objects
tags:
  - sdd
  - spec
  - domain
  - value-objects
  - wasm
  - type-safety
created: 2026-11-15
status: implemented
priority: P2
code:
  - crates/pjs-domain/Cargo.toml
  - crates/pjs-domain/src/lib.rs
  - crates/pjs-domain/src/value_objects/mod.rs
  - crates/pjs-domain/src/value_objects/priority.rs
  - crates/pjs-domain/src/value_objects/id.rs
  - crates/pjs-domain/src/value_objects/json_data.rs
  - crates/pjs-domain/src/value_objects/json_path.rs
  - crates/pjs-domain/src/value_objects/backpressure.rs
  - crates/pjs-domain/tests/property_tests.rs
  - crates/pjs-domain/tests/id_zero_cost_test.rs
  - crates/pjs-domain/tests/json_data_comprehensive.rs
related:
  - "[[019-jsondata-nan-handling/spec]]"
  - "[[043-domain-schema-depth-guard-wire-encoding/spec]]"
  - "[[044-domain-entities-events-priority-heuristic/spec]]"
  - "[[070-wasm-bindings-api/spec]]"
verified: 2026-11-15
---

# Spec 042: Domain Crate Contract and Core Value Objects

Part of [[MOC-specs]]. Rules in [[constitution]]. Plan: [[042-domain-crate-contract-and-value-objects/plan|plan]].

> [!info] Relation to [[019-jsondata-nan-handling/spec|019]]
> NaN/Infinity handling in `JsonData` is specified there; FR-042-009 here only states the current behaviour so the two can be reconciled by the migrator of 019.

## Overview

**Problem.** The workspace needs one dependency-light crate that holds the protocol's domain vocabulary and can run in both native Rust and `wasm32-unknown-unknown`.

**Goal.** `pjson-rs-domain` (`crates/pjs-domain`) defines: the crate-level contract (unsafe forbidden, documented public items, error type, features, WASM readiness) and the value objects `Priority`, `Id<T>` (`SessionId`, `StreamId`), `JsonData`, `JsonPath`/`PathSegment`, `BackpressureSignal`. Schema, depth guard and wire encodings are in [[043-domain-schema-depth-guard-wire-encoding/spec|043]]; entities, events and the priority heuristic are in [[044-domain-entities-events-priority-heuristic/spec|044]].

**Out of scope.** Application services, ports and infrastructure (`pjs-core`); WASM bindings ([[070-wasm-bindings-api/spec|070]]).

## User Stories

- As a library author, I construct a `Priority` that cannot be zero and an id that cannot be mixed up with another kind of id.
- As a WASM consumer, I depend on the domain crate without pulling in `tokio` or `axum`.
- As a protocol implementer, I round-trip a `JsonPath` through its `$.a[0]` text form.

## Functional Requirements

### FR-042-001 Unsafe forbidden

`pjson-rs-domain` shall forbid `unsafe` code (`[lints.rust] unsafe_code = "forbid"` in `Cargo.toml`) and warn on undocumented public items (`#![warn(missing_docs)]`).
Status: done. (`pjs-core` is different: it denies only `unsafe_op_in_unsafe_fn` and contains `unsafe`, see [[040-buffer-pool-aligned-alloc/spec|040]].)

### FR-042-002 Dependencies and features

The crate shall depend only on `chrono` (serde), `serde` (derive), `serde_json`, `smallvec` (serde), `thiserror`, `uuid` (`v4`, `serde`; plus `js` on `target_arch = "wasm32"`). The crate declares no features: the former `std` and `wasm` features were removed (#606).
Status: done.

### FR-042-003 std only

The crate shall build with `std` only: the unsupported `no_std` mode (the `cfg_attr(not(feature = "std"), no_std)` attribute and the `alloc` import) and the `std` and `wasm` features were removed (#606). The crate is still WASM-compatible (`wasm32-unknown-unknown`, `uuid/js`).
Status: done.

### FR-042-004 `DomainError`

The crate shall expose `DomainError` (`#[non_exhaustive]`, `thiserror`) and `DomainResult<T>`. Variants (all carry a `String`): `InvalidStateTransition`, `InvalidStreamState`, `InvalidSessionState`, `InvalidFrame`, `InvariantViolation`, `InvalidPriority`, `InvalidPath`, `SessionNotFound`, `StreamNotFound`, `TooManyStreams`, `Logic`, `Io`, `NotFound`, `ConcurrencyConflict`, `CompressionError`, `ValidationError`, `InvalidInput`, `InternalError`, `SecurityViolation`, `ResourceExhausted`. Helpers: `invariant_violation(msg)`, `invalid_transition(from, to)`, `From<String> -> Logic`.
Status: done.

### FR-042-005 `Priority`

`Priority` shall wrap `NonZeroU8`; `Priority::new(u8)` and `TryFrom<u8>` shall return `DomainError::InvalidPriority` for 0. Constants: `CRITICAL` = 100, `HIGH` = 80, `MEDIUM` = 50, `LOW` = 25, `BACKGROUND` = 10. `increase_by`/`decrease_by` shall saturate at 255 and 1. `is_critical()` shall be `value >= 100`; `is_high_or_above()` shall be `value >= 80`. `from_percentage(f32)` shall reject values outside 0..=100 and map `percent * 2.55` rounded, minimum 1; `to_percentage()` returns `value / 255 * 100`. `Display` prints `Critical(100)`, `High(80)`, `Medium(50)`, `Low(25)`, `Background(10)` for the constants and `Priority(n)` otherwise. Ordering is numeric (`Ord`). `From<Priority> for u8`.
`Priority` shall not implement `Serialize`/`Deserialize`; embedding types use `serde_support::priority` (bare `u8`, zero rejected) ([[043-domain-schema-depth-guard-wire-encoding/spec|043]]).
Status: done. Tests: unit tests and `tests/property_tests.rs`.

### FR-042-006 `Id<T>`

`Id<T: IdMarker>` shall wrap a `Uuid` with a `PhantomData<T>` marker; `SessionId = Id<SessionMarker>` and `StreamId = Id<StreamMarker>`. `IdMarker` is sealed (private `Sealed` supertrait), so only the two markers exist. API: `new()` (UUID v4), `from_uuid`, `from_string(&str) -> Result<_, uuid::Error>`, `as_uuid`, `as_str() -> String`, `Default`, `Display` (UUID), `Debug` (type name plus UUID), `From<Uuid>`, `From<Id<T>> for Uuid`, `Copy`, `Eq`, `Hash`. A `SessionId` shall not be assignable to a `StreamId` (compile-fail doc test). `size_of::<Id<T>>() == size_of::<Uuid>()`.
`Id<T>` shall not implement `Serialize`/`Deserialize`; embedding types use `serde_support::{session_id, stream_id}`.
Status: done. Tests: `tests/id_zero_cost_test.rs`, `tests/property_tests.rs`.

### FR-042-007 `JsonData`

`JsonData` shall be a `#[non_exhaustive]` enum: `Null` (default), `Bool(bool)`, `Integer(i64)`, `Float(f64)`, `String(String)`, `Array(Vec<JsonData>)`, `Object(HashMap<String, JsonData>)`.
Constructors: `null`, `bool`, `integer`, `string`, `array`, `object`, and `float(f64) -> DomainResult<Self>` which shall return `InvalidInput` for NaN and infinities. Predicates `is_*`, accessors `as_bool`, `as_i64`, `as_f64` (integers widen), `as_str`, `as_array[_mut]`, `as_object[_mut]`, `get(key)`. `From` impls: `bool`, `i64`, `String`, `&str`, `Vec<JsonData>`, `HashMap<String, JsonData>`, `serde_json::Value`.
Status: done.

### FR-042-008 `JsonData` dot-path helpers

`path("a.b.c")` shall return the nested value through objects only; `set_path("a.b", v)` shall create missing intermediate objects, return `false` if an intermediate or the target parent is not an object. These use plain dot notation (no array indices) and are unrelated to `JsonPath`.
Status: done.

### FR-042-009 `JsonData` serde

`Serialize` shall emit the natural JSON shape (`Null` as unit/null, `Integer` as i64, `Float` via `serialize_f64`). `Deserialize` shall use a hand-written visitor that:
- rejects NaN/infinite floats (`visit_f64`);
- maps `u64` above `i64::MAX` to `Float`;
- limits nesting to `MAX_DESERIALIZE_DEPTH` = 64 (error `"JSON nesting depth exceeds maximum of 64"`);
- caps pre-allocation from `size_hint` at 1 MiB worth of elements (`cautious_capacity`).
Status: done. Note the construction paths are asymmetric: the variant `JsonData::Float(f64::NAN)` can be built directly (variant is public).

### FR-042-010 `JsonData` Eq/Hash/Display/size

`JsonData` shall implement `Eq` and `Hash` (floats hash by bit pattern; objects hash with keys sorted) and `Display` (compact JSON-like; strings and keys are written between quotes without escaping). `memory_size()` shall return a rough estimate (strings counted at 2 bytes per byte).
Status: done as built; see Open Questions for `Eq` with NaN and `Display` escaping.

### FR-042-011 `JsonPath`

`JsonPath` shall hold `Vec<PathSegment>` with `PathSegment::{Key(String), Index(usize)}` (`#[non_exhaustive]`); the root path is the empty vector. Keys shall be non-empty and shall not contain `.`, `[` or `]`.
Construction: `root()`, `new("$...")`/`FromStr` (must start with `$`; segments `.key` and `[n]`; errors are `InvalidPath`), `from_segments(iter)` (validates keys), `append_key(&str) -> DomainResult`, `append_index(usize)`.
Queries: `segments`, `depth`, `parent`, `last_segment`, `last_key` (skips trailing indices), `is_prefix_of` (strict prefix), `to_json_pointer()` (root is `"/"`; keys are not escaped).
`Display` is `$` plus `.key` / `[n]`; serde uses the same string, and the textual form is the wire format. `Display` then `FromStr` round-trips (proptest).
Status: done.

### FR-042-012 `BackpressureSignal`

`BackpressureSignal` shall be a `#[non_exhaustive]` `Copy` enum `Ok` (default), `SlowDown`, `Pause`, serde-derived (variant names), with `should_pause()` (Pause only), `should_throttle()` (SlowDown or Pause), `suggested_delay_ms()` (0, 100, `u64::MAX`) and `Display` (`OK`, `SLOW_DOWN`, `PAUSE`).
Status: done. Consumed by `DomainEvent::BackpressureReceived` ([[044-domain-entities-events-priority-heuristic/spec|044]]).

## Non-Functional Requirements

- NFR-TYPE-042-001: invalid primitive states are unrepresentable for priority (non-zero) and id kinds (phantom marker); path keys are validated at construction.
- NFR-PERF-042-001: `Id<T>` is layout-identical to `Uuid` (test `id_zero_cost_test.rs`); `Priority` is one byte (`NonZeroU8`, so `Option<Priority>` is one byte).
- NFR-SEC-042-001: `JsonData` deserialisation is bounded in depth (64) and in pre-allocation.
- NFR-COMPAT-042-001: `cargo build -p pjs-wasm --target wasm32-unknown-unknown` is exercised by CI wasm-build jobs ([[078-ci-pipeline/spec|078]]); the domain crate's own tests run natively in the main matrix.

## Data Model

See FR text. Type inventory: `Priority`, `Id<T>`, `IdMarker`, `SessionMarker`, `StreamMarker`, `SessionId`, `StreamId`, `JsonData`, `JsonPath`, `PathSegment`, `BackpressureSignal`, `DomainError`, `DomainResult`.

## Edge Cases and Errors

| Input | Result |
|---|---|
| `Priority::new(0)` | `InvalidPriority("Priority cannot be zero")` |
| `Priority::from_percentage(101.0)` / NaN | `InvalidPriority` |
| `Priority::from_percentage(0.0)` | `Priority(1)` |
| `Id::from_string("invalid")` | `uuid::Error` |
| `JsonPath::new("")`, `"key"`, `"$."`, `"$.a."`, `"$.a[]"`, `"$.a[x]"` | `InvalidPath` |
| `JsonPath::new("$.a b")` | ok (spaces allowed) |
| `append_key("a.b")` | `InvalidPath` |
| `JsonData::float(NAN)` | `InvalidInput` |
| JSON nested deeper than 64 | deserialisation error |
| `set_path` through a non-object | returns `false` |

## Success Criteria

- `cargo nextest run -p pjson-rs-domain` and `cargo test -p pjson-rs-domain --doc` pass.
- `RUSTDOCFLAGS="--deny rustdoc::broken_intra_doc_links --deny rustdoc::private_intra_doc_links --deny rustdoc::redundant_explicit_links" cargo doc --no-deps -p pjson-rs-domain` passes (CI runs doc with `--all-features`).
- No `unsafe` compiles in the crate.

## Agent Boundaries

- Always: add a validating constructor (`new() -> Result`) for any new value object with an invariant; keep `unsafe` out.
- Ask first: adding a `no_std` mode or a Cargo feature (needs replacing `HashMap`, `chrono` features, `serde_json` use); changing `Priority` constant values (they are wire-visible through frames and heuristics).
- Never: add an infallible `From<u8> for Priority` or public fields to value objects; add dependencies on `tokio`, `axum` or `jiter` (see `parser/partial.rs` note).

## Open Questions

- `Priority` scale: constants peak at 100 but the type allows up to 255, so values 101..=255 are "critical" by `is_critical()` and `to_percentage()` of `CRITICAL` is about 39.2. `from_percentage(100.0)` yields 255. The percent API therefore does not match the constants' scale.
- `JsonData: Eq` is implemented manually while `PartialEq` is derived over `f64`; a directly constructed `Float(NAN)` is not equal to itself, breaking `Eq`. The public variant bypasses `float()`.
- `JsonData::Display` does not escape strings or keys, so its output is not valid JSON for values containing quotes, backslashes or control characters; `Frame::estimated_size` relies on its length only.
- `From<serde_json::Value>` maps a number that is neither `i64` nor `f64` to `Float(0.0)` silently (reachable only with serde_json's `arbitrary_precision`).
- `JsonData::set_path` uses `expect` on a key it just inserted (cannot fail, but is a panic site).
- `Id::as_str()` returns an owned `String` (name suggests a borrow).
- `JsonPath::to_json_pointer` does not escape `~` and `/` in keys (RFC 6901).
- `JsonData::Object` is a `HashMap`, so iteration and serialisation order of keys is unspecified; byte-stable encodings of multi-key objects are not guaranteed.

## See Also

- [[042-domain-crate-contract-and-value-objects/plan|Plan (as built)]]
- [[043-domain-schema-depth-guard-wire-encoding/spec|043 Schema, depth guard and wire encoding]]
- [[044-domain-entities-events-priority-heuristic/spec|044 Entities, events, priority heuristic]]
- [[019-jsondata-nan-handling/spec|019 NaN handling]]
- `docs/architecture/SPECIFICATION.md`, `docs/architecture/WASM_ROADMAP.md`
