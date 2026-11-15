---
aliases:
  - Fused UTF-8 Parse Validation
  - SonicParser UTF-8 Double-Pass
tags:
  - sdd
  - spec
  - research
  - parser
  - performance
created: 2026-08-18
status: obsolete
priority: P4
issue: "#454"
pr: "#459"
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
code:
  - crates/pjs-core/src/parser/zero_copy.rs
  - crates/pjs-core/src/parser/simd/mod.rs
verified: 2026-11-15
---

# Feature: Fuse UTF-8 Validation with sonic-rs SIMD Parse in SonicParser

> [!info] Verification (2026-11-15)
> **State: obsolete.** Implemented, then the target code was deleted.
>
> **Evidence**
> - Issue #454 ("research(parser): fuse UTF-8 validation into sonic-rs SIMD parse pass ...") is closed. PR #459 (commit `77596b9`, "correct running-mean calculation and fuse UTF-8 validation into SIMD parse") implemented it: `SonicParser::parse` called `sonic_rs::from_slice(input)` and the depth pre-check (`pre_validate_json_bytes`) scanned bytes. CHANGELOG ("Changed", #454) records the new error variant: invalid UTF-8 became `Error::InvalidJson` instead of `Error::Other`.
> - PR #462 (commit `8fd1711`) then made the byte-level pre-validation string-literal aware (depth-bound bypass fix, #456).
> - PR #495 (commit `fd15815`, `refactor(pjson-rs)!: remove dead Frame/semantic/SonicParser chain`) removed `Parser`, `SimpleParser`, `SonicParser`, `parser::{simple, sonic}`: no production caller. `crates/pjs-core/src/parser/` now contains `aligned_alloc.rs`, `buffer_pool.rs`, `mod.rs`, `partial.rs`, `simd/`, `zero_copy.rs`; `rg SonicParser crates` has no code hits.
> - `sonic-rs` remains a dependency (`crates/pjs-core/Cargo.toml`; feature `simd-avx512 = ["sonic-rs/avx512"]`) and is used by `parser/simd/mod.rs` (`sonic_rs::Value` helpers) and `infrastructure/http/streaming.rs`, not for a `SonicParser::parse` entry point.
>
> **Drift from original spec**
> - All FRs targeted `SonicParser::parse` / `pre_validate_json_string`; neither exists any more.
> - FR-003 error mapping: delivered as `Error::InvalidJson` (not `Error::Other("Invalid UTF-8 input: ...")`).
> - The depth guard survives as `SecurityValidator::validate_json_depth_bytes` (public primitive) and `ZeroCopyParser` per-level `validate_json_depth`, per the CHANGELOG removal entry.
> - No before/after benchmark numbers were attached to PR #459 in this verification. [NEEDS CLARIFICATION]

## 1. Overview

### Problem Statement (historical)

`SonicParser::parse` made two passes: `std::str::from_utf8` then `sonic_rs::from_str`, plus a `.chars()` depth pre-scan. `sonic_rs::from_slice` validates UTF-8 with SIMD in the same pass, so the std pass was redundant.

### Goal (historical)

Single fused SIMD pass for UTF-8 validation and parsing, with a byte-level, ASCII-safe depth pre-check preserving all security bounds.

### Out of Scope

- Parser selection in `parser/hybrid.rs` (file no longer exists), `zero_copy`, `simple`; `pjs-wasm`; `from_slice_unchecked`.

## 2. User Stories

Retained only as history: US-001 (no redundant pass), US-002 (benchmark parity), US-003 (depth-bound still effective on raw bytes).

## 3. Functional Requirements

| ID | Requirement | Status |
|----|------------|--------|
| FR-001 | `SonicParser::parse` validates UTF-8 and parses via `sonic_rs::from_slice` | superseded (done in #459, code removed in #495) |
| FR-002 | depth pre-check scans `&[u8]` for ASCII `{ [ } ]` | superseded (same) |
| FR-003 | invalid UTF-8 yields an equivalent parse error | superseded (variant changed to `InvalidJson`) |
| FR-004 | depth check runs before the sonic parse | superseded |
| FR-005 | post-parse structure validation unchanged | superseded |
| FR-006 | identical depth-counting semantics | superseded (string-literal awareness added in #462, then removed with the type) |

## 4. Non-Functional Requirements

| ID | Category | Requirement | Status |
|----|----------|-------------|--------|
| NFR-001 | Performance | no regression in `SonicParser::parse` benches | superseded (bench group removed in #495) |
| NFR-002 | Security | DoS protections equivalent | superseded; surviving guards: `SecurityValidator::validate_json_depth_bytes`, `ZeroCopyParser` |
| NFR-003 | Compatibility | error shapes | superseded |
| NFR-004 | Maintainability | document ASCII-safety argument | superseded |

## 5. Data Model

None current.

## 6. Edge Cases and Error Handling

Historical reasoning that remains valid for any byte-level delimiter scan: ASCII bytes `0x7B 0x5B 0x7D 0x5D` are `< 0x80` and can never be continuation bytes of a UTF-8 sequence, so scanning raw bytes for them is sound without prior UTF-8 validation.

## 7. Success Criteria

Not applicable (superseded).

## 8. Agent Boundaries

**Never**: reintroduce `from_slice_unchecked` on untrusted input; weaken the nesting-depth bound.

## 9. Open Questions

- If a sonic-rs-based ingest path is reintroduced, reuse the byte-level depth scan (`validate_json_depth_bytes`) and `from_slice` rather than `from_utf8` + `from_str`. [NEEDS CLARIFICATION: no such path exists today]

## 10. See Also

- [[constitution]], [[MOC-specs]]
- Issues #454, #456; PRs #459, #462, #495
