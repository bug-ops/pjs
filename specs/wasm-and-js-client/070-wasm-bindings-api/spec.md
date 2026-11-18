---
aliases:
  - WASM bindings API
  - pjs-wasm public API
tags:
  - sdd
  - spec
  - wasm
  - api
created: 2026-11-15
status: implemented
priority: P2
related:
  - "[[071-wasm-security-limits/spec]]"
  - "[[072-wasm-packaging-targets/spec]]"
  - "[[074-js-client-wasm-integration/spec]]"
  - "[[010-wasm-typescript-type-generation/spec]]"
  - "[[033-wasm-security-config-shared-reuse/spec]]"
issue: "#242"
code:
  - crates/pjs-wasm/src/lib.rs
  - crates/pjs-wasm/src/parser.rs
  - crates/pjs-wasm/src/streaming.rs
  - crates/pjs-wasm/src/priority_constants.rs
  - crates/pjs-wasm/src/priority_config.rs
  - crates/pjs-wasm/src/priority_assignment.rs
  - crates/pjs-wasm/src/frame_generation.rs
verified: 2026-11-18
---

# Spec 070: pjs-wasm public binding API

Part of [[MOC-specs]]; governed by [[constitution]].

> [!info] Verification (2026-11-15)
> Existing functionality documented as built. Evidence: `crates/pjs-wasm/src/*.rs` read in full; native tests in `crates/pjs-wasm/tests/` and `#[wasm_bindgen_test]` modules in `parser.rs` / `streaming.rs`; CHANGELOG entries for #242, #326, #404, #433, #531.

## Overview

**Problem.** Browser and Node.js code needs the PJS skeleton-then-patches frame model without a server. `pjs-wasm` exposes the pure-domain logic of `pjson-rs-domain` through `wasm-bindgen`.

**Goal.** Document the JS-visible surface: `PjsParser`, `PriorityStream`, `PriorityConstants`, `PriorityConfigBuilder`, `SecurityConfig` (see [[071-wasm-security-limits/spec]]), `version()`, and the frame-generation algorithm both entry points share.

**Out of scope.** Packaging and build targets ([[072-wasm-packaging-targets/spec]]); the TypeScript consumers ([[074-js-client-wasm-integration/spec]]); wire format (`docs/architecture/SPECIFICATION.md`).

## User Stories

- As a web developer I parse JSON and receive priority-ordered frames so I can render important fields first.
- As a web developer I register `onFrame`/`onComplete`/`onError` callbacks and call `start(json)` to receive frames progressively.
- As a platform integrator I get the same priorities in the browser as from the HTTP transport for the same payload.

## Functional Requirements

### FR-001 Module initialisation and version
When the module loads, the system shall run `init()` (`#[wasm_bindgen(start)]`), installing `console_error_panic_hook` when the default feature of the same name is enabled. `version()` and `PjsParser.version()` shall return `CARGO_PKG_VERSION` (the workspace version).
Code: `lib.rs` (`init`, `version`), `utils.rs` (`set_panic_hook`), `parser.rs` (`PjsParser::version`).
Status: done.

### FR-002 PjsParser construction
The system shall provide `new PjsParser()`, `PjsParser.withConfig(PriorityConfigBuilder)` and `PjsParser.withSecurityConfig(SecurityConfig)`. Each constructor defaults the unspecified half (priority config or security config) to its default. `withConfig` and `withSecurityConfig` are mutually exclusive: no constructor sets both.
Code: `parser.rs`.
Status: done. See Open Questions (no combined constructor).

### FR-003 PjsParser.parse
When `parse(json)` is called, the system shall: (1) reject input longer than `maxJsonSize` bytes, (2) parse with `serde_json`, (3) reject any array or object (at any depth) exceeding the configured element/key limits, (4) convert to `JsonData` and return it to JS through `serde_wasm_bindgen`. Errors are thrown as JS strings prefixed `Security error:`, `Parse error:` or `Conversion error:`.
Code: `parser.rs::parse`, `security.rs::validate_input_size`, `validate_json_structure`.
Status: done.

### FR-004 PjsParser.generateFrames
When `generateFrames(json, minPriority)` is called, the system shall run the FR-003 validation steps, build a stream id from the fixed UUID `00000000-0000-0000-0000-000000000001`, convert `minPriority` with `Priority::new` (zero is rejected, `1..=255` accepted), and return an array of serialized domain `Frame`s: one skeleton frame, one patch frame per distinct priority at or above `minPriority` in descending order, and one completion frame.
Code: `parser.rs::generate_frames`, `frame_generation.rs::generate_frames`.
Status: done.

