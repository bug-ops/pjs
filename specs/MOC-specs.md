---
aliases:
  - Specifications Index
  - Specs Overview
tags:
  - moc
  - sdd
  - pjs
created: 2026-11-15
status: moc
---

# Specifications

> [!abstract]
> Map of Content for all PJS specifications. Specs 001-037 were originally written as issue-driven specs and were re-verified against the code on 2026-11-15; specs 038-079 document existing functionality as built (living specs). Status values: `implemented`, `partial`, `draft`, `obsolete`.

Counts across 79 spec folders: 52 implemented, 21 partial, 1 draft, 5 obsolete.

## Project Foundation

- [[constitution]] - non-negotiable project principles
- [[architecture-baseline]] - verified inventory of design patterns and known drift
- [[testing-strategy]] - test layers, CI gates, regression guards
- [[decisions/adr-001-allocator-decision|ADR-001 allocator]] - mimalloc opt-in decision
- [[requirements/BRD-pjs-2026-11-15|BRD]] / [[requirements/SRS-pjs-2026-11-15|SRS]] / [[requirements/NFR-pjs-2026-11-15|NFR]] - reverse-engineered requirements
- [[023-implementation-roadmap/spec|023 Implementation roadmap (retrospective)]]
- [`docs/architecture/SPECIFICATION.md`](../docs/architecture/SPECIFICATION.md) - wire protocol draft (differs from implemented wire form, see [[045-priority-stream-planning/spec|045]])

## Parsing and memory

| ID | Spec | Status | Priority | Issue |
|----|------|--------|----------|-------|
| 001 | [[001-simd-backend-implementation/spec\|SIMD Backend Implementation]] | obsolete | P3 | #115 |
| 003 | [[003-sonic-rs-avx512-feature/spec\|Expose sonic-rs AVX-512 Feature Flag]] | implemented | P3 | #116 |
| 004 | [[004-partial-json-streaming/spec\|Partial JSON Parsing for Streaming Frames]] | partial | P4 | #117 |
| 005 | [[005-string-arena-safe-interning/spec\|StringArena Safe Interning]] | obsolete | P1 | #124 |
| 006 | [[006-sonic-rs-primary-parser-path/spec\|Wire sonic-rs as the Active Parser Through SIMD Feature Flags]] | implemented | P2 | #125 |
| 034 | [[034-fused-utf8-parse-validation/spec\|Fuse UTF-8 Validation with sonic-rs SIMD Parse in SonicParser]] | obsolete | P4 | #454 |
| 038 | [[038-zero-copy-lazy-json-parser/spec\|Zero-Copy Lazy JSON Parser]] | implemented | P2 |  |
| 039 | [[039-partial-json-jiter-parser/spec\|Partial JSON Parser (`partial-parse` feature)]] | partial | P4 | #117 |
| 040 | [[040-buffer-pool-aligned-alloc/spec\|Buffer Pool and Aligned Allocator]] | implemented | P2 |  |
| 041 | [[041-simd-features-and-global-allocator/spec\|SIMD Features, Feature Matrix and Global Allocator]] | implemented | P3 |  |

## Domain, application and infrastructure

