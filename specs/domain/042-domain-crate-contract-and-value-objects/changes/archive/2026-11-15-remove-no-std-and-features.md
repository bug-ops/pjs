---
tags:
  - sdd
  - delta
created: 2026-11-15
status: merged
related:
  - "[[042-domain-crate-contract-and-value-objects/spec|spec]]"
---

# Delta: remove-no-std-and-features

Merged into [[042-domain-crate-contract-and-value-objects/spec|spec]] on 2026-11-15.

## MODIFIED
- FR-042-002: the crate declares no features.
- FR-042-003: `std` only; the `no_std` attribute, the `alloc` import and the `std` and `wasm` features were removed (#606).

## REMOVED
- Open question on the `no_std` claim and the empty `wasm` feature.
