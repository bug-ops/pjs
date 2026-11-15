# PJS Specifications

Tracked, living specifications for the PJS workspace, written in Obsidian-flavoured Markdown (open this folder as a vault, or read the files directly). Start at [MOC-specs.md](MOC-specs.md).

| Path | Content |
|------|---------|
| `MOC-specs.md` | Index of all specs with status, priority and issue |
| `constitution.md` | Non-negotiable project principles |
| `architecture-baseline.md` | Verified design-pattern inventory and drift registry |
| `testing-strategy.md` | Test layers, CI gates, regression guards |
| `decisions/` | Architecture decision records |
| `requirements/` | Reverse-engineered BRD, SRS and NFR (ISO/IEC/IEEE 29148, ISO/IEC 25010) |
| `parser/` | Parsing: SIMD backends, sonic-rs, zero-copy, partial JSON |
| `memory/` | Buffer pool, allocators, interning, regex caches |
| `domain/` | pjs-domain value objects, entities, GAT ports, aggregates, services |
| `streaming/` | Priority planning, reconstruction, stream contracts and formats |
| `compression/` | Compression algorithms, dictionaries, schema strategies |
| `security/` | Limits, bomb detection, rate limiting, validated config |
| `application/` | CQRS commands, queries, DTOs, repositories, adapters, crate API |
| `http/` | REST API, middleware, auth, negotiation, metrics, serving |
| `websocket/` | Server and client transport, wire protocol |
| `wasm-and-js-client/` | WASM bindings, packaging, TypeScript client |
| `tooling/` | Bench, fuzz, demo, CI, release, test harnesses |
| `planning/` | Roadmaps |

Spec folders keep their numeric IDs (`001-037` issue-driven, re-verified against code; `038-079` existing functionality as built, `spec.md` plus as-built `plan.md`) inside the area folders. Wikilinks resolve by path suffix, so `[[NNN-name/spec]]` works from any location.

Conventions: each spec carries `status` (`implemented`, `partial`, `draft`, `obsolete`), `priority`, `issue`/`pr`, `code` paths and a `verified` date; migrated specs include a Verification callout listing drift from the original. A change to an existing capability is a delta against its `spec.md`, not a new spec. Last full verification: 2026-11-15 at HEAD dcf9b11.