| ID | Spec | Status | Priority | Issue |
|----|------|--------|----------|-------|
| 008 | [[008-cursor-pagination-active-sessions/spec\|Pagination for GetActiveSessionsQuery]] | partial | P3 | #136 |
| 018 | [[018-config-validation-startup/spec\|PjsConfig Startup Validation]] | partial | P3 | #175 |
| 019 | [[019-jsondata-nan-handling/spec\|JsonData NaN/Infinity Handling]] | implemented | P3 | #176 |
| 042 | [[042-domain-crate-contract-and-value-objects/spec\|Domain Crate Contract and Core Value Objects]] | implemented | P2 |  |
| 043 | [[043-domain-schema-depth-guard-wire-encoding/spec\|Domain Schema, Deserialisation Depth Guard and Wire Encoding]] | implemented | P2 |  |
| 044 | [[044-domain-entities-events-priority-heuristic/spec\|Domain Entities, Events and Priority Heuristic]] | implemented | P2 |  |
| 052 | [[052-domain-gat-ports/spec\|Domain GAT Ports]] | implemented |  |  |
| 053 | [[053-stream-session-aggregate/spec\|StreamSession Aggregate]] | implemented |  |  |
| 054 | [[054-domain-services/spec\|Domain Services]] | partial |  |  |
| 055 | [[055-application-commands/spec\|Application Commands and Command Handlers]] | implemented |  |  |
| 056 | [[056-application-queries-dto/spec\|Application Queries, Query Handlers, Pagination and DTOs]] | implemented |  |  |
| 057 | [[057-in-memory-repositories/spec\|In-Memory Repository, Stream Store, Frame Store and Limits]] | implemented |  |  |
| 058 | [[058-event-metrics-dictionary-adapters/spec\|Event Publisher, Metrics Collector and Dictionary Store Adapters]] | partial |  |  |
| 059 | [[059-crate-api-errors-features/spec\|Crate Public API Surface, `pjson_rs::Error` and Feature-Flag Matrix]] | partial |  |  |

## Streaming, compression and security

| ID | Spec | Status | Priority | Issue |
|----|------|--------|----------|-------|
| 002 | [[002-real-compression-algorithms/spec\|Real Compression Algorithm Backends]] | implemented | P2 | #114 |
| 011 | [[011-zstd-dictionary-compression/spec\|Zstd Trained-Dictionary Compression for Frame Transport]] | partial | P3 | #144 |
| 013 | [[013-regex-lazylock-cache/spec\|Compiled-Regex Cache in ValidationService]] | implemented | P3 | #154 |
| 024 | [[024-compress-owned-bytes-return/spec\|Fix maybe_compress_owned - Return Bytes Instead of Corrupted String]] | obsolete | P2 | #214 |
| 031 | [[031-array-duplication-reconstruction/spec\|PriorityStreamer::analyze() + JsonReconstructor Must Not Duplicate Object-Nested Arrays]] | implemented | P0 | #394 |
| 032 | [[032-session-stats-total-bytes-noop/spec\|SessionStats::total_bytes Must Reflect Real Payload Bytes]] | implemented | P2 | #400 |
| 045 | [[045-priority-stream-planning/spec\|Priority Stream Planning (skeleton + patches)]] | partial |  | #394 |
| 046 | [[046-json-reconstruction/spec\|JSON Reconstruction]] | implemented |  |  |
| 047 | [[047-streaming-frame-compression/spec\|Streaming Frame Compression]] | implemented |  | #603 |
| 048 | [[048-schema-compression-strategies/spec\|Schema Compression Strategies (Layer A)]] | implemented |  | #333 |
| 049 | [[049-secure-byte-compression/spec\|Secure Byte Compression (Layer B)]] | partial |  | #114 |
| 050 | [[050-compression-bomb-and-rate-limiting/spec\|Compression-Bomb Protection and Per-IP Rate Limiting]] | implemented |  | #346 |
| 051 | [[051-security-limits-and-validated-config/spec\|Security Limits and Validated Configuration]] | partial |  | #613 |

## Transport

