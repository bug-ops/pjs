---
tags:
  - sdd
  - delta
created: 2026-11-18
status: merged
related:
  - "[[042-domain-crate-contract-and-value-objects/spec|spec]]"
---

# Delta: jsonpath-grammar-closed-segments

Merged into [[042-domain-crate-contract-and-value-objects/spec|spec]] on 2026-11-18 (#713, #698).

## MODIFIED
- FR-042-011 `PathSegment` is a closed enum (`Key(String)` or `Index(usize)`); `#[non_exhaustive]` is removed so consumers match exhaustively. A key is any `String`, including the empty key and keys containing `.`, `[` or `]`.
- FR-042-011 `append_key(&str) -> JsonPath` and `from_segments(iter) -> JsonPath` are infallible; `is_valid_key` and key validation are removed.
- FR-042-011 text grammar: `path = "$" *( "." plain / "[" index "]" / "[" quoted "]" )`.
  - `plain`: non-empty, none of `.`, `[`, `]`; written as `.plain` (bytes identical to the previous form).
  - `index`: `0` or `[1-9][0-9]*` (ASCII digits only); the closing `]` is required; a value above `usize::MAX` is an error. The bound is the implementation's native index type (`usize`: 2^64-1 native, 2^32-1 on wasm32; JS client: `Number.MAX_SAFE_INTEGER`), so `append_index` always round-trips; indices above a bound are undefined behaviour of the cross-implementation contract.
  - `quoted`: a JSON string literal in `serde_json` canonical escaping, used if and only if the key is not plain.
- `Display` writes `[` + `serde_json::to_string(key)` + `]` for a non-plain key. `FromStr` scans the literal to its unescaped closing quote, decodes it, and rejects it when re-encoding differs from the slice (non-canonical escapes) or when the key is plain; then it requires `]`.
- Invariant: every accepted string re-renders identically (`from_str(p).to_string() == p`), and `Display` then `FromStr` is the identity for every path (proptest over arbitrary keys and indices). Serde keeps using this text form.
- FR-042-011 `to_json_pointer()` renders the root as `""` (RFC 6901) and `$[""]` as `"/"`; `~` and `/` in keys stay unescaped (open question, #379).

## REMOVED
- The `JP-1` restriction that keys exclude `.`, `[`, `]` and the empty string, and the key-validation error from `append_key`/`from_segments`.

## Edge Cases
| Input | Result |
|---|---|
| `$[0`, `$.a[0`, `$[+5]`, `$[007]`, `$[00]`, `$[-1]`, `$[]` | `InvalidPath` |
| `$["a.b"]`, `$[""]`, `$.x["k]"]` | ok; re-render identically |
| `$["a"]` (plain key quoted), `$["\u0041.b"]` (non-canonical escape) | `InvalidPath` |
| `append_key("a.b").to_string()` | `$["a.b"]` |
