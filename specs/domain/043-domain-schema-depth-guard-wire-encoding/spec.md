---
aliases:
  - Domain Schema, Depth Guard and Wire Encoding
  - Schema Value Object
  - serde_support
tags:
  - sdd
  - spec
  - domain
  - schema
  - serde
  - security
created: 2026-11-15
status: implemented
priority: P2
code:
  - crates/pjs-domain/src/value_objects/schema.rs
  - crates/pjs-domain/src/value_objects/depth_guard.rs
  - crates/pjs-domain/src/serde_support.rs
  - crates/pjs-domain/src/entities/stream_map.rs
  - crates/pjs-domain/tests/schema_comprehensive.rs
  - crates/pjs-domain/tests/serde_golden.rs
  - crates/pjs-core/src/application/dto/schema_dto.rs
  - crates/pjs-core/src/domain/services/validation_service.rs
related:
  - "[[042-domain-crate-contract-and-value-objects/spec]]"
  - "[[044-domain-entities-events-priority-heuristic/spec]]"
  - "[[041-simd-features-and-global-allocator/spec]]"
  - "[[051-security-limits-and-validated-config/spec]]"
verified: 2026-11-15
---

# Spec 043: Domain Schema, Deserialisation Depth Guard and Wire Encoding

Part of [[MOC-specs]]. Rules in [[constitution]]. Plan: [[043-domain-schema-depth-guard-wire-encoding/plan|plan]].

## Overview

**Problem.** The protocol carries schema definitions and typed ids/priorities over untrusted channels. Recursive types can exhaust the stack on hostile input, and value objects without serde impls still need exactly one agreed wire encoding.

**Goal.** Specify (1) the `Schema` value object and its validation-error vocabulary, (2) the reusable per-thread deserialisation depth guard, (3) the `serde_support` encodings and golden wire format, and (4) the `StreamMap` wire invariant.

**Out of scope.** Executing validation (that is `ValidationService` in `pjs-core`, see [[041-simd-features-and-global-allocator/spec|041]] FR-041-009); entity/event semantics ([[044-domain-entities-events-priority-heuristic/spec|044]]); HTTP DTO conversion (`SchemaDefinitionDto` in `pjs-core`).

## User Stories

- As an API author, I describe a payload shape with `Schema::Object { .. }` and get structured `SchemaValidationError`s with a path.
- As an operator, I want a 10,000-level nested schema document rejected instead of overflowing the stack.
- As a client developer, I rely on `Priority` being a bare number and ids being UUID strings on the wire.

## Functional Requirements

### FR-043-001 Schema variants

`Schema` shall be a `#[non_exhaustive]`, serde-derived enum (externally tagged) with variants:
- `String { min_length, max_length: Option<usize>, pattern: Option<String>, allowed_values: Option<SmallVec<[String; 8]>> }`
- `Integer { minimum, maximum: Option<i64> }`, `Number { minimum, maximum: Option<f64> }`
- `Boolean`, `Null`, `Any`
- `Array { items: Option<Box<Schema>>, min_items, max_items: Option<usize>, unique_items: bool }`
- `Object { properties: HashMap<String, Schema>, required: Vec<String>, additional_properties: bool }`
- `OneOf { schemas: SmallVec<[Box<Schema>; 4]> }`, `AllOf { schemas: SmallVec<[Box<Schema>; 4]> }`
Status: done.

### FR-043-002 Schema helpers

`Schema` shall provide constructors `string(min, max)`, `integer`, `number`, `array(Option<Schema>)`, `object(properties, required)` (sets `additional_properties: true`); `allows_type(SchemaType) -> bool` (`Any` allows all, `OneOf` any member, `AllOf` all members); and `validation_cost() -> usize` (heuristic: scalars 1/5, string 10 or 50 with pattern, array 10 + item cost, object 20 + sum, OneOf 30 + max * count, AllOf 20 + sum).
Status: done.

### FR-043-003 `SchemaType`

`SchemaType` shall be a `#[non_exhaustive]` `Copy` enum (`String`, `Integer`, `Number`, `Boolean`, `Null`, `Array`, `Object`); `From<&Schema>` maps `Any`, `OneOf` and `AllOf` to `Object`.
Status: done as built (see Open Questions).

### FR-043-004 `SchemaId`

`SchemaId` shall be a string newtype (`new(impl Into<String>)`, `as_str`, `Display`, serde) with no validation of content.
Status: done.

### FR-043-005 Validation errors

`SchemaValidationError` shall be a `#[non_exhaustive]`, serde-derived `thiserror` enum whose variants all carry the offending `path: String`: `TypeMismatch`, `MissingRequired`, `OutOfRange`, `StringLengthConstraint`, `PatternMismatch`, `InvalidPattern`, `PatternTooLong`, `ArraySizeConstraint`, `DuplicateItems`, `InvalidEnumValue`, `AdditionalPropertyNotAllowed`, `NoMatchingOneOf`, `AllOfFailure`. `SchemaValidationResult<T>` aliases `Result<T, SchemaValidationError>`. `From<DomainError>` converts any domain error into `TypeMismatch` at path `/`.
Status: done.

### FR-043-006 Schema deserialisation depth guard

