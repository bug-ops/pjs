---
aliases:
  - Accept Header Negotiation Crate
  - RFC 9110 Specificity Tie-Break
tags:
  - sdd
  - spec
  - research
  - http
  - dependency
created: 2026-08-18
status: implemented
priority: P3
issue: "#518"
pr: "#520"
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
  - "[[035-streaming-module-unreachable/spec|035 streaming module]]"
code:
  - crates/pjs-core/src/infrastructure/http/streaming.rs
  - crates/pjs-core/tests/http_streaming_comprehensive.rs
  - crates/pjs-core/Cargo.toml
  - Cargo.toml
verified: 2026-11-15
---

# Feature: RFC 9110 Accept-Header Negotiation via a Dedicated Crate

> [!info] Verification (2026-11-15)
> **State: implemented** (adopted `headers-accept`).
>
> **Evidence**
> - Issue #518 is closed; PR #520 (commit `9ccb5dd`, `fix(http): use headers-accept crate for RFC 9110 Accept-header negotiation`) delivered it. CHANGELOG entry (BREAKING, `StreamFormat::from_accept_header`) cites #518.
> - Root `Cargo.toml`: `headers-accept = "0.3.0"`, `mediatype = "0.21.0"`; `crates/pjs-core/Cargo.toml` lists both as optional dependencies enabled by the `http-server` feature (`dep:headers-accept`, `dep:mediatype`).
> - `StreamFormat::from_accept_header` (`infrastructure/http/streaming.rs`): truncates the raw header to `MAX_ACCEPT_ENTRIES = 16` comma-separated entries, drops each entry that fails `is_supported_media_range` (only `*/*` and `application/*` wildcards allowed), parses `q` itself (unparsable or non-finite drops the entry; finite out-of-range is clamped to `[0, 1]`; formatted to at most 3 digits by `format_q`, never rounding a positive `q` to `0.000`), validates each entry with `MediaTypeBuf::from_str`, then calls `Accept::negotiate` over `SUPPORTED_MEDIA_TYPES` (order: JSON, SSE, NDJSON, binary). Missing/unparsable header or no candidate falls back to `StreamFormat::Json`.
> - Tests in `crates/pjs-core/tests/http_streaming_comprehensive.rs` (`test_stream_format_from_accept_header_*`), including `..._specificity_breaks_tied_q`, `..._q_nan_drops_entry`, `..._bounds_entries`, `..._malformed_entry_preserves_others`, `..._out_of_range_q_clamped_and_kept`, `..._tiny_positive_q_survives`, `..._bare_wildcard_subtype_is_rejected`.
> - Follow-up doc fix: issue #529 (closed), rustdoc overclaim on wildcard handling.
>
> **Drift from original spec**
> - Selected crate: `headers-accept` (maintainer decision), with `mediatype` used directly for per-entry validation and candidate construction.
> - Wildcard handling is deliberately narrower than RFC 9110 `type/*`: only `*/*` and `application/*` are accepted; other ranges are dropped.
> - Wildcard ties resolve in candidate order (JSON, SSE, NDJSON, binary), not purely by specificity: `application/json;q=0, */*` yields SSE; `application/json;q=0, application/*` yields NDJSON.
> - The bound is enforced before `headers-accept` (it has no bound of its own), as decided.
> - The dependency is gated behind `http-server` rather than always built.
> - `.local/specs/constitution.md` reference in NFR-003 now maps to [[constitution]].

## 1. Overview

### Problem Statement (historical)

The hand-rolled `Accept` parser broke `q`-ties by first occurrence instead of by RFC 9110 §12.5.1 media-range specificity, and had already needed two fixes (q parsing, substring matching) when it first became reachable via the route in [[035-streaming-module-unreachable/spec|035]].

### Goal

Replace the ranking logic with a conformant crate while preserving the 16-entry DoS bound and the permissive fallback.

### Out of Scope

- Other headers (`Accept-Encoding`, `Accept-Language`); introducing `406`; changing the route fallback decided in 035.

## 2. User Stories

- US-001: a maintainer decided to adopt a crate (done, `headers-accept`).
- US-002: an implementor replaced the logic without regressing q-value, NaN/Infinity, substring, or bound behaviour (done).

