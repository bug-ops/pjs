---
tags:
  - sdd
  - delta
created: 2026-11-18
status: merged
related:
  - "[[044-domain-entities-events-priority-heuristic/spec|spec]]"
---

# Delta: no-unencodable-key-workarounds

Merged into [[044-domain-entities-events-priority-heuristic/spec|spec]] on 2026-11-18 (#698).

## MODIFIED
- FR-044-005 the skeleton no longer keeps values under special keys verbatim: every object key recurses like any other, and the skeleton shape is the input shape with leaves nulled and arrays emptied.
- FR-044-005 patch extraction walks objects and addresses every key (`.`, `[`, `]`, empty included) through a `JsonPath` with quoted segments; no subtree is skipped.

## REMOVED
- The root `Set` at index 0 and the unencodable-subtree projection (`Stream::unencodable_projection`); the sequence-reset caveat about that root `Set`.
- The statement that a key `JsonPath` cannot encode inside an array element travels verbatim without a root `Set` (arrays still travel whole in one `Set`), and the claim that a client receiving only the patch frames (no skeleton, as on the HTTP path) reconstructs such documents exactly: that guarantee came from the root `Set`. Patch frames now assume the skeleton (or a parent-creating client) provides the parent objects, as they already did for documents without special keys.

## Edge Cases
| Input | Result |
|---|---|
| `{"a.b": 7}` | skeleton `{"a.b": 0}`, patch `Set` at `$["a.b"]` = 7, no root patch |
| `{"": 8, "x": {"k]": 1}}` | patches at `$[""]` and `$.x["k]"]` |