### FR-005 Frame generation algorithm
The system shall implement frame generation once, in `frame_generation::generate_frames`, shared by `PjsParser` and `PriorityStream`:
1. Skeleton frame (sequence 0): same shape as the input, leaves replaced by `null` (strings), `0`, `0.0`, `false`; arrays replaced by `[]`; no value is kept verbatim by key: every key recurses and nulls leaves like any other; recursion truncated to `null` at `max_depth` (from `SecurityConfig.maxDepth`). Built by the shared domain function `pjson_rs_domain::services::skeleton`, the same one `Stream` uses (#758).
2. Fields extracted recursively through objects and arrays up to `max_depth` for every key (`JsonPath` quoted segments address keys with `.`, `[`, `]` or the empty key), each assigned a `Priority` by `PriorityAssigner`; no field is skipped.
3. Fields grouped by priority, priorities sorted descending; groups below `min_priority` skipped; each remaining group becomes one patch frame of `set` patches.
3a. A non-object root (array or scalar) skips steps 2-3: after the skeleton (`[]` / `null`) it emits one patch frame at `Priority::CRITICAL` holding a single root `set` (`$`) with the whole value, exempt from `min_priority`, because indexed patches cannot create the array and the skeleton alone loses the value. The value is shipped whole, deeper than `max_depth` included: truncating a root `set` would corrupt the data, and nesting is already bounded by the `serde_json` parse recursion cap (about 128 levels) and `maxJsonSize`. `max_depth` truncation (steps 1-2) applies to object roots only.
4. Completion frame last. Sequence numbers are contiguous from 0.
Code: `frame_generation.rs` (`generate_frames`; the skeleton is `pjson_rs_domain::services::skeleton`, #758), `priority_assignment.rs` (`group_by_priority`, `sort_priorities`).
Status: done.

### FR-006 Priority assignment parity with HTTP
The system shall compute every field priority by delegating to `pjson_rs_domain::services::compute_priority` with the `PriorityHeuristicConfig` the assigner is built with (`PriorityAssigner::with_config`; the Rust-only `PriorityConfig` copy was removed in #771) and a `ValueShape` of the value (#755), so WASM and HTTP transports produce identical priorities for identical `(path, value)` input. Matching is case-insensitive on the last path segment. The assigner's configuration is fixed at construction; a call allocates nothing.
Code: `priority_assignment.rs` (`PriorityAssigner::calculate_field_priority`); regression test `crates/pjs-wasm/tests/priority_parity.rs` (issue #242).
Status: done.

### FR-007 PriorityConfigBuilder
The system shall expose a chainable builder (methods take and return `self`): `addCriticalField`, `addHighField`, `addLowPattern`, `addBackgroundPattern`, `setLargeArrayThreshold`, `setLargeStringThreshold`. Added names are de-duplicated (`push_unique`); thresholds are unvalidated `usize`. The builder is consumed by `PjsParser.withConfig` / `PriorityStream.withConfig`, which clone its config.
Code: `priority_config.rs`.
Status: done.

### FR-008 PriorityConstants
The system shall expose static getters `CRITICAL` (100), `HIGH` (80), `MEDIUM` (50), `LOW` (25), `BACKGROUND` (10), read from `Priority::*` in the domain crate.
Code: `priority_constants.rs`.
Status: done. Note: they are generated as static functions (`PriorityConstants.MEDIUM()` in the crate-level docs) although the item docs show property syntax; see Open Questions.

### FR-009 PriorityStream callbacks
The system shall provide `new PriorityStream()`, `PriorityStream.withConfig(builder)`, `setSecurityConfig(&SecurityConfig)`, `setMinPriority(u8)` (default 1; throws for `0`), `onFrame`, `onComplete`, `onError` (each stores one `js_sys::Function`; registering again replaces it).
Code: `streaming.rs`.
Status: done.

### FR-010 PriorityStream.start
When `start(json)` is called, the system shall perform the same validation as FR-003, generate frames (FR-005), invoke `onFrame` once per frame in order, then `onComplete` with `StreamStats { totalFrames, patchFrames, bytesProcessed, durationMs }`. On any failure it shall invoke `onError` with the message (if registered) and also throw a JS string. Frame delivery is synchronous: all callbacks fire before `start` returns. Callback exceptions are swallowed (`let _ = callback.call1(...)`).
Code: `streaming.rs::start`, `emit_frame`, `emit_complete`, `emit_error`.
Status: done.

### FR-011 FrameData and StreamStats shapes
`onFrame` receives `FrameData { type: "skeleton"|"patch"|"complete"|"error"|"unknown", sequence: u64, priority: u8, payload: string }` where `payload` is the frame payload serialized to a JSON string. `StreamStats` is camelCase with `u32` counters and `f64` `durationMs`. Both derive `tsify::Tsify` for generated `.d.ts`; TypeScript callback aliases (`FrameCallback`, `StreamStatsCallback`, `ErrorCallback`) come from a `typescript_custom_section`.
Code: `streaming.rs` (`FrameData`, `StreamStats`, `TS_CALLBACK_TYPES`).
Status: done.

## Non-Functional Requirements

- NFR-SIZE-001: release build is optimised with `wasm-opt -Os` (`[package.metadata.wasm-pack.profile.release]`); CI warns above 200 KB raw / 80 KB gzipped ([[072-wasm-packaging-targets/spec]]).
- NFR-PARITY-001: `tests/priority_parity.rs` must pass: WASM and domain priorities agree for default and user-extended configs.
- NFR-DETERMINISM-001: output for identical input and config is identical; the stream id is a fixed constant (chosen to avoid Node.js `crypto` dependence, per the comment in `parser.rs`).

## Data Model

| Type | Kind | Notes |
|------|------|-------|
| `PjsParser` | `#[wasm_bindgen]` struct | `PriorityAssigner` + `SecurityConfig` |
| `PriorityStream` | `#[wasm_bindgen]` struct | assigner, `min_priority: u8`, `SecurityConfig`, 3 optional callbacks |
| `PriorityConfigBuilder` | `#[wasm_bindgen]`, `Clone` | wraps the domain `PriorityHeuristicConfig`; JS exposes tier lists and two thresholds, the other knobs stay defaults |
| `PriorityAssigner`, `PrioritizedField` | Rust-only | `pub` for tests; `PriorityAssigner` has `new`, `with_config`, `calculate_field_priority` and `extract_prioritized_fields_with_limit` |
| `FrameData`, `StreamStats` | serde + `Tsify` | callback payloads |
| `PriorityConstants` | unit struct | static getters |

## Edge Cases and Errors

| Condition | Behaviour |
|-----------|-----------|
| Input larger than `maxJsonSize` | throws `Security error: Input too large ...`; `start` also calls `onError` |
| Invalid JSON | throws `Parse error: ...` |
| Array/object over limit at any depth | throws `Security error: Array too large ...` / `Object too large ...` |
| `minPriority == 0` (`generateFrames`) | throws `Invalid priority: ...` |
| `setMinPriority(0)` | throws `Priority must be between 1 and 255` |
| Nesting deeper than `maxDepth` | object roots: truncated (skeleton `null`, fields not extracted), not rejected; non-object roots: shipped whole in the root `set` |
| Array or scalar root | skeleton `[]`/`null`, one CRITICAL root `set` frame, completion; any `minPriority` |
| Frame of unknown type in `FrameData::from` | `type: "unknown"` (domain `FrameType` is non-exhaustive) |
| Payload serialization failure | `payload: "null"` |

## Success Criteria

1. `cargo nextest run -p pjs-wasm --tests` passes natively (CI job `wasm-native-tests`).
2. The wasm-pack build exposes `PjsParser`, `PriorityConstants`, `PriorityConfigBuilder`, `version` in the generated `.d.ts` (CI job `wasm-package-validation`).
3. `crates/pjs-js-client` type-checks against the real generated bindings (`wasm-types-drift`).

## Agent Boundaries

- Always: keep priority logic delegated to `pjson-rs-domain`; keep one frame-generation implementation; update `crates/pjs-js-client/src/types/pjs-wasm.d.ts` when the JS API changes.
- Ask first: changing the exported JS names, `FrameData` shape, or default limits (semver-governed per `.claude/rules/continuous-improvement.md`).
- Never: add a second priority table in this crate; use `sonic-rs` here (not WASM-compatible); introduce async APIs without updating callbacks docs.

## Open Questions

- `PriorityConstants` docs and the `lib.rs` example disagree on call syntax (`MEDIUM` vs `MEDIUM()`); the getters are `pub fn` with `#[wasm_bindgen(getter)]` on a unit struct, so generated JS shape needs confirmation. [NEEDS CLARIFICATION: generated d.ts for `PriorityConstants`]
- `PjsParser` offers no constructor taking both a `PriorityConfigBuilder` and a `SecurityConfig`.
- `PjsParser::version` duplicates the free `version()` function.
- `PriorityStream::set_min_priority` accepts 1..=255 although domain priorities in practice range 1..=100.
- Typed errors: all JS errors are `JsValue::from_str(format!(..))` strings; `SecurityError` is not exposed as a typed JS error (stringly typed).
- Item docs in `priority_constants.rs` use property syntax, the crate docs use call syntax.
- `FrameData.type` is a `String`, not a TS union.
- Issue #591 (open): `pjs-wasm` lacks `#![forbid(unsafe_code)]`.

## See Also

[[plan]] | [[071-wasm-security-limits/spec]] | [[072-wasm-packaging-targets/spec]] | [[074-js-client-wasm-integration/spec]] | `docs/architecture/WASM_ROADMAP.md` (planning-era document; still shows `src/api.rs`/`types.rs` that do not exist) | `crates/pjs-wasm/README.md`