## 3. Functional Requirements

| ID | Requirement | Priority | Status |
|----|------------|----------|--------|
| FR-001 | THE SYSTEM SHALL record a maintainer decision | must | done (adopt `headers-accept`) |
| FR-002 | IF adoption THEN select exactly one crate and record the rationale | must | done |
| FR-003 | Rank by `q`, then by specificity (exact > `application/*` > `*/*`) | must | done (`Accept::negotiate`) |
| FR-004 | Preserve `MAX_ACCEPT_ENTRIES = 16` | must | done (raw-header truncation before the crate) |
| FR-005 | `q=0`, non-finite and unparsable `q` drop the entry; finite out-of-range `q` is clamped and kept | must | done |
| FR-006 | Permissive fallback to `StreamFormat::Json`, no `406` | must | done |
| FR-007 | Remove the self-disclosed specificity gap from the doc comment | should | done |
| FR-008 | IF declined THEN keep the disclosure | must | superseded |

## 4. Non-Functional Requirements

| ID | Category | Requirement | Status |
|----|----------|-------------|--------|
| NFR-001 | Correctness | RFC 9110 §12.5.1 for the supported media ranges | done, with the narrowed wildcard set above |
| NFR-002 | Security | 16-entry bound not weakened | done |
| NFR-003 | Dependency footprint | justify new crates | partial: `headers-accept` and `mediatype` added; justification is in PR #520 [NEEDS CLARIFICATION: `cargo deny`/`cargo tree` evidence not re-checked] |
| NFR-004 | Test parity | existing negotiation tests preserved | done |
| NFR-005 | Traceability | issue with one severity label | done (#518, P3, research) |

## 5. Data Model

| Entity | Description |
|--------|-------------|
| `StreamFormat` | unchanged enum: `Json`, `NdJson`, `ServerSentEvents`, `Binary` |
| `SUPPORTED_MEDIA_TYPES` | `[(MediaType<'static>, StreamFormat); 4]`: `application/json`, `text/event-stream`, `application/x-ndjson`, `application/octet-stream` |
| `X_NDJSON` | `Name::new_unchecked("x-ndjson")` (not in the `mediatype` registry) |

## 6. Edge Cases and Error Handling

| Scenario | Behaviour |
|----------|-----------|
| `application/*;q=1.0, application/x-ndjson;q=1.0` | NDJSON (specificity wins) |
| More than 16 entries | extras ignored |
| `q=nan`, `inf`, `-inf`, unparsable | entry dropped |
| `q=5` | clamped to 1.0, kept |
| `q=0.0004` | formatted as `0.001`, entry survives |
| `text/*`, `*/x-ndjson` | entry dropped (unsupported wildcard) |
| Malformed entry (`garbage!!`) | dropped alone; others kept |
| Non-UTF-8 header | `Json` |

## 7. Success Criteria

| ID | Metric | Target | Status |
|----|--------|--------|--------|
| SC-001 | decision recorded | adopt | done |
| SC-002 | specificity test passes | `NdJson` | done (`..._specificity_breaks_tied_q`) |
| SC-003 | no regression, bound enforced | 100% | done in CI |
| SC-004 | traceability | issue filed | done |

## 8. Agent Boundaries

**Always**: keep the 16-entry bound and the permissive fallback; run the full check suite.
**Ask first**: widening the wildcard set; adding another negotiation dependency.
**Never**: introduce `406` incidentally; delete negotiation tests without equivalents.

## 9. Open Questions

- Spec text said wildcard ties resolve by specificity only; the as-built rule includes candidate order (see rustdoc). Keep rustdoc as the source of truth (#529).
- `f32` parsing of `q` in `from_accept_header` plus manual formatting is custom logic around the crate; consider upstreaming or simplifying [NEEDS CLARIFICATION].

## 10. See Also

- [[constitution]], [[MOC-specs]], [[035-streaming-module-unreachable/spec|035]], [[063-http-streaming-negotiation/spec|063 HTTP streaming and negotiation (current behaviour)]]
- RFC 9110 §12.4.2 and §12.5.1
- Issues #518, #529; PRs #512, #520
