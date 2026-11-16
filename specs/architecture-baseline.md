---
aliases:
  - Architecture Baseline
  - Design Patterns Inventory
tags:
  - sdd
  - architecture
  - baseline
  - pjs
created: 2026-05-01
last-verified: 2026-11-15
verified-head: dcf9b11
status: living
audience: contributors and agents working on PJS
related:
  - "[[constitution]]"
  - "[[testing-strategy]]"
  - "[[MOC-specs]]"
  - "[[decisions/adr-001-allocator-decision|ADR-001 allocator decision]]"
---

# Architecture Baseline

> [!info] Verification (2026-11-15)
> Migrated from an earlier untracked draft (originally verified 2026-05-01 at HEAD e996ad9, v0.5.2). Every pattern claim, drift item and file reference was re-checked against HEAD dcf9b11 (workspace v0.7.0). Statuses below are current; file references are by path and symbol, not line number.
>
> **Drift of the original baseline from the code (what changed since 2026-05-01):**
> - Resolved: D1 (`tokio` in domain, `ConnectionManager` deleted, #254), D2/D3 (`get_stream_mut` removed, #259; `streams()` now returns the invariant-keeping `StreamMap`, #577), D5 (`Parser { use_sonic }` and `hybrid.rs` gone, #126/#495), D6 (single `JsonPath`), D9 (doc-link CI gate exists), D14 (`cargo deny` + OSV run in CI).
> - Removed from the code: `PriorityService`, `ConnectionManager`, `AdaptiveFrameStream`/`PriorityFrameStream`, `infrastructure/integration/*` (`UniversalAdapter*` builders), `memory/` module, `AdjustPriorityThresholdCommand`, `GetSessionEventsQuery`, `GetStreamEventsQuery`, `Parser`/`parser/hybrid.rs`/`sonic.rs`/`simple.rs`.
> - New patterns: `StreamMap` keyed-by-own-id collection, `DepthGuard` RAII recursion guard, `Validated<T>` typestate config wrapper, `*_atomic` repository methods, WebSocket session ownership by `ConnectionId`, `#![forbid(unsafe_code)]` in `pjs-domain`, `fuzz/` workspace.
> - New leaks and gaps found: see D15 to D24 in [[#17. Drift registry]].

This document inventories the design patterns the codebase actually applies, so future changes do not drift. It is a baseline, not an aspiration: when adding code, follow the relevant section or document a deliberate deviation in the PR. Principles that govern all work live in [[constitution]]; test infrastructure is in [[testing-strategy]].

Each pattern carries a **status** (Compliant, Partial, Drift) and an **enforcement** (compile-time, test, CI, convention).

---

## 1. Hexagonal / Clean Architecture

**Status:** Compliant on the domain side; one outward leak in `application/` (D15) and one in `domain/ports/` (D16).
**Enforcement:** crate split plus module imports; no automated import-direction check exists (convention only).

### Layer split

| Layer | Crate / path | Allowed dependencies |
|---|---|---|
| Domain (pure) | `crates/pjs-domain/` (`pjson-rs-domain`) | `chrono`, `serde`, `serde_json`, `smallvec`, `thiserror`, `uuid` (see its `Cargo.toml`); `[lints.rust] unsafe_code = "forbid"` |
| Domain (extended) | `crates/pjs-core/src/domain/` | re-exports `pjs-domain` (`pub use pjson_rs_domain::{DomainError, DomainResult, entities, events, value_objects}`), adds `aggregates/`, `config/`, `ports/`, `services/`, `macros.rs` |
| Application | `crates/pjs-core/src/application/` | domain; defines `commands/`, `queries/`, `handlers/`, `dto/`, `shared.rs` |
| Infrastructure | `crates/pjs-core/src/infrastructure/` | domain ports plus `axum`, `tokio`, `tokio-tungstenite`, `subtle`, etc.: `adapters/`, `http/`, `websocket/`, `repositories/`, `bounded_channel.rs`, `schema_repository.rs` |
| Cross-cutting | `compression/`, `parser/`, `stream/`, `security/`, `config/`, `error.rs`, `global_alloc.rs` | used by application and infrastructure |
| Edge crates | `pjs-wasm` (depends only on `pjson-rs-domain`, no features), `pjs-demo`, `pjs-bench`, `pjs-js-client` | |

### Dependency direction (verified 2026-11-15)

- `rg` over `crates/pjs-domain/src` finds no `tokio`, `axum`, `sonic`, `brotli`, `flate`, `jiter`, `reqwest`, `hyper` imports (only substring hits in doc comments).
- `crates/pjs-core/src/domain/` contains `use tokio::` only inside `#[cfg(test)]` modules (`services/gat_orchestrator.rs`, `ports/gat.rs`, `ports/repositories.rs`). The former production leak (`ConnectionManager` with `tokio::sync::RwLock`) was deleted with the orphaned `PriorityService`/`TimeoutMonitor` chain in #254 (closes #251).
- `pjs-domain` is `std` only: the unsupported `no_std` mode and the `std` and `wasm` features were removed (#606).

### Known leaks

| # | Leak | Location | Status |
|---|---|---|---|
| L1 | `tokio::sync::RwLock` in `domain/services/connection_manager.rs` | removed | Resolved (#254) |
| L2 | `application/handlers/command_handlers.rs` imports `crate::infrastructure::adapters::InMemoryFrameStore` as the default type parameter `F` of `SessionCommandHandler` and constructs it in `SessionCommandHandler::new` / `with_dictionary_store` | `application/handlers/command_handlers.rs` | Open (D15) |
| L3 | `domain/ports/dictionary_store.rs` used `crate::compression::zstd::ZstdDictionary` and `crate::Result`; it now uses `TrainedDictionary` (in `pjs-domain`) and `DomainResult` | `domain/ports/dictionary_store.rs` | Resolved (D16, #605) |

### Rule for new code

A file under `domain/` MUST NOT contain `use tokio::`, `use axum::`, `use sonic_rs::` outside `#[cfg(test)]`. `application/` may depend on `domain/ports/` only, never on a concrete adapter. New ports go in `domain/ports/`; new adapters in `infrastructure/`.

---

## 2. DDD Tactical Patterns

### 2.1 Value objects

**Status:** Compliant. Immutable, validated at construction, equal by value.

| Type | Path | Invariant and enforcement |
|---|---|---|
| `Priority(NonZeroU8)` | `pjs-domain/src/value_objects/priority.rs` | non-zero via `NonZeroU8`; `Priority::new(u8) -> DomainResult<Self>`; constants `CRITICAL`(100), `HIGH`(80), `MEDIUM`(50), `LOW`(25), `BACKGROUND`(10) built by private `const fn new_const` that fails const evaluation on zero. No `unsafe` (crate forbids it) |
| `JsonPath { segments: Vec<PathSegment> }` | `value_objects/json_path.rs` | segmented path (`PathSegment::Key(String) \| Index(usize)`, `#[non_exhaustive]`); every constructor (`new`, `from_segments`, `root`) validates keys so `Display`/`FromStr` round-trip is unambiguous |
| `JsonData` | `value_objects/json_data.rs` | enum of JSON types; `JsonData::float(f64) -> DomainResult` rejects NaN/Infinity (RFC 8259 s6); deserialization bounded by `MAX_DESERIALIZE_DEPTH = 64` |
| `Id<T: IdMarker>` | `value_objects/id.rs` | UUID v4 plus `PhantomData<T>`; `IdMarker` is sealed (`private::Sealed`); `SessionId = Id<SessionMarker>`, `StreamId = Id<StreamMarker>` are distinct compile-time types |
| `SchemaId(String)` | `value_objects/schema.rs` | standalone newtype (no longer `Id<SchemaMarker>`; no `SchemaMarker` exists) |
| `Schema` | `value_objects/schema.rs` | recursive enum; `Deserialize` bounded by `enter_deserialize_depth` (#475) |
| `BackpressureSignal` | `value_objects/backpressure.rs` | enum |
| `DepthGuard` + `enter_deserialize_depth` | `value_objects/depth_guard.rs` | RAII per-thread recursion counter shared by `Schema` here and `SchemaDefinitionDto` in `pjs-core` (`application/dto/schema_dto.rs`); `#[must_use]` guard decrements on drop, including on error/unwind |

### 2.2 Entities

**Status:** Compliant.

| Entity | Path | Identity | State |
|---|---|---|---|
| `Frame` | `pjs-domain/src/entities/frame.rs` | `(stream_id, sequence)` | immutable after construction; `FramePatch` models changes |
| `Stream` | `entities/stream.rs` | `StreamId` (private field, `id()` accessor) | `StreamState`: `Preparing -> Streaming -> Completed \| Failed \| Cancelled`; transitions are methods returning `DomainResult` |
| `StreamMap` | `entities/stream_map.rs` | key is always `stream.id()` | `insert` derives the key from the stream; `Deserialize` rejects entries whose key differs from the stream id; wire format is a map keyed by UUID string |

All `Stream` fields are private. `StreamMap::get_mut` is `pub` and can in principle be used to overwrite a whole value (documented caveat; only the aggregate should expose it). `Stream::generate_skeleton` is still a "Simplified" private helper reachable only through `create_skeleton_frame`, which has no production caller (tests only) — D12, latent.

### 2.3 Aggregate root

**Status:** Compliant (previously Partial). Root: `StreamSession` in `crates/pjs-core/src/domain/aggregates/stream_session.rs`.

- All fields private; children held in `streams: StreamMap`; `pending_events: VecDeque<DomainEvent>`; injectable `TimeProvider` (`with_time_provider`, port `TimeProvider` in `domain/ports/mod.rs`).
- Invariants enforced in the root: `create_stream` checks `is_active()` and `SessionConfig::max_concurrent_streams`; `activate`, `close`, `force_close_expired`, `extend_timeout` enforce the session state machine `Initializing -> Active -> Closing -> Completed | Failed`; `SessionStats` updated inside aggregate methods.
- Child mutation goes through the root: `update_stream_config`, `create_stream_with_config`, `create_stream_patch_frames`, `commit_patch_frames_for_stream`, `create_priority_frames`, `commit_priority_frames`, `start_stream`, `complete_stream`, `fail_stream`. Each bumps `updated_at` and raises events.
- Reads: `streams() -> &StreamMap`, `stream(id) -> Option<&Stream>`. The former `get_stream_mut` was removed (#259).
- Events are raised only via private `add_event`, drained by `take_events()`.

**Rule for new code:** a new child mutation is a method on `StreamSession`, not a mutable accessor.

### 2.4 Domain events

**Status:** Compliant. `DomainEvent` (`pjs-domain/src/events/mod.rs`) is a tagged enum (`#[serde(tag = "event_type", rename_all = "snake_case")]`; adding a variant changes the wire format). Variants cover session lifecycle (`SessionActivated/Closed/Expired/TimedOut/TimeoutExtended`), stream lifecycle (`StreamCreated/Started/Completed/Failed/Cancelled`, `StreamPaused/Resumed`), frame production (`SkeletonGenerated`, `PatchFramesGenerated`, `FramesBatched`), and operational events (`PriorityThresholdAdjusted`, `StreamConfigUpdated`, `PerformanceMetricsRecorded`, `BackpressureReceived`, `CreditsUpdated`).

Events leave the application layer only through `EventPublisherGat::publish_batch`. The handler helper `save_and_publish` drains events **before** persisting the cloned session (so the stored copy has an empty buffer; #466/#467); events are dropped, not retried, if the save fails.

### 2.5 Repositories and ports

**Status:** Mostly compliant; one deliberate asymmetry (D4).

| Port | Granularity | Path |
|---|---|---|
| `StreamRepositoryGat` | aggregate (`StreamSession`); includes `*_atomic` per-session read-modify-write methods (#457/#458, #472) | `domain/ports/gat.rs` |
| `StreamStoreGat` | entity (`Stream`), bypasses the aggregate; used by HTTP handlers | `domain/ports/gat.rs` |
| `FrameStoreGat` | frame history per stream (production impl `InMemoryFrameStore`) | `domain/ports/gat.rs` |
| `EventPublisherGat` | `DomainEvent` batches | `domain/ports/gat.rs` |
| `EventStoreGat` | event persistence port with **no implementation** other than tests (D19) | `domain/ports/gat.rs` |
| `DictionaryStore` | per-session zstd dictionary; **non-GAT** by design (`Pin<Box<dyn Future>>`), `NoopDictionaryStore` default | `domain/ports/dictionary_store.rs` |

`StreamStoreGat` is a documented trade-off: HTTP handlers read/write single streams without loading the session. Prefer `StreamRepositoryGat` plus aggregate methods when session invariants matter.

### 2.6 Domain services

**Status:** Compliant. `pjs-core/src/domain/services/`: `GatStreamingOrchestrator` (+ `GatOrchestratorFactory`, `OrchestratorConfig`, `HealthStatus`) in `gat_orchestrator.rs`, and `ValidationService` (schema validation with a per-instance regex cache capped at 64 entries). The priority heuristic lives in `pjs-domain/src/services/priority.rs` (`compute_priority`, `PriorityHeuristicConfig`) and is shared with WASM. `PriorityService` and `ConnectionManager` no longer exist.

### 2.7 Strategic DDD

Not applied: single bounded context. Reconsider only if PJS becomes multi-protocol or multi-tenant.

---

## 3. CQRS (application layer)

**Status:** Compliant.

| Item | Count | Path |
|---|---|---|
| Commands | 7 | `application/commands/mod.rs`: `CreateSessionCommand`, `CreateStreamCommand`, `StartStreamCommand`, `GenerateFramesCommand`, `CompleteStreamCommand`, `CloseSessionCommand`, `BatchGenerateFramesCommand` |
| Queries | 9 | `application/queries/mod.rs`: `GetSessionQuery`, `GetActiveSessionsQuery`, `GetStreamQuery`, `GetStreamsForSessionQuery`, `GetSessionHealthQuery`, `GetStreamFramesQuery`, `GetSessionStatsQuery`, `GetSystemStatsQuery`, `SearchSessionsQuery` |
| Command handler | 1 struct `SessionCommandHandler<R, P, F>` implementing `CommandHandlerGat<C>` for each command, plus `CommandValidator` | `application/handlers/command_handlers.rs` |
| Query handlers | 3 structs: `SessionQueryHandler`, `StreamQueryHandler`, `SystemQueryHandler` implementing `QueryHandlerGat<Q>` | `application/handlers/query_handlers.rs` |
| DTOs | `IdDto`/`SessionIdDto`/`StreamIdDto`, `PriorityDto`, schema DTOs | `application/dto/` |

### Handler pattern

Mutating commands for an existing session use the repository's atomic read-modify-write (`create_stream_atomic`, `start_stream_atomic`, `complete_stream_atomic`, `close_session_atomic`, `batch_generate_frames_atomic`, ...), which return the raised events; the handler then calls `event_publisher.publish_batch(events)`. `CreateSessionCommand` and other paths that persist a whole session use `save_and_publish` (drain events, save a clone, publish). The application layer orchestrates; domain rules stay in the aggregate. Frames accepted by the handler are also appended to `FrameStoreGat` and fed to `DictionaryStore::train_if_ready` (feature `compression`).

`CreateStreamCommand` carries `source_data: JsonData` directly (the old `JsonDataDto` conversion no longer exists).

### Rule for new code

- New mutating use case: a command plus a `CommandHandlerGat<C>` impl; keep the read-modify-write atomic in the repository when it touches an existing session.
- New read use case: a query plus a `QueryHandlerGat<Q>` impl.
- DTOs stay in `application/dto/`; HTTP request/response types stay in `infrastructure/http/`.

---

## 4. GAT-based async ports

**Status:** Compliant. The project's signature pattern (requires `#![feature(impl_trait_in_assoc_type)]`).

`gat_port!` (`domain/macros.rs`, re-exported at crate level, uses the `pastey` crate via `$crate::__pastey`) turns `async fn` declarations into a `Send + Sync` trait with one `XxxFuture<'a>: Future<Output = DomainResult<T>> + Send + 'a` associated type per method, avoiding `async_trait` boxing.

Where applied (`domain/ports/gat.rs`): 15 port traits. Ten are declared through `gat_port!` (`FrameSourceGat`, `FrameSinkGat`, `StreamRepositoryGat`, `FrameStoreGat`, `StreamStoreGat`, `EventPublisherGat`, `SessionMetricsGat`, `FrameRepositoryGat`, `EventStoreGat`, `FrameWriterGat`); five are hand-written GAT traits (`MetricsCollectorGat`, `SessionTransactionGat`, `CacheGat`, `WriterFactoryGat`, `ConnectionMonitorGat`); `FrameSinkGatExt` is an extension trait. Application handlers implement `CommandHandlerGat`/`QueryHandlerGat` with `impl Future` associated types.

The 1.82x claim (CLAUDE.md) is attributed to removing `async_trait` virtual dispatch; it is independent of the allocator ([[decisions/adr-001-allocator-decision]]).

### Deliberate exception

`DictionaryStore` returns `DictionaryFuture<'a, T> = Pin<Box<dyn Future<Output = DomainResult<T>> + Send + 'a>>` because it is cold-path (once per dictionary request or sample batch) and used as `Arc<dyn DictionaryStore>`. The port is not feature-gated and uses `TrainedDictionary` (#605); only the `InMemoryDictionaryStore` adapter needs `compression`.

**Rule:** hot-path port (per frame/decode/event) uses `gat_port!`; cold-path port may box, with the choice documented.

---

## 5. Type-driven design

**Status:** Compliant for security-critical types; gaps noted.

| Type | Gate | Invariant |
|---|---|---|
| `ZstdDictionary(Vec<u8>)` | private `new_checked` (`compression/zstd.rs`) | non-empty, `<= MAX_DICT_SIZE` (112 KiB), magic `0xEC30A437`; documented as a type invariant |
| `Priority`, `JsonPath`, `JsonData::float` | see 2.1 | |
| `Validated<T>` / `ValidatedSecurityConfig` / `ValidatedCompressionConfig` (`config.rs`) | sealed `Validate` trait; only `Validate::validated()` yields a `Validated<T>` | proof that `validate()` passed. `SecurityConfig::validate` rejects zero and inconsistent limits (#614) |
| `MaxDepth(usize)` (`config/security.rs`) | `From<&ValidatedSecurityConfig>` | depth derived from validated config |
| `ConnectionId(Uuid)` (`infrastructure/websocket/mod.rs`) | newtype | WebSocket session owner tag |

Other enforced patterns: sealed `PartialJsonParser` (`parser/partial.rs`, feature `partial-parse`) so only `pjs-core` implements it; sealed `IdMarker`; `Id<Marker>` brands. State machines (`SessionState`, `StreamState`, `FrameType`) are runtime enums, not typestate, by choice.

**Known gaps:** `pjson_rs::Error` is stringly typed (open #599); `StreamFilter`, `SessionQueryCriteria` and `Pagination` validate at construction (#604) and `SessionConfig.session_timeout` is a `SessionTimeout` (#596); `SessionConfig`/`SessionStats` are otherwise plain structs with public fields.

**Rule for new types with invariants:** private fields, a single `new`/`new_checked`/`try_from` gate, the invariant stated in the type's doc comment. `unsafe` constructors are not allowed in `pjs-domain`.

---

## 6. Strategy / runtime polymorphism

**Status:** Compliant. Sum-type strategies: `ByteCodec` (`compression/secure.rs`: `None`, `Deflate`, `Gzip`, `Brotli`, `ZstdDict(Arc<ZstdDictionary>)`; no longer `Copy`) and `CompressionStrategy` (Layer A). The old bool-flag `Parser { use_sonic }` strategy and `parser/hybrid.rs` are gone (#126, #495); the parser surface is now `ZeroCopyParser`/`LazyParser`/`IncrementalParser`, the optional `JiterPartialParser`, and `parser/simd`. `sonic-rs` remains a non-optional dependency of `pjson-rs`; production use is serialization of frames in `infrastructure/http/streaming.rs` (`sonic_rs::to_vec`, `StreamTransportError::Serialization`), and `build.rs` configures its SIMD codegen via the `simd-*` features.

**Rule:** a new backend or codec extends an enum with an exhaustive `match`, not stacked booleans.

---

## 7. Builder pattern

**Status:** Rarely needed now. The previous builders (`UniversalAdapterBuilder`, `PooledResponseBuilder`, `PooledSSEBuilder`, `AdaptiveFrameStream::with_*`) were deleted with `infrastructure/integration/` and the dead streams (#487, #507, #521). Remaining construction idioms: `BatchFrameStream::new(stream, format, batch_size)` plus `into_stream()` (`infrastructure/http/streaming.rs`), config presets (`SecurityConfig::high_throughput/low_memory/development`), and router factory functions (`create_pjs_router*`, `apply_common_layers`). Use a builder when construction has four or more optional parameters; otherwise a plain constructor.

---

## 8. Defense in depth (security)

**Status:** Compliant.

1. Every `ByteCodec` decoder is wrapped by `CompressionBombProtector` through the `run!` macro in `SecureCompressor` decompression (`compression/secure.rs`); bulk decoders that bypass byte counting are not used there.
2. `ZstdDictionary::new_checked` type gate.
3. `InMemoryDictionaryStore` runs the bomb-detector size gate on `register()` and again after training inside the `spawn_blocking` closure (`infrastructure/repositories/dictionary_store.rs`), deliberately symmetric.
4. API-key comparison uses `subtle::ConstantTimeEq` over the full key list with no early return (`infrastructure/http/auth.rs`; HMAC-SHA256 tags; `http-auth-jwt` adds `JwtAuthLayer`).
5. `create_pjs_router_with_rate_limit_and_auth` places rate limiting outside auth, so unauthenticated requests still consume budget; `/pjs/health` stays unauthenticated.
6. Depth/size bounds: `SecurityConfig` (`config/security.rs`, `MAX_SAFE_JSON_DEPTH` ceiling), `SecurityValidator::validate_json_depth_bytes` (string-aware), `DepthTracker`, schema/DTO recursion guards, `serve_with_limits` connection limits (`infrastructure/http/serve.rs`), WebSocket origin allow-list and rate limits (`infrastructure/websocket/security.rs`).
7. CORS origins validated at router build time (`build_cors_layer` returns `PjsError::HttpError`).

`ZeroCopyParser` enforces the array, object-key and string limits (#613) and `ConnectionLimits.max_header_bytes` caps request headers (#618); several other `SecurityConfig` fields remain unread.

**Rule:** untrusted decoding goes through a budget gate; structured inputs fail at type construction; credential comparison is constant-time; document the threat model in the module doc.

---

## 9. Layered compression

**Status:** Compliant. Layer A `compression/mod.rs` (`SchemaCompressor`, `CompressionStrategy`, operates on `serde_json::Value`) is orthogonal to Layer B `compression/secure.rs` (`SecureCompressor`, `ByteCodec`, operates on `&[u8]`). `SecureCompressedData` is in-process only, not a wire format. Streaming integration is in `stream/compression_integration.rs`; trained dictionaries in `compression/zstd.rs`.

---

## 10. Error handling

**Status:** Compliant, with a stringly-typed top-level error (#599).

| Crate | Type | Path |
|---|---|---|
| `pjs-domain` | `DomainError` (20 variants, `#[non_exhaustive]`, `thiserror`) | `pjs-domain/src/lib.rs` |
| `pjs-core` application | `ApplicationError`: `Domain`, `Validation`, `Authorization`, `NotFound`, `Concurrency`, `Conflict`, `Logic` | `application/mod.rs` |
| `pjs-core` top level | `Error` / `Result` | `error.rs` |
| `pjs-core` HTTP | `PjsError` (implements `IntoResponse`) | `infrastructure/http/axum_adapter.rs` |
| others | `ConfigError`, `CompressionBombError`, `RateLimitError`, `StreamTransportError`, `StreamExtensionError`, `CreateSessionError`, `AuthConfigError`, `TrySendError` | per module |

`PjsError::into_response` maps `ApplicationError` variants: `NotFound` 404, `Validation` 400, `Authorization` 401, `Concurrency`/`Conflict` 409, `Domain` 500 except `DomainError::InvalidInput` 400 (#621), `Logic` 500 (closes #173); parse errors in the HTTP layer map to 400. **Rule:** classify new errors in that mapping; do not default to 500 for a classifiable error.

---

## 11. Async runtime discipline

**Status:** Compliant on hot paths.

- Hot-path traits are GAT (section 4); tokio primitives appear only in `infrastructure/` (and test modules).
- CPU-bound zstd training runs in `tokio::task::spawn_blocking` (`infrastructure/repositories/dictionary_store.rs`).
- Streaming bodies use `async_stream::try_stream!` (`BatchFrameStream::into_stream`). A hand-written `poll_next` remains only inside a test harness (`PendingThenReady` in `infrastructure/http/streaming.rs`, used to catch waker-contract bugs, #166/#168).
- WebSocket frames go through a per-connection bounded outgoing channel owned by the controller, so a slow connection stalls only its own sessions (#570).

**Rule:** new `Stream` impls use `async_stream`; CPU-bound work over about 1 ms goes through `spawn_blocking`.

---

## 12. Configuration and feature flags

**Status:** Partial. `PjsConfig` and `StreamingConfig`/`ParserConfig`/`SimdConfig` expose `validate() -> Result<(), ConfigError>` (`config.rs`); `SecurityConfig` and `CompressionConfig` implement the sealed `Validate` trait and can be wrapped in `Validated<T>`. `HttpServerConfig` has **no** `validate()`; its CORS origins are checked when the router is built. `SecurityConfig::validate` is too permissive (#614).

Features of `pjson-rs` (`crates/pjs-core/Cargo.toml`): default = `simd-auto`, `schema-validation`, `compression`, `http-server`, `websocket-server`, `websocket-client`. Optional: `simd-avx512` (enables `sonic-rs/avx512`), `mimalloc`, `partial-parse` (`jiter`), `http-auth-jwt`, `metrics`. `jemalloc` was removed ([[decisions/adr-001-allocator-decision]]). `mimalloc` and the system allocator are not mutually exclusive features: `mimalloc` is opt-in. The `websocket-client` feature does not compile alone (open #598); there is no per-feature CI matrix (open #615).

Gating idiom: `#[cfg(all(feature = "compression", not(target_arch = "wasm32")))]` for items that pull non-WASM dependencies (for example `ByteCodec::ZstdDict`).

**Drift:** the documented rule "annotate feature-gated public API with `#[cfg_attr(docsrs, doc(cfg(...)))]`" is not applied anywhere (no `doc(cfg` or `docsrs` in `crates/`) — D21.

---

## 13. Observability

**Status:** Compliant where present.

- `tracing` macros are used across security-relevant and transport code (over 100 call sites in `pjs-core/src`, concentrated in `infrastructure/websocket/`, `http/serve.rs`, `compression/secure.rs`).
- Feature `metrics`: process-global Prometheus recorder installed once via `OnceLock` (`infrastructure/http/metrics.rs`), `GET /metrics`, aggregate counter `pjs_frames_total` incremented in the `GenerateFrames`/`BatchGenerateFrames` handlers (HTTP throughput only, #239); no per-session labels.
- Real uptime: `PjsAppState::with_stores` captures `Instant::now()` and passes it to `SystemQueryHandler::with_start_time` (#142).
- `pjson_rs::global_allocator_name()` returns `"mimalloc"` or `"system"`.

**Rule:** emit a `tracing` event on security decisions; keep metric cardinality aggregate; use `criterion` benches (`crates/pjs-bench`) for hot-path measurement.

---

## 14. Release and versioning

**Status:** Compliant.

- Single version source `[workspace.package].version` (0.7.0); crates inherit it.
- Workspace: `members = ["crates/*"]`, `default-members = [pjs-core, pjs-domain, pjs-wasm]`, `exclude = ["crates/pjs-js-client", "fuzz"]`. `pjs-demo` and `pjs-bench` are members (not default); `fuzz/` is its own workspace.
- Published to crates.io: `pjson-rs`, `pjson-rs-domain` (`publish = true`, trusted publishing). `pjs-wasm` is `publish = false` on crates.io and ships to npm as `@pjson/wasm`; `pjs-js-client` ships as `@pjson/node` (`.github/workflows/release.yml`).
- `CHANGELOG.md` is Keep-a-Changelog; pre-1.0 breaking changes are allowed without deprecation but must be marked BREAKING and carry `!`/`BREAKING CHANGE:` in the commit.
- Conventional Commits per `.claude/rules/commits-and-issues.md`.

CI observations: the publish job installs nightly (#617) and dependabot proposes semver-major updates (#616).

---

## 15. Testing discipline

See [[testing-strategy]] for the verified test pyramid, CI gates and guard tests. Summary: nextest unit tests (CI runs `--lib --bins` per OS/allocator), integration tests in `crates/*/tests/` (CI runs them in the coverage job and in `release.yml`), doctests per OS, proptest in `pjs-domain`, fuzz targets in `fuzz/`, criterion benches (not in CI), WASM native tests in CI and `wasm_bindgen_test` tests run locally only, Jest tests for `pjs-js-client`.

**Rule:** new public API gets a unit test and a runnable doctest; a new port/adapter gets an integration test wiring the real adapter; a hot-path change records criterion before/after numbers in the PR.

---

## 16. Documentation discipline

**Status:** Compliant (previously Drift). The CI `docs` job runs `cargo doc --no-deps --all-features -p pjson-rs -p pjson-rs-domain` with `--deny rustdoc::broken_intra_doc_links --deny rustdoc::private_intra_doc_links --deny rustdoc::redundant_explicit_links` (#236, #237, #661). `pjs-core` and `pjs-domain` set `#![warn(missing_docs)]`. Required for every `pub` item: `///` doc (what and why), `# Examples` for non-trivial APIs, module `//!` docs, trait contracts. Exception: `doc(cfg)` annotations (D21).

Before a PR: `RUSTDOCFLAGS="--deny rustdoc::broken_intra_doc_links --deny rustdoc::private_intra_doc_links --deny rustdoc::redundant_explicit_links" cargo doc --no-deps -p pjson-rs -p pjson-rs-domain` and `cargo test --doc --all-features`.

---

## 17. Drift registry

| # | Drift | Location | Severity | Status (2026-11-15) |
|---|---|---|---|---|
| D1 | `tokio::sync::RwLock` in domain layer | `domain/services/connection_manager.rs` | Medium | **Resolved** (#254, file deleted) |
| D2 | Aggregate child mutably exposed (`get_stream_mut`) | `stream_session.rs` | Medium | **Resolved** (#259; `update_stream_config` added) |
| D3 | Aggregate internal collection exposed (`streams()`) | `stream_session.rs` | Low | **Mitigated**: returns `&StreamMap` (#577); `StreamMap::get_mut` still `pub` |
| D4 | Stream-level store sidesteps aggregate | `StreamStoreGat` vs `StreamRepositoryGat` | Low | Open, deliberate trade-off |
| D5 | Bool-flag parser strategy | `parser/mod.rs` | Low | **Resolved** (`Parser` removed, #495) |
| D6 | Two `JsonPath` types | domain vs `stream/priority.rs` | Low | **Resolved**: `stream/priority.rs` imports the domain `JsonPath` |
| D7 | `train_if_ready` unwired | | High | **Resolved** (#224/#230/#232; called from `train_from_frames`) |
| D8 | `Parser::parse_partial` unwired | | Low | **Changed**: `Parser` removed; `JiterPartialParser` (feature `partial-parse`) has no production caller outside `parser/` and `config/security.rs` |
| D9 | Doc gate not in CI | | Medium | **Resolved** (#236/#237) |
| D10 | `with_compression(true)` always errors | | Low | **Resolved**, type removed (#521) |
| D11 | WebSocket session model disjoint from `StreamSession` | `infrastructure/websocket/mod.rs` | Medium | Open, intentional; since #577 each `WebSocketStreamSession` has an `owner: ConnectionId` and frames/acks are routed by owner |
| D12 | `Stream::generate_skeleton` "Simplified" stub | `pjs-domain/src/entities/stream.rs` | Low | Open, latent (no production caller of `create_skeleton_frame`) |
| D13 | `StreamingOrchestrator` orphan | | | **Resolved** (#234) |
| D14 | `cargo deny`/audit not in CI | | Low | **Resolved** for deny and OSV (jobs `security-deny`, `security-osv`, PR/merge-queue/schedule only); `cargo audit` is not run in CI |
| D15 | `application/` depends on `infrastructure::adapters::InMemoryFrameStore` (default type param and constructors of `SessionCommandHandler`) | `application/handlers/command_handlers.rs` | Medium | **New, open**; no issue found by `gh issue list --search` |
| D16 | `domain/ports/dictionary_store.rs` imported `crate::compression::zstd` and `crate::Result` | `domain/ports/` | Low | **Resolved** (#605) |
| D17 | `pjs-domain` `no_std` build fails; docs mention a nonexistent `serde` feature | `pjs-domain` | Low | **Resolved** (mode and features removed, #606) |
| D18 | `SecurityConfig::validate` permissive; parser limits unenforced | `config/security.rs` | Medium | **Resolved** (#613, #614) |
| D19 | `EventStoreGat` has no production implementation | `domain/ports/gat.rs` | Low | Open (design stub; matches section 19) |
| D20 | A few non-test `unsafe` sites lack an adjacent SAFETY comment (for example `AlignedAllocator::realloc_aligned`/`dealloc_aligned` signatures, some `buffer_pool.rs` call sites); not exhaustively audited | `parser/aligned_alloc.rs`, `parser/buffer_pool.rs` | Low | **New, open** |
| D21 | No `#[cfg_attr(docsrs, doc(cfg(...)))]` on feature-gated API despite the rule | all crates | Low | **New, open** |
| D22 | `pjson_rs::Error` is stringly typed; `StreamFilter::validate` never called | `error.rs`, `domain/ports/repositories.rs` | Medium | `Error` open (#599); filter validation resolved (#604) |
| D23 | WebSocket transport never prioritizes; shared broadcast could drop other connections' frames | `infrastructure/websocket/` | Medium | Broadcast removed (#570); planner open (#624) |
| D24 | `CLAUDE.md` (project) still lists `jemalloc` feature and says `pjs-demo` is excluded from the workspace; CI comment says "Windows skips mimalloc" but the matrix runs 3 OS x 2 allocators | `.claude/CLAUDE.md`, `.github/workflows/ci.yml` | Low | **New, open** (docs/comments only) |

---

## 18. Quick reference

| When you touch | Read first |
|---|---|
| a domain entity or value object | 2.1, 2.2, 5 |
| the session aggregate | 2.3 |
| a use case | 3 |
| a port | 4 (hot path: `gat_port!`; cold path: boxed future plus doc) |
| decompression or untrusted input | 8 |
| an HTTP route or stream body | 10, 11 |
| config | 12 |
| a release | 14 |
| tests | [[testing-strategy]] |
| allocators | [[decisions/adr-001-allocator-decision]] |

## 19. Not applied (consciously)

- Strategic DDD (single bounded context).
- Event sourcing: events are emitted but not persisted; `EventStoreGat` has no production implementation.
- CQRS read-model projections: queries hit the same repository as commands.
- Strict "test through port" discipline: some tests use mocks bypassing ports.
- Compile-time typestate for Stream/Session state machines.
- The `async_trait` crate (avoided in favour of GAT; `DictionaryStore` uses a plain boxed future).

Escalate before adopting any of these.

## Maintenance

Update this document when a layer or crate is added, a pattern is adopted or removed, a drift item is resolved (move it out of section 17 into its section), or a verification claim breaks. Re-verify `last-verified` before relying on it for non-trivial decisions.