While deserialising a `Schema`, nesting through `Array::items`, `Object::properties` and `OneOf`/`AllOf::schemas` shall be counted by a thread-local counter (`SCHEMA_DESERIALIZE_DEPTH`); entering a level beyond `MAX_DESERIALIZE_DEPTH` (64) shall fail with `"Schema nesting depth exceeds maximum of 64"`. The counter shall return to its previous value after success, error or unwind.
Code: `deserialize_boxed_schema_option`, `deserialize_schema_properties`, `deserialize_schema_list`.
Status: done. With `serde_json`, its own ~128-token recursion limit usually triggers first (about 43 levels of `Object` nesting); the guard matters for formats without such a limit (MessagePack, CBOR).

### FR-043-007 Reusable guard primitive

`enter_deserialize_depth::<E: serde::de::Error>(counter: &'static LocalKey<Cell<usize>>, type_name: &str) -> Result<DepthGuard, E>` shall increment the counter or fail with `"{type_name} nesting depth exceeds maximum of 64"`; `DepthGuard` (`#[must_use]`) shall decrement on `Drop`. Each recursive type declares its own thread-local counter. `pjs-core`'s `SchemaDefinitionDto` uses the same primitive.
Status: done.

### FR-043-008 Wire encoding helpers

`serde_support` (doc-hidden) shall provide `#[serde(with = ...)]` modules: `priority` (bare `u8`, zero rejected on decode), `priority_map` (`HashMap<String, Priority>` as `u8` values, each validated), `session_id` and `stream_id` (UUID), `option_stream_id` and `option_session_id` (optional UUID; use with `#[serde(default)]`).
Status: done. Golden tests (`tests/serde_golden.rs`) decode literals into `Frame`, `Stream`, `DomainEvent` and require byte-identical re-encoding: priority as number, ids as UUID strings, `Option<StreamId>` omitted when `None`.

### FR-043-009 `StreamMap` wire format

`StreamMap` shall serialise as a map keyed by each stream's UUID string. On decode it shall reject: a key that is not a UUID, a key that differs from the embedded stream id (`"stream map key does not match stream id"`), and a duplicate stream id (`"duplicate stream id in stream map"`). It shall not pre-size from `size_hint`.
Status: done.

## Non-Functional Requirements

- NFR-SEC-043-001: stack-depth bound of 64 for `JsonData` ([[042-domain-crate-contract-and-value-objects/spec|042]]) and `Schema` deserialisation (CWE-674); regression history in CHANGELOG (#470, #476, shared primitive extracted).
- NFR-SEC-043-002: no attacker-controlled pre-allocation in `StreamMap` or `JsonData` decoding.
- NFR-COMPAT-043-001: wire encodings are frozen by golden tests; changing them is a breaking protocol change.

## Data Model

Types: `Schema`, `SchemaId`, `SchemaType`, `SchemaValidationError`, `SchemaValidationResult`, `DepthGuard`, `enter_deserialize_depth`, `MAX_DESERIALIZE_DEPTH` (=64, re-exported at crate root), `StreamMap`, and the `serde_support` modules.

## Edge Cases and Errors

| Case | Result |
|---|---|
| nested schema deeper than 64 levels (non-JSON formats) | deserialisation error naming "Schema" |
| `priority: 0` in a frame | deserialisation error from `Priority::new` |
| `StreamMap` entry with mismatched key | error, whole map rejected |
| schema with `min_length > max_length` or invalid regex | accepted at deserialisation; reported only at validation time (`InvalidPattern`, `PatternTooLong`) |
| `Schema::OneOf { schemas: [] }` | deserialises; `allows_type` is `false`; `validation_cost` is 30 |

## Success Criteria

- `cargo nextest run -p pjson-rs-domain` passes `schema_comprehensive`, `serde_golden`, and the depth-guard unit tests.
- Golden literals round-trip byte-identically.

## Agent Boundaries

- Always: put every new recursive deserialisable domain type behind `enter_deserialize_depth` with its own thread-local counter.
- Ask first: adding schema variants (wire-visible, validators must handle them); changing any `serde_support` encoding.
- Never: derive `Serialize`/`Deserialize` on `Priority` or `Id<T>`; compile regexes in the domain crate (validation lives in `pjs-core`).

## Open Questions

- `SchemaType::from(&Schema)` returns `Object` for `Any`, `OneOf`, `AllOf` ("most flexible"); this is lossy and surprising.
- `Schema` deserialisation does not check invariants (`min <= max`, `required` entries present in `properties`, pattern validity, empty `OneOf`); every invariant is deferred to validation time, which contradicts the "make illegal states unrepresentable" rule.
- `Schema::Number` holds `f64` bounds and derives `PartialEq`; NaN bounds are constructible.
- `SchemaValidationError` carries paths and expected/actual values as `String`s (stringly typed) rather than `JsonPath` and typed values.
- `From<DomainError> for SchemaValidationError` collapses every domain error into a `TypeMismatch` with `expected: "valid"`, which loses the error kind.
- Thread-local depth counters are not async-safe in principle (a guard held across an `.await` on a work-stealing runtime could count another task's depth); deserialisation is synchronous so this does not occur today.
- Documentation drift: the guard's doc claims `serde_json` limits Schema nesting to as low as ~43 levels; no test pins that number `[NEEDS CLARIFICATION]`.

## See Also

- [[043-domain-schema-depth-guard-wire-encoding/plan|Plan (as built)]]
- [[042-domain-crate-contract-and-value-objects/spec|042 Value objects]]
- [[044-domain-entities-events-priority-heuristic/spec|044 Entities, events, priority]]
- [[051-security-limits-and-validated-config/spec|051 Security limits]]
