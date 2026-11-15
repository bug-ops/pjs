---
aliases:
  - WASM TypeScript Types
  - tsify Type Generation
tags:
  - sdd
  - spec
  - wasm
  - typescript
created: 2026-04-28
status: partial
priority: P3
issue: "#143"
pr: "#199"
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
code:
  - crates/pjs-wasm/src/streaming.rs
  - crates/pjs-wasm/Cargo.toml
  - crates/pjs-js-client/tsconfig.wasm-drift.json
  - .github/workflows/ci.yml
verified: 2026-11-15
---

# Feature: Typed TypeScript Definitions for pjs-wasm via tsify

> [!info] Verification (2026-11-15)
> **State: partial.** `FrameData` and `StreamStats` derive `Tsify`; callback type aliases are emitted; doc examples were fixed; a drift check exists in CI. `getPayloadObject()` was never added, and `onFrame`/`onComplete`/`onError` still accept an untyped `js_sys::Function`.
>
> **Evidence**
> - Issues #143, #138, #140 are CLOSED (2026-04-28); delivered by PR #199 (`24bcb66`, "add tsify-next for precise TypeScript types and fix doc examples"), CHANGELOG 0.5.2 entry citing #143.
> - `crates/pjs-wasm/src/streaming.rs`: `#[derive(Debug, Clone, Serialize, Deserialize, Tsify)]` on `StreamStats` (`#[serde(rename_all = "camelCase")]`) and `FrameData`; `#[wasm_bindgen(typescript_custom_section)]` const `TS_CALLBACK_TYPES` emits `FrameCallback`, `StreamStatsCallback`, `ErrorCallback`.
> - `crates/pjs-wasm/Cargo.toml`: `tsify = { workspace = true, features = ["js"] }`; workspace pin `tsify = "0.5"`, locked at 0.5.8. The crate used is `tsify`, not `tsify-next` (the CHANGELOG wording is stale).
> - `PriorityStream::on_frame/on_complete/on_error` take `js_sys::Function` (`#[wasm_bindgen(js_name = onFrame)]` etc.).
> - Commit `1211869` (#531) removed `#[tsify(into_wasm_abi, from_wasm_abi)]` from both structs because tsify 0.5.8 deprecated it and the types are already converted manually via `serde_wasm_bindgen`.
> - CI job `wasm-types-drift` (`.github/workflows/ci.yml`) downloads the built `pkg` and runs `npm run typecheck:wasm-drift` (`tsc --noEmit -p tsconfig.wasm-drift.json`) in `crates/pjs-js-client`.
>
> **Drift from original spec**
> - Crate is `tsify` 0.5.x, not `tsify-next ^0.5.6`; the `into_wasm_abi, from_wasm_abi` attribute from the sketch is gone.
> - `FrameData.payload` is a JSON `String`; no `getPayloadObject()` exists anywhere in `crates/` (spec premise was wrong). FR-3 is therefore moot.
> - The CI check is `wasm-types-drift` (against the real `pkg`), not a `wasm-pack --target bundler` + `tsc` step.
> - `frame_type` is renamed to `type` on the wire via `#[serde(rename = "type")]`.

## Overview

`wasm-pack` emits coarse `.d.ts` for `#[wasm_bindgen]` exports. Goal: precise, generated TypeScript for the data structs and callbacks of `pjs-wasm` so TS consumers need no casts and doc examples cannot reference non-existent methods. Out of scope: runtime behaviour changes of the bindings.

## User Stories

- As a TypeScript developer, I want typed `FrameData`/`StreamStats` so my IDE autocompletes `frame.type`, `frame.sequence`, `frame.priority`.
- As a maintainer, I want generated definitions derived from the Rust structs so docs and types cannot drift (#138/#140 class of bug).

## Functional Requirements

### FR-001: Tsify-derived interfaces for exported data structs
All data structs crossing the boundary by value derive `Tsify`.
Status: done (`FrameData`, `StreamStats`). `SecurityConfig`, `PriorityConfigBuilder`, `PjsParser`, `PriorityStream` are `#[wasm_bindgen]` classes and get native class declarations instead.

### FR-002: Typed callback aliases
`FrameCallback`, `StreamStatsCallback`, `ErrorCallback` are emitted as TS type aliases.
Status: partial. Aliases exist, but `onFrame`/`onComplete`/`onError` parameters are still `js_sys::Function` (emitted as `Function`), so the aliases are not applied to the method signatures. [NEEDS CLARIFICATION: confirm against a built `pkg/*.d.ts`; a `unchecked_param_type` attribute would bind them.]

### FR-003: Structured payload accessor
Status: superseded. No `getPayloadObject()` exists; `payload` is a JSON string by design (`FrameData` doc comment).

### FR-004: Doc examples match the binding
Status: done (PR #199 fixed #138, #140).

### FR-005: CI type check of consumer against generated `.d.ts`
Status: done in a different form (`wasm-types-drift` job, script `typecheck:wasm-drift`).

## Non-Functional Requirements

- NFR-001: `tsify` is a proc-macro compile-time cost only. Status: holds by construction.
- NFR-002: bundle growth <= 1 KB gzipped. Status: [NEEDS CLARIFICATION: not measured anywhere in the repo].

## Data Model

| Type | File | TS shape |
|------|------|----------|
| `FrameData` | `streaming.rs` | `{ type: string; sequence: number; priority: number; payload: string }` |
| `StreamStats` | `streaming.rs` | `{ totalFrames; patchFrames; bytesProcessed; durationMs }` (camelCase) |

## Success Criteria

- `wasm-types-drift` passes against the built `pkg`.
- `FrameData`/`StreamStats` appear as interfaces in the generated `.d.ts`.

## Agent Boundaries

- Always: keep `frame_type` serialized as `type`; run the drift check after changing exported shapes.
- Ask first: renaming exported JS names (semver-governed public API).
- Never: reintroduce the deprecated `into_wasm_abi, from_wasm_abi` tsify attributes.

## Open Questions

- `FrameData.frame_type` and `priority` are `String`/`u8` rather than a closed union / `Priority` newtype, and unknown frame types map to `"unknown"` (stringly typed boundary).
- Callback parameters remain untyped (FR-002).

## See Also

- [[020-ts-client-wasm-api-sync/spec|020 TS client / WASM API sync]]
- `docs/architecture/WASM_ROADMAP.md`
- tsify: https://crates.io/crates/tsify
