---
aliases:
  - ApiKeyConfig Debug Redaction
tags:
  - sdd
  - spec
  - security
  - http
  - auth
created: 2026-04-29
status: implemented
priority: P2
issue: "#216"
pr: "#219"
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
  - "[[007-http-auth-middleware/spec|Spec 007]]"
code:
  - crates/pjs-core/src/infrastructure/http/auth.rs
verified: 2026-11-15
---

# Spec 026: ApiKeyConfig - Redacting Debug Implementation

> [!info] Verification (2026-11-15)
> **State: implemented.** Issue #216 CLOSED 2026-04-28; PR #219 (`5446652`); CHANGELOG: "`ApiKeyConfig` no longer derives `Debug`; a hand-written impl redacts `hmac_key` and `keys`".
>
> Evidence: `impl fmt::Debug for ApiKeyConfig` in `auth.rs` prints `keys` as `"[N redacted tags]"` and `hmac_key` as `"[redacted]"`; test `api_key_config_debug_redacts_key_material`.
>
> Drift from original spec:
> - Debug output exposes the key count (allowed by FR-3).
> - No `len()` accessor was added (FR-5 SHOULD, not done; tests use `Debug` text instead).
> - The test asserts `"redacted"` is present and `"hmac_key: ["` is absent; it does not assert the secret string or its HMAC tag is absent (acceptance criterion 1 only partially covered).

## Problem

A derived `Debug` printed `keys: Vec<[u8; 32]>` and `hmac_key: [u8; 32]`; leaked logs would let an attacker recompute HMAC tags and bypass constant-time comparison.

## Functional Requirements

### FR-1 Manual `impl Debug` replaces derive
Status: done.

### FR-2 `hmac_key` fully redacted
Status: done.

### FR-3 Key bytes redacted, count may be shown
Status: done.

### FR-4 Tests do not rely on byte-level Debug output
Status: done.

### FR-5 `len()` accessor on `ApiKeyConfig`
Status: open (optional).

## Acceptance Criteria

- [~] Debug output contains "redacted": covered by test.
- [ ] Debug output of `ApiKeyConfig::new(&["secret"])` does not contain `"secret"` or its tag: no dedicated assertion. [NEEDS CLARIFICATION: add test]
- [x] Existing auth tests pass.
- [x] Clippy clean (CI gate).

## See Also

- [[007-http-auth-middleware/spec|Spec 007]]
- `secrecy` crate as optional alternative if more secrets are added