| ID | Spec | Status | Priority | Issue |
|----|------|--------|----------|-------|
| 007 | [[007-http-auth-middleware/spec\|HTTP Authentication Middleware for PJS Endpoints]] | partial | P3 | #135 |
| 009 | [[009-realtime-metrics-observability/spec\|Real-time Metrics and Observability]] | partial | P2 | #142 |
| 012 | [[012-websocket-integration-test-infra/spec\|WebSocket Integration Test Infrastructure]] | implemented | P2 | #158 |
| 014 | [[014-stream-poll-waker-contract/spec\|Stream poll_next Waker Contract (Detection and Prevention)]] | obsolete | P2 | #161 |
| 015 | [[015-batch-stream-ndjson-format/spec\|BatchFrameStream NDJSON Output Format]] | implemented | P2 | #167 |
| 016 | [[016-async-stream-test-harness/spec\|Async Stream Test Harness]] | partial | P2 | #168 |
| 017 | [[017-axum-error-http-status-mapping/spec\|Axum Error to HTTP Status Mapping]] | implemented | P2 | #173 |
| 025 | [[025-streaming-response-content-type-helper/spec\|create_streaming_response_with_content_type Helper]] | implemented | P2 | #215 |
| 026 | [[026-apikey-config-debug-redaction/spec\|ApiKeyConfig - Redacting Debug Implementation]] | implemented | P2 | #216 |
| 029 | [[029-websocket-stream-session-binding/spec\|Fix websocket-streaming-server Session/WebSocket URL Mismatch]] | implemented | P2 | #382 |
| 035 | [[035-streaming-module-unreachable/spec\|Resolve `infrastructure::http::streaming` Module Unreachability]] | implemented | P2 | #511 |
| 036 | [[036-accept-header-negotiation-crate/spec\|RFC 9110 Accept-Header Negotiation via a Dedicated Crate]] | implemented | P3 | #518 |
| 037 | [[037-graceful-shutdown-serve-with-limits/spec\|Graceful Shutdown for serve_with_limits]] | draft | P2 | #590 |
| 060 | [[060-http-rest-api/spec\|HTTP REST API (route table, DTOs, errors, router constructors)]] | implemented | P2 |  |
| 061 | [[061-http-middleware-stack/spec\|HTTP middleware stack: layers, rate limiting, auth, security headers]] | partial | P2 |  |
| 062 | [[062-http-serve-connection-limits/spec\|Connection-level protection: `serve_with_limits`]] | implemented | P2 | #523 |
| 063 | [[063-http-streaming-negotiation/spec\|HTTP frame streaming and `Accept` negotiation]] | implemented | P2 | #511 |
| 064 | [[064-http-axum-extension/spec\|`PjsExtension`: bolt-on PJS routes for existing axum apps]] | partial | P3 |  |
| 065 | [[065-http-prometheus-metrics/spec\|Prometheus `/metrics` endpoint]] | partial | P3 |  |
| 066 | [[066-websocket-server-transport/spec\|WebSocket server transport: upgrade, ownership, caps and TTL]] | partial | P2 |  |
| 067 | [[067-websocket-wire-protocol/spec\|WebSocket wire protocol (`WsMessage`)]] | implemented | P2 |  |
| 068 | [[068-websocket-client/spec\|`PjsWebSocketClient`: client sessions, caps and TTL]] | partial | P2 |  |
| 069 | [[069-transport-features-and-limits/spec\|Transport feature flags and limits catalog]] | implemented | P3 |  |

## Clients and tooling

| ID | Spec | Status | Priority | Issue |
|----|------|--------|----------|-------|
| 010 | [[010-wasm-typescript-type-generation/spec\|Typed TypeScript Definitions for pjs-wasm via tsify]] | partial | P3 | #143 |
| 020 | [[020-ts-client-wasm-api-sync/spec\|TypeScript Client - WASM/Rust API Sync Strategy]] | implemented | P1 | #178 |
| 021 | [[021-bench-crate-workspace-isolation/spec\|pjs-bench - Workspace Isolation Pattern]] | implemented | P2 | #179 |
| 022 | [[022-ci-js-client-enforcement/spec\|CI Enforcement for pjs-js-client npm Tests]] | implemented | P2 | #180 |
| 027 | [[027-wasm-backend-node-init/spec\|WasmBackend.connect() in Node.js - fetch() Cannot Load file: URL]] | implemented | P1 | #317 |
| 028 | [[028-pjs-demo-workspace-build/spec\|Restore Buildability of `pjs-demo` Demo Servers]] | implemented | P2 | #318 |
| 030 | [[030-wasm-loader-esm-cjs-mismatch/spec\|Fix WasmBackend/WasmParser Node.js Initialization (ESM/CJS mismatch after #351)]] | implemented | P1 | #383 |
| 033 | [[033-wasm-security-config-shared-reuse/spec\|SecurityConfig Shareable Between PjsParser and PriorityStream in pjs-wasm]] | implemented | P1 | #404 |
| 070 | [[070-wasm-bindings-api/spec\|pjs-wasm public binding API]] | implemented | P2 | #242 |
| 071 | [[071-wasm-security-limits/spec\|pjs-wasm security limits]] | implemented | P1 | #326 |
| 072 | [[072-wasm-packaging-targets/spec\|pjs-wasm build targets and npm package]] | implemented | P2 | #344 |
| 073 | [[073-js-client-core/spec\|TypeScript client core (client, transports, reconstruction)]] | partial | P2 | #178 |
| 074 | [[074-js-client-wasm-integration/spec\|TypeScript client WASM integration]] | implemented | P2 | #383 |
| 075 | [[075-bench-suite/spec\|pjs-bench benchmark suite]] | implemented | P2 | #514 |
| 076 | [[076-fuzz-targets/spec\|cargo-fuzz targets and CI replay]] | implemented | P2 | #575 |
| 077 | [[077-demo-servers/spec\|pjs-demo servers, datasets and tests]] | implemented | P3 | #406 |
| 078 | [[078-ci-pipeline/spec\|CI pipeline and supply-chain gates]] | implemented | P1 |  |
| 079 | [[079-release-pipeline/spec\|release and publish pipeline]] | implemented | P1 |  |

## Backlog and defect specs

Specs whose work is not complete (`draft` or `partial`); see each Verification callout for remaining requirements.

| ID | Spec | Status |
|----|------|--------|
| 004 | [[004-partial-json-streaming/spec\|Partial JSON Parsing for Streaming Frames]] | partial |
| 007 | [[007-http-auth-middleware/spec\|HTTP Authentication Middleware for PJS Endpoints]] | partial |
| 008 | [[008-cursor-pagination-active-sessions/spec\|Pagination for GetActiveSessionsQuery]] | partial |
| 009 | [[009-realtime-metrics-observability/spec\|Real-time Metrics and Observability]] | partial |
| 010 | [[010-wasm-typescript-type-generation/spec\|Typed TypeScript Definitions for pjs-wasm via tsify]] | partial |
| 011 | [[011-zstd-dictionary-compression/spec\|Zstd Trained-Dictionary Compression for Frame Transport]] | partial |
| 016 | [[016-async-stream-test-harness/spec\|Async Stream Test Harness]] | partial |
| 018 | [[018-config-validation-startup/spec\|PjsConfig Startup Validation]] | partial |
| 037 | [[037-graceful-shutdown-serve-with-limits/spec\|Graceful Shutdown for serve_with_limits]] | draft |
| 039 | [[039-partial-json-jiter-parser/spec\|Partial JSON Parser (`partial-parse` feature)]] | partial |
| 045 | [[045-priority-stream-planning/spec\|Priority Stream Planning (skeleton + patches)]] | partial |
| 049 | [[049-secure-byte-compression/spec\|Secure Byte Compression (Layer B)]] | partial |
| 051 | [[051-security-limits-and-validated-config/spec\|Security Limits and Validated Configuration]] | partial |
| 054 | [[054-domain-services/spec\|Domain Services]] | partial |
| 058 | [[058-event-metrics-dictionary-adapters/spec\|Event Publisher, Metrics Collector and Dictionary Store Adapters]] | partial |
| 059 | [[059-crate-api-errors-features/spec\|Crate Public API Surface, `pjson_rs::Error` and Feature-Flag Matrix]] | partial |
| 061 | [[061-http-middleware-stack/spec\|HTTP middleware stack: layers, rate limiting, auth, security headers]] | partial |
| 064 | [[064-http-axum-extension/spec\|`PjsExtension`: bolt-on PJS routes for existing axum apps]] | partial |
| 065 | [[065-http-prometheus-metrics/spec\|Prometheus `/metrics` endpoint]] | partial |
| 066 | [[066-websocket-server-transport/spec\|WebSocket server transport: upgrade, ownership, caps and TTL]] | partial |
| 068 | [[068-websocket-client/spec\|`PjsWebSocketClient`: client sessions, caps and TTL]] | partial |
| 073 | [[073-js-client-core/spec\|TypeScript client core (client, transports, reconstruction)]] | partial |
