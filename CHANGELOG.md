# Changelog

<!-- markdownlint-disable MD024 -->

All notable changes to the Priority JSON Streaming Protocol (PJS) project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- **BREAKING** `StreamSession::get_stream_mut` removed. Child-stream mutation now flows through two new aggregate-root methods: `update_stream_config(stream_id, config)` and `create_stream_patch_frames(stream_id, priority_threshold, max_frames)`. Both bump the session-level `updated_at` timestamp and raise the appropriate domain events (`StreamConfigUpdated`, `FramesBatched`); `create_stream_patch_frames` additionally maintains `stats.total_frames` so session-level metrics stay consistent with the per-stream mutation. Previously `command_handlers.rs` reached inside the aggregate via `get_stream_mut` to call `Stream::update_config` (silent: no event, no timestamp bump) and `Stream::create_patch_frames` (silent: stale `stats.total_frames`, no `FramesBatched` event). The removal closes the longest-standing entry (D2) in the architectural-baseline drift register. Pre-1.0 breaking change; no deprecation cycle (closes #259).

### Added

- `pjson_rs_domain::services::compute_priority` and `PriorityHeuristicConfig` — single source of truth for the priority heuristic shared by every transport. `Stream::compute_priority` (HTTP path) and `pjs_wasm::priority_assignment::PriorityAssigner` (WebAssembly path) now both delegate here, so the same `(path, value)` input yields the same `Priority` regardless of how it reaches the client (closes #242).
- Cross-engine parity tests in `crates/pjs-wasm/tests/priority_parity.rs` covering every divergence case from #242 (`state`/`error` were CRITICAL on HTTP but fell through on WASM; `description`/`message`/`content`/`body` similar; large-array penalty differed). The test asserts WASM and domain agree pointwise for the documented sample set so future drift fails CI loudly.
- CI job `docs` enforcing `RUSTDOCFLAGS=--deny rustdoc::broken_intra_doc_links` on `cargo doc --no-deps --all-features -p pjson-rs -p pjson-rs-domain`. Wired into the `ci-success` gate so broken intra-doc links now block merges instead of slipping through review (closes #236).

### Changed

- **BREAKING** WebAssembly priority assignment now matches the HTTP transport. `PriorityConfig::low_patterns` and `background_patterns` are still populated through `PriorityConfigBuilder.addLowPattern` / `addBackgroundPattern`, but matching is now exact (case-insensitive) on the last path key instead of substring-on-name. The legacy WASM-only branches — substring `stats`/`meta` → LOW, `obj.contains_key("timestamp")` → MEDIUM, large-array → BACKGROUND — are gone; payloads now flow through the domain heuristic (depth boost + value-shape penalty + override map) just like HTTP. Same `Priority` for the same payload across transports (closes #242).
- The previously private `pjs_wasm::priority_assignment` module is now `pub` so Rust integration tests (and downstream callers using the rlib) can verify cross-engine parity directly.
- **BREAKING** `pjson_rs::infrastructure::websocket::StreamSession` renamed to `WebSocketStreamSession` to remove the name collision with the domain-layer `crate::domain::aggregates::StreamSession`. The two types remain deliberately disjoint — the WebSocket controller maintains an ephemeral, transport-local session model that does not share state with `POST /pjs/sessions`. The new name and the type's doc comment make this explicit at the call site (closes #239).
- **BREAKING** `AdaptiveFrameStream::into_stream`, `BatchFrameStream::into_stream`, `PriorityFrameStream::into_stream`, `create_streaming_response`, and `create_streaming_response_with_content_type` now operate on `Vec<u8>` instead of `String`. Threading bytes end-to-end is what makes `AdaptiveFrameStream::with_compression(true)` actually usable — the previous `String` pipeline rejected gzip output (which is binary, not UTF-8) with `StreamError::Io("compressed output is not valid UTF-8")` for every chunk. Callers that need a textual view of an uncompressed frame can decode each payload with `std::str::from_utf8`. Pre-1.0 breaking change; no deprecation cycle (closes #226).

### Fixed

- **Security** — `AxumWebSocketTransport` now wires `WebSocketRateLimiter` through the entire connection lifecycle. `upgrade_handler` extracts the peer address via `ConnectInfo<SocketAddr>` and rejects upgrade floods with HTTP 429 before any WebSocket frames are exchanged; `handle_socket` acquires a `RateLimitGuard` per connection (rejecting with WebSocket close code 1008 if the per-IP connection budget is exhausted) and enforces `RateLimitConfig::max_messages_per_second` / `max_frame_size` on every inbound `Text`/`Binary` frame before it reaches `AdaptiveStreamController::create_session`. Closes the parallel of #224 for the WebSocket transport: rate-limit infrastructure (`WebSocketRateLimiter`, `RateLimitGuard`, `SecureWebSocketHandler::create_connection_guard`) was already implemented and unit-tested but never invoked from production code, leaving WebSocket connections without per-IP, per-connection, or per-message budget. Routers must now be served with `into_make_service_with_connect_info::<SocketAddr>()` so the peer address is available to the upgrade handler. New constructor `AxumWebSocketTransport::with_rate_limit_config(RateLimitConfig)` exposes the underlying configuration; `new()` continues to use `RateLimitConfig::default()`. The `frame_rx.recv()` arm in the per-connection `tokio::select!` now matches on the full `Result` so `RecvError::Lagged` is logged-and-skipped while `RecvError::Closed` ends the loop instead of busy-spinning the future. Wire-level regression test `test_wire_inbound_messages_rate_limited` confirms a flooding client receives a policy-violation close (closes #250).
- `AdaptiveFrameStream::with_compression(true)` now produces decompressible gzip payloads instead of failing for every non-trivial input. Added `test_adaptive_frame_stream_with_compression` and `test_adaptive_stream_with_compression_round_trips` that round-trip frames through `flate2::read::GzDecoder` and assert the gzip magic header (`1f 8b`).
- `cargo doc --deny rustdoc::broken_intra_doc_links` now passes: replaced `[ApiKeyAuthLayer]` with `[super::ApiKeyAuthLayer]` in `JwtAuthLayer` doc, dropped the link to private `build_cors_layer` in `create_pjs_router_with_config` doc, and wrapped `Id<T>` and `Box<dyn Trait>` in code spans in `id_dto.rs` and `gat.rs` (closes #225)
- `pjson_rs_domain::services::priority` module-level doc no longer links to private `crate::Stream::extract_patches`; updated to point at the public `Stream::create_patch_frames` entry point. CI doc-gate now also denies `rustdoc::private_intra_doc_links` so future regressions of this class are caught at PR time (closes #248).
- `GET /pjs/sessions/{session_id}/dictionary` is now reachable end-to-end. `SessionCommandHandler` accepts an `Arc<dyn DictionaryStore>` and feeds each accepted frame's serialized payload into the per-session training corpus from `GenerateFramesCommand` and `BatchGenerateFramesCommand` handlers, so the endpoint flips from `404 Not Found` to `200 OK` once `N_TRAIN` (32) frames have been generated (closes #224).
- `cargo deny check` no longer fails the licenses gate. Added `CDLA-Permissive-2.0` to the allow-list in `deny.toml`; required by `webpki-root-certs` (TLS root cert data bundled by `rustls-platform-verifier` via `reqwest`). The license is permissive (Community Data License Agreement) and compatible with the project's MIT/Apache-2.0 dual-licensing (closes #238).
- `pjson_rs::parser::JsonValue::parse_raw` no longer silently destroys input by replacing every `Raw(bytes)` with `JsonValue::Null`. The method now classifies the underlying bytes and constructs the matching typed variant (`Null`, `Bool`, `Number`, `String`, `Array`, `Object`) while preserving zero-copy semantics; invalid input, empty input, unterminated strings, and escaped strings (which cannot be represented in `JsonValue::String<&str>` without allocation) now return `Error::InvalidJson` instead of producing a misleading `Ok(Null)`. Replaces the existing `test_json_value_parse_raw` (which masked the bug because input `b"null"` and the placeholder both yielded `Null`) with round-trip coverage for every variant plus error cases (closes #243).
- `Stream::extract_patches` and `batch_patches_into_frames` are no longer stubs. `extract_patches` walks the source `JsonData` and emits one `FramePatch` per leaf value (primitives and arrays), pairing each with a priority computed from a field-name heuristic (`id`/`status` → CRITICAL, `name`/`title` → HIGH, etc.), the per-stream `priority_rules` override map, and depth/value-shape fallback rules; patches below the requested threshold are dropped. `batch_patches_into_frames` now sets each frame's priority to the maximum priority of the patches in its chunk instead of the placeholder `Priority::MEDIUM`. Together this means `GenerateFramesCommand` produces frames for non-empty source data, `train_if_ready` actually fires, and the chain `#224 → #230 → #232` delivers a reachable `GET /pjs/sessions/{id}/dictionary` end-to-end (closes #232).

### Added

- `POST /pjs/sessions/{session_id}/streams/{stream_id}/generate-frames` HTTP route dispatching `GenerateFramesCommand`. Body fields `priority_threshold: Option<u8>` (default `Priority::BACKGROUND` = 10) and `max_frames: Option<usize>` (default 16); response carries the produced `frames` array and a `frame_count` count. Closes the HTTP-layer gap that left `GenerateFramesCommand` and the dictionary-training corpus unreachable for HTTP-only clients (closes #230).
- `SessionCommandHandler::with_dictionary_store(repository, event_publisher, dictionary_store)` constructor; the existing `new` constructor defaults to `NoopDictionaryStore` (no behaviour change for callers that do not opt in to dictionary training).
- Regression tests in `application::handlers::command_handlers::tests::dictionary_wiring` verifying that the handler invokes `train_if_ready` for every accepted frame and that `N_TRAIN` frames produce a usable trained dictionary.
- End-to-end HTTP test `dictionary_endpoint_becomes_reachable_after_training` that drives `create-session → create-stream → start-stream → generate-frames` over the real Axum router, asserts the dictionary endpoint is `404` before training and `200 OK` (with a non-empty zstd dictionary body) after a single `generate-frames` call crosses the `N_TRAIN` threshold. The pre-existing smoke test now also asserts `frame_count > 0` instead of merely checking that the field is numeric.

### Changed

- `pjs-demo`: removed the unused `_dict_store` binding and the misleading "GET /pjs/sessions/{session_id}/dictionary" startup banner from `interactive-demo-server` — the demo never mounted the PJS router, so the binding was dead code and the printed endpoint was unreachable from this binary.

### Removed

- **BREAKING** Orphaned `AdjustPriorityThresholdCommand` from `crates/pjs-core/src/application/commands/mod.rs`. The struct had no `CommandHandlerGat` impl, no test, no example, no HTTP route, and no production caller — only the type definition existed. Same orphan-deletion precedent as #129, #233, #245, #251, #255, #257, #259, #262. Closes #265.
- **BREAKING** `pjson_rs::parser::SimdZeroCopyParser` and its companion `SimdZeroCopyConfig`, `SimdParseResult`, `SimdParsingStats` types (`crates/pjs-core/src/parser/simd_zero_copy.rs`, 589 LOC) plus the `parser_simd_zero_copy_comprehensive.rs` test suite (2047 LOC). The parser shipped four "SIMD validation methods" the source itself flagged as "simplified": `simd_validate_object_structure` and `simd_validate_array_structure` counted every `{`/`}`/`[`/`]` byte including those inside string literals (rejecting valid JSON whose strings contain a `}`); `simd_validate_number` accepted any string of digits/`.`/`-`/`+`/`e`/`E` regardless of order or count (so `+-123.456e789` and `..1` validated as numbers); and `simd_unescape_string` was a plain byte-loop labelled "SIMD-style". None of the methods used SIMD intrinsics — the struct name and the `parse_simd` entry point were misleading. No production code referenced the parser; the `Parser` façade in `parser/mod.rs` already routes through `SonicParser` (sonic-rs SIMD backend) when `cfg(pjs_simd)` is enabled. Same orphan-deletion precedent as #233, #245, #251, #255, #257. Closes #261.
- **BREAKING** Orphaned `crates/pjs-core/src/application/dto/event_dto.rs` (1006 LOC) and its `pub use event_dto::{DomainEventDto, EventIdDto, PerformanceMetricsDto, PriorityDistributionDto}` re-export from `application/dto/mod.rs`. The four DTOs (`DomainEventDto`, `EventIdDto`, `PerformanceMetricsDto`, `PriorityDistributionDto`) were the return-type backing for `EventQueryHandler` / `GetSessionEventsQuery` / `GetStreamEventsQuery` / `EventsResponse`, all deleted in PR #256 (closes #255). With those gone, only the file itself and the re-export referenced these types — no production code, no tests, no examples — and the public re-exports through `pjson_rs` had no downstream callers. Same scope-miss / focused-follow-up pattern as #257 (`SessionManager` orphaned by PR #254). Closes #262.
- **BREAKING** Orphaned `PriorityService` (`crates/pjs-core/src/domain/services/priority_service.rs`, 673 LOC) and its `pub use` re-exports from `domain/services/mod.rs` and `domain/mod.rs`. Superseded by `pjson_rs_domain::services::priority::compute_priority` (introduced in PR #246, closing #242), which is now the single source of truth used by both the HTTP path (`Stream::create_patch_frames`) and the WASM bindings. `PriorityService` had zero production callers. Closes #251.
- **BREAKING** Orphaned `ConnectionManager` (`crates/pjs-core/src/domain/services/connection_manager.rs`, 654 LOC) and `TimeoutMonitor` (`crates/pjs-core/src/infrastructure/services/timeout_monitor.rs`) chain together with the `tests/timeout_monitor_comprehensive.rs` integration suite (~600 LOC). `TimeoutMonitor` was the sole consumer of `ConnectionManager` and was itself only reachable through its `pub use` re-export — never instantiated from any HTTP/WS handler, command handler, or example. Removing the chain also retires the `tokio::sync::RwLock` import in the domain layer (architecture-baseline drift D1). Same precedent as #129, #233, #245. Closes #251.
- **BREAKING** Orphaned `EventQueryHandler` from `crates/pjs-core/src/application/handlers/query_handlers.rs` together with its `GetSessionEventsQuery`, `GetStreamEventsQuery`, and `EventsResponse` types from `crates/pjs-core/src/application/queries/mod.rs`. The handler had no production caller — `PjsAppState` never wired it up, the HTTP router has no `events` route, and only the handler's own creation test referenced it. Same orphan-pattern precedent as #129, #233, #245, and #251; deletion is the project's standard resolution for unwired modules. Closes #255.
- **BREAKING** Entire `crates/pjs-core/src/application/services/` directory (5 modules, 3172 LOC) and its 3 integration test files. `EventService`, `OptimizationService`, `PerformanceAnalysisService`, `PrioritizationService`, and `StreamContext`/`StreamSession`/`StreamConfig` from `stream_context` were never reached from production code — only the `tests/{event,optimization,prioritization}_service_comprehensive.rs` integration tests referenced them. Removal also eliminates the third `StreamSession` and the fourth `StreamConfig` name collision flagged in #239 / #241. Same precedent as #129 (orphaned `session_service.rs`) and #233 (orphaned `streaming_orchestrator.rs`); deletion is the project's standard resolution for orphaned modules. The `pub mod services;` declaration is removed from `application/mod.rs`. Closes #245.
- **BREAKING** Orphaned `SessionManager` (`crates/pjs-core/src/infrastructure/services/session_manager.rs`, 402 LOC) along with its `CleanupReport` and `SessionManagerConfig` companions — the fourth orphan from the #251 scope (added via the R46 follow-up comment) that PR #254 missed because the comment landed mid-flight. `SessionManager` had zero production callers; it was reachable only through its `pub use` re-export and inline tests, paralleling the GAT-backed session storage path (`StreamRepositoryGat` via `GatInMemoryStreamRepository`) that production actually uses. With this last orphan removed, `infrastructure/services/` is empty, so the whole directory plus the `pub mod services;` / `pub use services::*;` lines in `infrastructure/mod.rs` are gone. Same precedent as #129, #233, #245, #251. Closes #257.
- Orphaned `crates/pjs-core/src/domain/services/streaming_orchestrator.rs` (402 lines). The file was never declared as a module in `domain/services/mod.rs`, so cargo never compiled it; its functionality is fully covered by `GatStreamingOrchestrator::stream_session_with_priority` (closes #233).

## [0.5.2] - 2026-04-29

### Security

- `ApiKeyConfig` no longer derives `Debug`; a hand-written impl redacts `hmac_key` and `keys` fields, preventing HMAC key material from appearing in logs or panic output (closes #216)

### Added

- `GET /pjs/sessions/search` HTTP route dispatching to `SearchSessionsQuery`; supports `state`, `sort_by` (`created_at`, `updated_at`, `stream_count`, `total_bytes`), `sort_order` (`asc`/`ascending`, `desc`/`descending`), `limit`, and `offset` query parameters (closes #209)
- **`feature = "compression"`** — `ZstdDictCompressor` with per-session trained-dictionary compression (zstd 0.13, `zdict_builder`). Exposes `ZstdDictionary` newtype (type invariant: `len() ≤ 112 KiB`), `ZstdDictCompressor::train`, `compress`, and `decompress` (closes #144).
- `ByteCodec::ZstdDict(Arc<ZstdDictionary>)` variant in `SecureCompressor` — encode/decode arms route through the bomb-detector byte-counting `run!` macro, same as all other codecs.
- `DictionaryStore` trait and `NoopDictionaryStore` (zero-dep default) in `domain::ports` — hand-rolled `Pin<Box<dyn Future>>` port; no `async-trait` dependency.
- `InMemoryDictionaryStore` (behind `compression` + non-wasm32) — per-session corpus accumulation with `tokio::sync::OnceCell`-guarded one-time training. `register()` for pre-trained dictionaries; `train_if_ready()` for incremental corpus growth. Both call the bomb-detector as a size-budget gate.
- `GET /pjs/sessions/{session_id}/dictionary` HTTP endpoint (behind `http-server` + `compression` + non-wasm32) — returns the trained dictionary with `Content-Type: application/zstd-dictionary` and `Cache-Control: private, max-age=300` once `N_TRAIN` (32) frame samples are accumulated.
- `PjsAppState::with_dictionary_store(repo, publisher, store, dict_store)` — four-arg constructor that enables the dictionary endpoint end-to-end; existing `PjsAppState::new` defaults to `NoopDictionaryStore` (no behaviour change for existing callers).
- `pjs-demo`: interactive demo server now instantiates `InMemoryDictionaryStore` at startup and prints the dictionary endpoint path.
- 11 integration tests for `create_pjs_router_with_auth` and `create_pjs_router_with_rate_limit_and_auth` in `tests/http_middleware_tests.rs`: verify that `/pjs/health` is publicly accessible without credentials, protected routes return 401 without auth and 200 with a valid API key (both `X-PJS-API-Key` and `Authorization: Bearer` schemes), and that the rate-limit layer is correctly applied as outermost (closes #218)
- 24 integration tests in `tests/http_middleware_tests.rs` covering `ApiKeyAuthLayer` (auth pass/fail, OPTIONS bypass, multi-key), `AuthConfigError` construction validation, `RateLimitMiddleware` (budget enforcement, 429 with `Retry-After`), and `create_pjs_router` construction (closes #197)
- Serde round-trip tests for `Frame` covering all four frame types, all four patch operations, metadata, unicode, large payloads, timestamp precision, priority preservation, stream-ID preservation, and JSON field-name stability (`crates/pjs-domain/tests/frame_comprehensive.rs`)
- NaN/Infinity rejection tests for `JsonData::float` and round-trip serialization tests for finite float values (`crates/pjs-domain/tests/json_data_comprehensive.rs`)
- `pjson-rs`: new `partial-parse` feature flag; adds `jiter = "0.14"` workspace dependency and `parser/partial.rs` with the sealed `PartialJsonParser` trait, `PartialParseResult`, `StreamingHint`, `ParseDiagnostic` (`DuplicateKey`, `BigIntLossyConversion`), `JiterPartialParser` (hand-rolled per-token walker), and `JiterConfig`; foundation for partial JSON parsing in streaming frame delivery (#117)
- `pjs-wasm`: added `tsify-next` dependency; `FrameData` and `StreamStats` now derive `Tsify` and generate precise TypeScript interfaces in the wasm-pack `.d.ts` output; `FrameCallback`, `StreamStatsCallback`, and `ErrorCallback` type aliases are emitted via `typescript_custom_section` (closes #143)
- `PjsConfig::validate()` and sub-config validators (`StreamingConfig`, `ParserConfig`, `SimdConfig`, `SecurityConfig`) return `Err(ConfigError)` for zero-value fields and inconsistent bounds; `ConfigError` is re-exported from `pjson_rs` (closes #175)
- `ApiKeyAuthLayer` Tower middleware for `Authorization: Bearer` and `X-PJS-API-Key` authentication using HMAC-SHA256 tag comparison via `subtle::ConstantTimeEq` — constant-time, no key-index or length leakage (closes #135)
- `JwtAuthLayer` Tower middleware for JWT authentication, gated behind the `http-auth-jwt` feature flag using `jsonwebtoken`
- `create_pjs_router_with_auth` and `create_pjs_router_with_rate_limit_and_auth` factory functions; `/pjs/health` remains unauthenticated via nested router design
- `AuthConfigError` error type for `ApiKeyConfig` construction failures
- `PendingThenReady<I>` adversarial test harness and 5 new waker-contract tests using `tokio_test::block_on` to deterministically catch `poll_next` waker bugs (#168)
- CI job `js-client-test` runs `npm ci && npm test` for `crates/pjs-js-client` on push and JS file changes (#180)
- Wire-level WebSocket integration tests that perform real protocol upgrades, frame exchange, and connection close verification (closes #158)
- `AxumWebSocketTransport::active_connection_count` async method for observability of open connections
- `pjson_rs::global_allocator_name()` — returns `"mimalloc"` or `"system"` for diagnostics and benchmark reporting (#160)
- `mimalloc` feature now registers `mimalloc::MiMalloc` as the actual `#[global_allocator]` on non-wasm targets; previously it was dead extern-crate linkage with no effect (#160)
- New `crates/pjs-core/src/global_alloc` module owns the `#[global_allocator]` registration, separated from the aligned-buffer helpers (#160)
- Real deflate, gzip, and brotli compression/decompression in `SecureCompressor` via `flate2` (pure Rust) and `brotli` crates, gated on `feature = "compression"` (#114)
- `CompressionBombConfig::max_compressed_size` field to independently limit compressed input size before decoding (#114)
- `Error::CompressionError(String)` variant for codec-level failures, distinct from `SecurityError` (#114)
- `HttpServerConfig` struct with `allowed_origins: Vec<String>` for configurable CORS origins; `create_pjs_router_with_config` and `create_pjs_router_with_rate_limit_and_config` variants accept it — original signatures unchanged (#152)
- `metrics` Cargo feature: adds `metrics` + `metrics-exporter-prometheus` dependencies; installs a process-global Prometheus recorder via `OnceLock::get_or_try_init`; exposes `GET /metrics` endpoint in Prometheus text format (#142)
- `GET /pjs/stats` route backed by `SystemQueryHandler` with real wall-clock uptime and correct `frames_per_second`/`bytes_per_second` rates; `PjsAppState` stores `start_time: Instant` (#142)
- Aggregate frame counter `pjs_frames_total` (no per-session label) incremented in `GenerateFramesCommand` and `BatchGenerateFramesCommand` handlers when `metrics` feature is enabled (#142)

### Changed

- **BREAKING** `ByteCodec` no longer implements `Copy` — the new `ZstdDict(Arc<ZstdDictionary>)` variant requires `Clone`. Callers that relied on implicit copy semantics must call `.clone()` explicitly. Pre-1.0 breaking change; no deprecation cycle.

- `AuthConfigError::RngFailure` now wraps the underlying `getrandom::Error` instead of discarding it, providing operators with actionable diagnostic information when the system RNG fails in sandboxed environments (closes #203)
- Fixed inverted layer ordering diagram in `create_pjs_router_with_rate_limit_and_auth` doc comment; the diagram now correctly shows `rate_limit` as the outermost layer wrapping both public and protected sub-routers, with `auth` as an inner layer on protected routes only (closes #204)
- Extracted `public_routes`, `protected_routes`, and `apply_common_layers` helpers in `axum_adapter.rs` to eliminate route table duplication across router factory functions
- Added `ApiKeyAuthLayer` and related auth infrastructure behind `http-server` feature flag; `http-auth-jwt` feature gate added for optional JWT support
- **BREAKING** `JsonData::float(f64)` now returns `DomainResult<Self>` and rejects NaN and infinite values per RFC 8259 §6; the `From<f64> for JsonData` impl has been removed — callers must use `JsonData::float(value)?` to propagate the error. Closes #176.
- **BREAKING:** `jemalloc` feature removed along with all `tikv-jemalloc-*` workspace dependencies (`tikv-jemallocator`, `tikv-jemalloc-ctl`, `tikv-jemalloc-sys`). Use `mimalloc` (now a real `#[global_allocator]`) or the system allocator (#160)
- **BREAKING:** `parser::allocator::SimdAllocator` renamed to `parser::aligned_alloc::AlignedAllocator`; module `parser::allocator` is now `parser::aligned_alloc`. Per-backend FFI branches removed — all paths now route through the registered `#[global_allocator]` (#160)
- **BREAKING:** `AllocatorBackend` enum, `AllocatorStats` struct, `initialize_global_allocator()`, and `global_allocator()` removed. Use `global_allocator_name()` for diagnostics and `aligned_allocator()` for the buffer-pool accessor (#160)
- CI build and test matrices collapsed from 3 allocators (`system`, `jemalloc`, `mimalloc`) to 2 (`system`, `mimalloc`); Windows jemalloc exclusion removed; test jobs now use per-variant `features` instead of `--all-features` (#160)
- SIMD feature flags (`simd-auto`, `simd-avx2`, `simd-avx512`, `simd-sse42`, `simd-neon`) now activate sonic-rs SIMD codegen via `.cargo/config.toml` (`-C target-cpu=native` on x86_64/aarch64); `crates/pjs-core/build.rs` emits `pjs_simd_*` cfg gates and `cargo::warning` diagnostics when a SIMD feature is enabled but the required CPU target features are not exposed to rustc (#125)
- `SecureCompressor::new` and `with_default_security` now accept `ByteCodec` instead of `CompressionStrategy`; `CompressionStrategy` is Layer A (JSON-aware) and is unchanged (#114)
- `SecureCompressedData` gains a `codec: ByteCodec` field to identify which decoder to use on decompression (#114)
- `CompressionBombConfig::validate_pre_decompression` now checks `max_compressed_size` (not `max_decompressed_size`); the decompressed output is still monitored by `CompressionBombProtector` during streaming (#114)
- `CompressionBombConfig::max_ratio` default raised from 100.0 to 300.0 to accommodate legitimate brotli ratios on repetitive JSON (200x+ is normal) (#114)
- `CompressionBombConfig::high_throughput()` preset `max_ratio` raised to 1000.0 (#114)

### Fixed

- `axum_extension.rs` SSE handler no longer silently drops frames containing non-finite floats via `unwrap_or_default()`; serialization is now asserted infallible (invariant guaranteed by `JsonData::float` validation at construction). Closes #176.
- `GetActiveSessionsQuery` now routes through `find_sessions_by_criteria` with a bounded `Pagination` instead of the unbounded `find_active_sessions()` — eliminates the load-all-then-paginate allocation at large session counts (closes #136)
- `SearchSessionsQuery` enforces a maximum page size of 100 and correctly reports `has_more` in the response
- `SessionsResponse` gains a `has_more: bool` field indicating whether additional pages exist
- Stream adapters (`AdaptiveFrameStream`, `BatchFrameStream`, `PriorityFrameStream`) migrated from hand-rolled `poll_next` to `async-stream::try_stream!` to eliminate latent waker-contract bugs (#166). Consume the named builder via `.into_stream()` before `.collect()` / `.next()`.
- **BREAKING** (#167): `BatchFrameStream` with `StreamFormat::Json` now emits one JSON object per line per frame (NDJSON-of-objects), matching `StreamFormat::NdJson`'s wire format. Previously emitted one JSON array per batch. Pre-1.0 breaking change — no deprecation cycle. Consumers parsing each line should expect `serde_json::Value::Object`, not `Value::Array`.
- Fixed 6 broken intra-doc links in `global_alloc.rs`, `gat_memory_repository.rs`, and `auth.rs` that caused `RUSTDOCFLAGS="--deny rustdoc::broken_intra_doc_links" cargo doc` to fail; replaced unresolvable links with plain backtick text or qualified paths (closes #210)
- `GetActiveSessionsQuery` and `SearchSessionsQuery` tests now assert `has_more` correctness and the 100-item page cap (closes #208)
- nextest `default-filter` in `.config/nextest.toml` changed from `not test(integration)` (substring match on full test path) to `not test(/^integration_/)` (regex anchor on function name); restores 99 unit tests in `stream::compression_integration` and `infrastructure::integration` that were silently excluded (closes #200, resolves false 0% coverage in #195 and #196)
- TODO(CQ-004) comment block before `pub enum PjsError` in `axum_adapter.rs` converted from `///` to `//`; eliminates misattributed rustdoc and spurious ignored doctest on `PjsError` (closes #193)
- `pjs-wasm`: corrected module-level doc example in `lib.rs` — `PriorityStream.withSecurityConfig()` does not exist; replaced with `new PriorityStream()` + `stream.setSecurityConfig(security)` (closes #138)
- `pjs-wasm`: corrected `PriorityConfigBuilder` doc example — `.build()` is not a JavaScript-visible method; replaced with `PjsParser.withConfig(config)` usage pattern (closes #140)
- `PjsError::Application` now maps `ApplicationError` variants to semantically correct HTTP status codes: `NotFound` → 404, `Validation` → 400, `Authorization` → 401, `Concurrency`/`Conflict` → 409, `Logic`/`Domain` → 500 (closes #173)
- Renamed `infrastructure::http::PjsConfig` (HTTP extension config) to `HttpExtensionConfig` to eliminate name collision with the top-level `pjson_rs::PjsConfig` library config (closes #174)
- `StreamProcessor::process_frame` now returns `ProcessResult::Processed(frame)` immediately for each accepted frame; removed the dead `Incomplete` variant and the 64-frame buffer accumulation that made all frames appear incomplete (#181)
- `pjs-bench` benchmark crate restored as a workspace member — `cargo bench -p pjs-bench` now works; fixed pre-existing unused import, deprecated `criterion::black_box`, and `.clone()` on `Copy` type errors in bench sources (#179)
- `pjs-js-client`: align `JsonReconstructor` API — add `processSkeleton(frame)`, `applyPatch(patchFrame)`, `getCurrentState()`, `isComplete()`, `reset()` (#178)
- `pjs-js-client`: align `FrameProcessor` API — add `validateFrame(frame)` returning `{ isValid, errors }`, state-machine `processFrame(frame)`, `getStatistics()` (#178)
- `pjs-js-client`: fix `PJSClient` transport getter for Jest spy support; per-stream `JsonReconstructor` for concurrent isolation; `PatchApplied` fires once per frame (#178)
- `pjs-js-client`: prepend `baseUrl` to session URL in `HttpTransport.connect()` (#178)
- `pjs-js-client`: fix negative array index detection and long-string priority heuristic in utils (#178)
- `pjs-js-client`: WASM test suite skipped when `pjs-wasm/pkg` is absent (#178)
- `AdaptiveFrameStream::poll_next` now respects `buffer_size`: frames are prefetched into `current_buffer` and drained per-poll, enabling batched delivery (#163)
- `AdaptiveFrameStream::with_compression(true)` now applies `SecureCompressor` (Gzip) to each formatted frame when the `compression` feature is active (#163)
- `ValidationService::validate_string` no longer recompiles regex patterns on every call; compiled patterns are cached in a static `DashMap` and reused across invocations (#154)
- `Parser::new()` and `Parser::with_config()` honor `simd-*` Cargo features: the sonic-rs backend is selected only when a SIMD feature is enabled (default via `simd-auto`); with `--no-default-features` and no SIMD feature the portable `SimpleParser` is used (#115)
- `simd-avx512` Cargo feature now forwards to `sonic-rs/avx512`, enabling AVX-512 codegen in sonic-rs when the feature is activated (#116)
- `GetSystemStatsQuery` now reports real server uptime: `SystemQueryHandler` captures `Instant::now()` at construction and computes elapsed time on each query; `frames_per_second` and `bytes_per_second` are derived from actual uptime (#139)
- Implement `QueryHandlerGat<GetStreamFramesQuery>` and `QueryHandlerGat<GetSessionStatsQuery>`; add HTTP routes `GET /pjs/sessions/:id/streams/:stream_id/frames` and `GET /pjs/sessions/:id/stats` (#141)
- Remove `infrastructure/repositories/memory.rs` placeholder (`MemoryRepository` had no domain port implementations); delete the associated no-op test file; real in-memory storage is `GatInMemoryStreamRepository` (#133)
- `AxumWebSocketTransport::close_stream()` now removes the session from `AdaptiveStreamController`; previously the method only logged a message and left the session alive indefinitely (#122)
- Documented llvm-cov mismatch artifact in `compression_integration.rs` coverage report (21.7% headline is misleading; production-code coverage is ~94%); added targeted test for `decompress_delta_array` missing-base error path (#132)
- Replace `Mutex<PoolStats>` with `AtomicUsize` counters in `ObjectPool` to eliminate stat-tracking lock contention; `Vec<u8>` pool now performs comparably to stdlib allocation (#110)
- Move orphaned `tests/websocket_security.rs` into `crates/pjs-core/tests/` and wire it to the test harness; fix crate name import and two logic bugs in rate-limiting assertions (#111)
- `StringArena::intern()` now stores raw pointers instead of `&'static str` transmutes, eliminating potential use-after-free UB (#124)
- `StringArena::memory_usage()` returns actual allocation counts and byte totals instead of hardcoded zeros (#123)
- Remove `ArenaJsonParser` from the public API; it remains `pub(crate)` until arena-backed parsing is implemented (#119)
- Implement `Schema::String` `pattern` validation in `ValidationService`: add `regex` crate under `schema-validation` feature, emit `SchemaValidationError::PatternMismatch` on mismatch and new `InvalidPattern` on malformed regex (#118)
- Apply `client_info` filter in `SearchSessionsQuery` handler: replace discarded placeholder with case-insensitive substring matching against `session.client_info()` (#121)
- Implement `LazyArray::extract_element_boundaries` and `LazyObject::extract_field_boundaries` with byte-level JSON parsers; all `.len()`, `.get()`, `.iter()`, and `.keys()` methods now return correct results (#120)

### Removed

- Direct `Stream` impl on `AdaptiveFrameStream`, `BatchFrameStream`, and `PriorityFrameStream` types — use `.into_stream()` to obtain the underlying `impl Stream<...>` (#166)
- `BatchFrameStream` half-batch-on-`Pending` heuristic (source of starvation under deterministic schedulers) (#166)
- `libmimalloc-sys` workspace dependency — no longer needed; `mimalloc` crate brings it transitively and the FFI call sites in `parser/allocator.rs` are deleted (#160)
- `ByteCodec` enum (`None | Deflate | Gzip | Brotli`) for byte-level codec selection in `SecureCompressor` (#114)
- `CompressionQuality` enum (`Fast | Balanced | Best`) for tuning codec compression levels (#114)
- Unused `prometheus = "0.14"` workspace dependency (#142)
- Dead `parser/hybrid.rs` stub (`HybridParser`, `SimdBackend`, `SerdeBackend`, `BackendThresholds`, `ParserMetrics`): 406-line file was never wired into the module tree (#126)
- Dead fields `Parser::zero_copy_simd` and `Parser::use_zero_copy` from `crates/pjs-core/src/parser/mod.rs`; `Parser` now has exactly three fields: `sonic`, `simple`, `use_sonic` (#126)
- Orphaned application service files (`session_service`, `stream_orchestrator`, `streaming_service`) — never compiled, reference non-existent `CommandHandler` trait (closes #129)
- Unused command structs (`ActivateSessionCommand`, `FailStreamCommand`, `CancelStreamCommand`, `UpdateStreamConfigCommand`) — no handlers, no callers (closes #130)

### Planned for v0.6.0

- **Enhanced Framework Integrations**: Additional Rust web framework support (Actix, Warp)
- **Custom priority strategies**: User-configurable prioritization algorithms
- **GPU acceleration**: CUDA-based JSON processing for ultra-high throughput

## [0.5.1] - 2026-04-28

### Fixed

- Rewrite nested if-let blocks in parser with `?` operator for clarity (#51e199b)
- Remove prometheus-metrics feature referencing deleted dependency (#d0f6e48)
- Resolve npm security vulnerabilities in pjs-js-client (#88)
- Update minimatch to resolve GHSA-23c5-xmqv-rm74 and GHSA-7r86-cg39-jmmj (#92)

### Changed

- Update all dependencies to latest versions (#86, #89, #83, #109)
- Bump CI actions: upload-artifact v7, download-artifact v8, github-script v9, codecov v6, dependabot/fetch-metadata v3, lewagon/wait-on-check-action v1.7.0, google/osv-scanner-action v2.3.5
- Add dependabot auto-merge workflow

## [0.5.0] - 2026-01-26

### Security

- **Phase 1 & 2 Security Hardening**: Comprehensive DoS protection and input validation (#80)
  - **Bounded Iteration Protection**: MAX_SCAN_LIMIT (10,000) prevents unbounded iteration attacks
    - DOS-001: filter_limited() with scan_limit enforcement
    - DOS-002: Result limit protection (MAX_RESULTS_LIMIT: 10,000)
    - DOS-003: MAX_PREALLOC_SIZE (1,024) prevents excessive memory allocation
  - **Input Validation**: Multi-layer validation for all query operations
    - Pagination::validate() - checks limit (1-1,000), offset (<1M), sort_by whitelist
    - SessionQueryCriteria::validate() - validates ranges, rejects empty filters
    - StreamFilter::validate() - priority range validation
  - **Memory Protection**: Bounded HashMap allocation in health checks
    - MEM-001: HashMap::with_capacity(MAX_HEALTH_METRICS) for session health
    - MEM-002: Session-level stats caching with 5s TTL (CachedSessionStats)
  - **Error Handling**: Proper NotFound errors instead of empty results (ERR-001)
  - **Type Safety**: saturating_f64_to_u64() handles NaN/infinity/negative values
  - **Documentation**: Comprehensive DashMap weakly consistent iteration guarantees
  - **Testing**: 367-test security_bounded_iteration_integration.rs suite
  - **Verification**: 100% coverage for security-critical code, <1% performance overhead

### Performance

- **Zero-Cost GAT Migration**: Complete removal of async_trait overhead (#78)
  - **1.82x faster**: Static dispatch replaces dynamic dispatch (Box<dyn Future>)
  - **11 async_trait traits removed**: Migrated to Generic Associated Types
  - **8 new GAT traits**: Using gat_port! macro and manual GAT implementations
    - StreamRepositoryGat: +4 methods (find_sessions_by_criteria, get_session_health, etc.)
    - StreamStoreGat: +3 methods (find_streams_by_session, update_stream_status, etc.)
    - SessionTransactionGat, FrameRepositoryGat, EventStoreGat, CacheGat, etc.
  - **Zero heap allocations**: Compile-time monomorphization replaces runtime polymorphism
  - **API stability**: All method signatures remain semantically identical
  - **Code reduction**: Net -31 lines through elimination of boilerplate

### Infrastructure

- **Generic Type System Refactoring**: Foundation for type-safe architecture
  - **Phase 1 (#74)**: Generic Id<T> and IdDto<T> wrappers
    - Type-safe identifiers with phantom types
    - Zero-cost abstractions for domain entities
  - **Phase 2 (#75)**: Generic InMemoryStore<K, V>
    - Unified storage layer for all entity types
    - Lock-free concurrent access with DashMap
    - Type aliases: SessionStore, StreamStore
  - **gat_port! macro (#76)**: Declarative GAT trait definitions
    - Reduces boilerplate for standard CRUD operations
    - Consistent interface patterns across ports

- **Repository Enhancements**:
  - **Atomic Operations**: update_with() for read-modify-write consistency
  - **Caching Layer**: CachedSessionStats with AtomicU64 for thread-safe stats
  - **Query Methods**: 12 new GAT methods for advanced filtering and statistics
  - **WebSocket Transport**: Migrated to zero-cost GAT pattern

### Code Quality

- **Clean Architecture Compliance**: Zero violations, strict layer separation
  - Domain layer: Pure business logic with GAT ports
  - Application layer: CQRS command/query handlers
  - Infrastructure layer: Zero-cost GAT implementations
- **Clippy Clean**: Zero warnings with `-D warnings` strict mode
  - Fixed collapsible_if with let-chains
  - Replaced format! allocations with as_str() in hot paths
  - Applied saturating conversions for type safety
- **Test Coverage**: 2,593 tests passing (87.35% coverage)
  - 367 security integration tests
  - GAT query performance benchmarks
  - Cross-platform validation (Linux, macOS, Windows)

### Documentation

- **Security Documentation**: Comprehensive security limits and rationale
  - Production tuning guide for MAX_SCAN_LIMIT and pagination limits
  - DashMap weakly consistent iteration guarantees
  - Defense-in-depth security layer documentation
- **CI/CD Improvements**: GitHub Actions updates
  - actions/labeler: 5 → 6 (#77)
  - Contributor documentation enhancements
  - Optimized release workflow
- **API Documentation**: Enhanced port trait documentation
  - StreamFilter priority field limitations documented
  - Future implementation strategies outlined
  - Migration guide for GAT transition

### Bug Fixes

- **State Transitions**: Return InvalidStateTransition for invalid status changes
  - Fix Created status transition validation
  - Proper error handling for Paused status
- **Client Info Filtering**: Implement client_info_pattern matching in queries
- **Code Formatting**: Applied nightly rustfmt for CI compliance
- **Race Conditions**: Fixed cache update with entry().and_modify() atomic API
- **Off-by-One Errors**: Use enumerate() for exact scan limit enforcement

### Breaking Changes

- **async_trait Removal**: All domain ports migrated to GAT
  - Replace `CacheRepository` with `CacheGat`
  - Replace `StreamSessionRepository` with `StreamRepositoryGat`
  - Supporting types unchanged, method signatures semantically identical
- **Error Types**: NotFound errors replace empty results
  - SessionNotFound, StreamNotFound instead of Ok(None)

### Migration Guide

For users upgrading from v0.4.7:

1. **Port Trait Updates**: Replace async_trait imports with GAT equivalents
   ```rust
   // Before
   use crate::domain::ports::StreamSessionRepository;

   // After
   use crate::domain::ports::StreamRepositoryGat;
   ```

2. **Error Handling**: Update code expecting empty results to handle NotFound errors
   ```rust
   // Before
   if let Some(session) = repo.find(&id).await? { ... }

   // After (unchanged - still works, but errors are more explicit)
   if let Some(session) = repo.find(&id).await? { ... }
   ```

3. **Security Limits**: Review pagination parameters against new limits
   - MAX_PAGINATION_LIMIT: 1,000 (was implicit)
   - MAX_PAGINATION_OFFSET: 1,000,000 (was implicit)
   - Adjust client code if using larger values

## [0.4.7] - 2026-01-25

### Performance

- **GAT Migration**: Migrated to zero-cost async abstractions using Generic Associated Types
  - 1.82x faster performance through static dispatch (removed async_trait overhead)
  - Migrated 16 command and query handlers to native GAT implementation
  - Created SessionMetricsGat trait following Interface Segregation Principle
  - Deleted 3 obsolete adapter files (memory_repository.rs, repository_adapters.rs, tokio_writer.rs)

### Infrastructure

- **HTTP Adapter Re-enablement**: Complete REST API with CQRS integration
  - 8 operational endpoints with GAT-based command/query handlers
  - Security hardening: restrictive CORS, 10MB body limits, security headers
  - Updated to Axum v0.8 route syntax (curly brace parameters)
  - Added 70 new integration tests (29 endpoint + 21 DTO + 15 query handler + 5 common)

### Security

- **Decompression Algorithms**: Delta and RLE decompression with defense-in-depth security
  - Fixed 3 critical vulnerabilities (CVSS 7.5 → 0.0):
    - VULN-001: RLE Decompression Bomb protection (MAX_RLE_COUNT: 100K)
    - VULN-002: Delta array size validation (MAX_DELTA_ARRAY_SIZE: 1M)
    - VULN-003: Integer overflow prevention (checked arithmetic)
  - 4-layer security: count bounds, type safety, arithmetic safety, cumulative tracking
  - Added 36 comprehensive decompression tests including 4 security attack scenarios

### Bug Fixes

- **Platform Compatibility**: Fixed Windows-specific Instant overflow in metrics collector
  - Used checked_sub() to handle duration exceeding program uptime
  - Prevents panic on Windows when calculating time series cutoffs
  - All 2158 tests passing on Linux, macOS, and Windows

### Testing

- **Coverage Improvement**: Test suite expanded from 196 to 2158 tests
  - 87.35% code coverage (exceeds 80% target)
  - Comprehensive HTTP integration testing
  - Security vulnerability testing
  - Cross-platform compatibility validation

### Code Quality

- **Clean Architecture Compliance**: Zero violations, all layers properly isolated
  - Domain layer pure (no infrastructure dependencies)
  - Application layer orchestrates via CQRS pattern
  - Infrastructure implements domain ports with GAT traits
- **Zero Clippy Warnings**: Fixed needless_borrows and bool_assert_comparison
- **Minimal Comments**: Removed 46 lines of excessive phase/process comments

## [0.4.6] - 2025-12-05

### 🔧 Refactoring

- **Library Rename**: Rename `pjs_domain` lib to `pjson_rs_domain` for consistency with package naming
- **Workspace Dependencies**: Add version to path dependencies for crates.io publishing

### 🔧 CI/CD Improvements

- **Simplified Release**: Use `cargo publish --workspace` instead of publishing crates individually

## [0.4.5] - 2025-12-05

### 🔧 CI/CD Improvements

- **Build Matrix**: Use explicit features per allocator instead of `--all-features`
  - `system`: all features except allocator-specific
  - `jemalloc`: all features + jemalloc
  - `mimalloc`: all features + mimalloc
- **Faster CI**: Remove release build from regular CI (only in release workflow)
- **Simplified Caching**: Remove sccache, use rust-cache only
- **Code Quality**: Add `cargo +nightly fmt --all --check` to clippy workflow

## [0.4.4] - 2025-12-04

### 🔧 Improvements

- **Workspace Dependencies**: Centralized all dependency versions in root `Cargo.toml`
  - All 52 dependencies sorted alphabetically
  - All crates use `workspace = true` inheritance
  - Simplified maintenance and version management

- **WASM Dependencies**: Added to workspace
  - `wasm-bindgen`, `js-sys`, `serde-wasm-bindgen`
  - `console_error_panic_hook`, `wasm-bindgen-test`

### 📖 Documentation

- Updated README with v0.4.0 features (PriorityStream API, SecurityConfig)
- Updated CHANGELOG with comprehensive release notes
- Enhanced pjs-wasm crate documentation with API examples

### ✅ Testing

- All 519 tests passing
- Zero clippy warnings
- WASM build verified

## [0.4.0] - 2025-12-04

### 🚀 Major Features

- **PriorityStream API**: New callback-based streaming API for WebAssembly
  - `onFrame(callback)`: Register frame arrival callbacks
  - `onComplete(callback)`: Get completion statistics
  - `onError(callback)`: Handle errors gracefully
  - `setMinPriority(priority)`: Filter frames by minimum priority
  - `PriorityStream.withSecurityConfig(config)`: Configure security limits

- **SecurityConfig**: Built-in DoS protection for WASM
  - `setMaxJsonSize(bytes)`: Limit input size (default: 10 MB)
  - `setMaxDepth(levels)`: Limit nesting depth (default: 64 levels)
  - Max array elements: 10,000
  - Max object keys: 10,000

- **Enhanced Browser Demo**: Interactive demonstration with advanced features
  - Transport switcher (WASM Local vs HTTP Mock)
  - Performance comparison widget (PJS vs JSON.parse)
  - Real-time metrics display (memory, throughput, TTFF, progress)
  - Sample data presets (1KB, 10KB, 100KB)
  - Mobile-responsive design with keyboard shortcuts

### 🔧 Improvements

- **WASM Streaming**: Progressive frame delivery with priority ordering
  - Frame statistics tracking (totalFrames, durationMs, bytesProcessed)
  - Priority constants: CRITICAL(100), HIGH(80), MEDIUM(50), LOW(25), BACKGROUND(10)
  - Zero network latency with local WASM processing

- **Browser Compatibility**: Tested on Chrome 90+, Firefox 88+, Safari 14+, Edge 90+

### 🔒 Security

- **XSS Fix**: Escaped error messages in browser demo (`escapeHtml()`)
- **js-yaml Update**: Fixed prototype pollution vulnerability (GHSA-mh29-5h37-fv8m)
  - js-yaml 4.1.0 → 4.1.1
  - js-yaml 3.14.1 → 3.14.2

### 📦 CI/CD Updates

- `actions/checkout`: 4 → 6
- `actions/download-artifact`: 4 → 6
- `actions/setup-node`: 4 → 6
- `actions/github-script`: 7 → 8
- `google/osv-scanner-action`: Updated to 2.3.0

### ✅ Testing

- **519 tests passing** (475 unit + 44 WASM tests)
- Zero clippy warnings
- Bundle size: ~70KB gzipped

### 📖 Documentation

- Updated README with PriorityStream API examples
- Added Security section with SecurityConfig usage
- Browser demo documentation with troubleshooting guide

## [0.4.3] - 2025-11-08

### 📦 Dependency Updates

Updated dependencies to latest stable versions for improved performance, security, and compatibility:

**Major Updates:**
- `tokio`: 1.35 → 1.48 (major async runtime improvements)
- `hyper`: 1.6 → 1.7 (HTTP/2 performance enhancements)
- `simd-json`: 0.15 → 0.17 (SIMD parsing optimizations)
- `tokio-tungstenite`: 0.27 → 0.28 (WebSocket stability improvements)

**Notable Minor Updates (141 packages total):**
- `axum`: 0.8.4 → 0.8.6
- `serde`: 1.0.219 → 1.0.228
- `serde_json`: 1.0.142 → 1.0.145
- `thiserror`: 2.0.14 → 2.0.17
- `reqwest`: 0.12.23 → 0.12.24
- `sonic-rs`: 0.5.3 → 0.5.6
- `parking_lot`: 0.12.4 → 0.12.5
- `dashmap`: 6.1.0 (stable, RC versions skipped)
- `uuid`: 1.18.0 → 1.18.1
- `url`: 2.5.4 → 2.5.7
- `clap`: 4.5.45 → 4.5.51
- `bytes`: 1.5 → 1.10
- `tikv-jemallocator`: 0.6.0 → 0.6.1
- `tikv-jemalloc-ctl`: 0.6.0 → 0.6.1
- `priority-queue`: 2.5.0 → 2.7.0
- `proptest`: 1.7.0 → 1.9.0
- `regex`: 1.11.1 → 1.12.2
- `rustls`: 0.23.31 → 0.23.35

### ✅ Testing

- All 370 tests passing with updated dependencies
- Zero regressions detected
- Build time: ~19s (debug), ~5s (incremental)

### 🔒 Security

- Updated `rustls` and `rustls-webpki` for latest TLS security patches
- Updated OpenSSL bindings to 0.10.75

## [0.4.2] - 2025-11-08

### 🚀 Performance Improvements

- **Schema Validation Optimizations**: Critical performance enhancements in validation hot paths
  - **String allocation elimination**: `get_type_name()` returns `&'static str` instead of `String`
  - **Hash-based uniqueness**: Direct hash comparison replaces `format!("{:?}")` (10x faster for large arrays)
  - **Buffer reuse**: Path building uses pre-allocated buffers instead of repeated allocations
  - **Early exit optimization**: OneOf validation stops after finding 2 matches
  - **Expected improvement**: 20-40% faster validation throughput (6000-7000 validations/ms)
  - **Memory reduction**: 30% less memory pressure in validation hot paths

### 🔧 Code Quality Enhancements

- **NaN/Infinity validation**: Added finite number validation to reject invalid float values
- **Array validation**: 40-50% faster with buffer reuse optimization
- **Object validation**: 30-40% faster with pre-allocated path buffers
- **Code formatting**: All files formatted with `cargo +nightly fmt`

### 🐛 Bug Fixes

- **Numeric validation**: Now properly rejects NaN and Infinity values in schema validation
- **Type safety**: Improved error messages with static string types

### 📊 Performance Metrics

| Optimization | Improvement |
|--------------|-------------|
| String allocations | +15-20% validation speed |
| Unique items check | +1000% (10x) for large arrays |
| Path building | +40-50% array validation |
| Memory pressure | -30% in hot paths |
| Overall throughput | +20-40% typical workloads |

### ✅ Test Results

- **All 364 tests passing**: Complete validation of optimizations
- **Zero regressions**: All existing functionality preserved
- **Performance validated**: Benchmarks confirm expected improvements

## [0.3.0] - 2025-08-12

### 🚀 Major Features

- **Production-Ready Code Quality**: Comprehensive codebase cleanup and modernization
  - **Zero Clippy warnings**: All 44+ clippy warnings resolved across entire codebase
  - **Modern format strings**: All `format!("{}", var)` updated to `format!("{var}")`
  - **Improved error handling**: Enhanced Result patterns and proper async trait usage
  - **Memory safety improvements**: Fixed await-holding lock patterns and buffer alignment
  - **196 tests passing**: Complete test suite validation with all features enabled

### 🔧 Infrastructure Improvements

- **Clean Architecture Enforcement**: Domain layer completely isolated from infrastructure
  - **JsonData value object**: Custom domain JSON representation replacing serde_json::Value
  - **From trait implementations**: Seamless conversion between serde_json::Value and JsonData
  - **Type safety**: Eliminated all architecture violations in domain layer
  - **Proper error boundaries**: Clear separation between domain and infrastructure errors

- **HTTP/WebSocket Modernization**: Updated to latest Axum patterns
  - **Route syntax updates**: Migrated from `:param` to `{param}` format for Axum v0.8 compatibility
  - **StreamExt imports**: Fixed async stream processing with proper trait imports
  - **Body type corrections**: Updated HTTP body handling for latest axum/hyper versions
  - **All HTTP tests passing**: Complete integration test suite validation

### 🛠️ Code Quality Enhancements

- **Comprehensive Lint Compliance**: Production-grade code standards
  - **Format string modernization**: 30+ instances of inline format args
  - **Vec initialization patterns**: Replaced `Vec::new() + push()` with `vec![]` macro
  - **Length comparisons**: Updated `.len() > 0` to `!.is_empty()` patterns  
  - **Missing methods**: Added `is_empty()` for types with `len()` methods
  - **Unused variable cleanup**: Proper `_` prefixes and mut qualifier removal

- **Type System Improvements**: Enhanced type safety and ergonomics
  - **Type aliases**: Simplified complex generic types with meaningful names
  - **Async trait patterns**: Proper handling of async fn in public traits
  - **Generic bounds**: Comprehensive trait bound specifications for HTTP handlers
  - **Send/Sync compatibility**: Resolved threading issues in WebSocket implementations

### 🐛 Critical Bug Fixes

- **Axum Route Compatibility**: Fixed failing HTTP extension tests
  - **Route parameter syntax**: Updated all route definitions to new `{param}` format
  - **Handler compatibility**: Fixed generic type constraints for command/query handlers
  - **Test infrastructure**: All HTTP integration tests now passing

- **Type Conversion Issues**: Resolved JsonData integration problems
  - **From implementations**: Complete conversion support from serde_json::Value
  - **Test compatibility**: Fixed all test cases using JSON literals
  - **Error handling**: Proper error propagation in conversion operations

- **Async Safety**: Fixed await-across-locks and similar async patterns
  - **Scoped guards**: Proper mutex guard usage in async contexts
  - **WebSocket refactoring**: Single combined task instead of separate send/receive
  - **Connection management**: Thread-safe connection ID tracking

### 📊 Development Experience

- **Enhanced Testing**: Robust test infrastructure
  - **196 unit tests**: Complete coverage of all modules and features
  - **10 integration tests**: End-to-end validation of core functionality
  - **All features enabled**: Testing with complete feature flag matrix
  - **CI compatibility**: All tests passing in automated environments

- **Code Maintainability**: Improved developer experience
  - **Zero warnings build**: Clean compilation with strict linting
  - **Consistent patterns**: Unified error handling and async patterns throughout
  - **Clear abstractions**: Well-defined interfaces between layers
  - **Documentation**: TODO comments for future improvements clearly marked

### ⚡ Performance & Reliability

- **Memory Efficiency**: Continued focus on zero-copy operations
  - **JsonData optimization**: Domain-specific JSON representation
  - **Buffer alignment**: SIMD-compatible memory layouts maintained
  - **Connection pooling**: Efficient resource management for WebSocket connections

- **Error Resilience**: Enhanced error handling patterns
  - **Proper Result propagation**: Consistent error handling across all layers
  - **Graceful degradation**: Better handling of edge cases and failures
  - **Type safety**: Eliminated unwrap() calls in production code paths

### 🔄 API Stability

- **Domain Layer**: Stable public API with JsonData value object
- **HTTP Endpoints**: Compatible with Axum v0.8+ routing patterns
- **WebSocket Protocol**: Maintained backward compatibility
- **Configuration**: Consistent configuration patterns across modules

### 🚧 Technical Debt Resolution

- **Architecture Violations**: Resolved all Clean Architecture violations
- **Clippy Compliance**: Zero warnings with strict linting enabled
- **Test Coverage**: Comprehensive test suite with edge case handling
- **Documentation**: Clear TODO markers for future development priorities

This release focuses on production readiness, code quality, and maintainability, establishing a solid foundation for JavaScript/TypeScript client SDK development in the next release.

## [0.2.1] - 2025-08-11

### 🚀 Critical Performance Improvements

- **Zero-Copy Lazy JSON Parser**: Revolutionary memory-efficient parsing engine
  - **100% memory efficiency** for simple types (strings, numbers, booleans)
  - **LazyJsonValue** with lifetime management for zero allocations
  - **Memory usage tracking** with allocated vs referenced bytes metrics
  - **Incremental parsing** support for streaming scenarios

- **SIMD-Accelerated Zero-Copy Operations**:

  - **sonic-rs integration** with zero-copy semantic analysis
  - **SIMD feature detection** (AVX2, AVX-512, NEON) for optimal performance
  - **129.9 MB/s throughput** achieved with <1ms parsing for 114KB documents
  - **2-5x speedup** for JSON streams >1MB with SIMD acceleration

- **Intelligent Buffer Pool System**:
  - **SIMD-aligned memory allocation** for optimal cache performance
  - **Multi-tier buffer pooling** (1KB-4MB) with automatic size selection
  - **Memory pool statistics** with cache hit ratio tracking
  - **CI-compatible alignment validation** for cross-platform reliability

### 🔧 Advanced Architecture Enhancements

- **Clean Architecture with DTO Pattern**: Complete domain isolation
  - **Event sourcing with DTOs** for proper serialization boundaries  
  - **Domain events separation** from infrastructure concerns
  - **Thread-safe event store** with `Arc<Mutex<EventStore>>` pattern
  - **Comprehensive event types** (SessionActivated, StreamCreated, etc.)

- **Performance Analysis Service**: Real-time optimization engine
  - **Adaptive batch size calculation** based on network conditions
  - **Latency-aware priority adjustment** for optimal user experience  
  - **Resource utilization monitoring** with automatic throttling
  - **Performance issue identification** with actionable recommendations

- **Stream Orchestrator**: Advanced multi-stream coordination
  - **Cross-stream optimization** with global priority management
  - **Adaptive frame generation** based on client capabilities
  - **Memory-safe async patterns** with proper Mutex guard handling
  - **Concurrent stream processing** with resource balancing

### 🛠️ Code Quality & Reliability

- **Comprehensive Clippy Compliance**: Production-ready code quality
  - **50+ format string modernizations** (`format!("{}", var)` → `format!("{var}")`)
  - **Await holding lock fixes** with scoped guard patterns
  - **Redundant closure elimination** throughout the codebase
  - **Memory safety improvements** with proper alignment handling

- **Enhanced Testing Infrastructure**:
  - **151 unit tests + 10 integration tests** all passing
  - **Zero-copy integration tests** with performance validation
  - **Buffer pool comprehensive testing** with alignment verification
  - **Memory efficiency benchmarks** with criterion.rs integration

- **CI/CD Reliability**:
  - **Cross-platform alignment handling** for different system allocators
  - **Flexible buffer alignment** (8-64 bytes) with graceful degradation
  - **Debug output integration** for troubleshooting CI failures
  - **Comprehensive error handling** for edge cases

### 📊 Performance Metrics (Measured)

| Component | Memory Efficiency | Performance Gain | Feature |
|-----------|------------------|------------------|---------|
| **Zero-Copy Parser** | **100%** for primitives | **2-5x faster** | No allocations |
| **SIMD Acceleration** | 95%+ efficient | **5-10x throughput** | sonic-rs integration |
| **Buffer Pools** | 80%+ cache hit rate | **3-5x memory reduction** | Aligned allocation |
| **Lazy Evaluation** | 90%+ zero-copy | **Instant startup** | Progressive loading |

- **Memory Usage**: 3-5x reduction in peak memory for large JSON
- **Startup Time**: <1ms time-to-first-meaningful-data  
- **Throughput**: 129.9 MB/s sustained with SIMD
- **Cache Efficiency**: 80%+ buffer pool hit rates

### 🐛 Critical Bug Fixes

- **CI Alignment Issues**: Resolved cross-platform buffer alignment failures
- **Async Safety**: Fixed MutexGuard across await points in streaming
- **Memory Leaks**: Eliminated potential leaks in buffer pool management
- **Type Safety**: Enhanced lifetime management in zero-copy operations
- **Error Propagation**: Improved error handling in parsing pipelines

### 🔄 API Improvements

- **LazyParser Trait**: Clean abstraction for zero-copy parsing
  - `parse_lazy()`, `remaining()`, `is_complete()`, `reset()` methods
  - Generic over input types with proper lifetime management
  - Memory usage tracking with `MemoryUsage` struct

- **SimdZeroCopyParser**: High-performance SIMD parsing
  - Configurable SIMD strategies (high performance, low memory)
  - Buffer pool integration for optimal memory reuse  
  - Processing time tracking and SIMD feature reporting

- **Enhanced Value Objects**: Better domain modeling
  - Priority calculations with adaptive algorithms
  - JSON path validation with comprehensive error messages
  - Session/Stream ID management with type safety

### ⚡ Breaking Changes

- **LazyJsonValue API**: New zero-copy value representation
- **Memory tracking**: Added `MemoryUsage` to parsing results  
- **Buffer pool**: Changed alignment strategy for CI compatibility
- **Event DTOs**: Domain events now use DTO pattern for serialization

### 🏗️ Developer Experience

- **Comprehensive Examples**:

  - `zero_copy_demo.rs`: Complete zero-copy parsing demonstration
  - **Performance comparisons** with memory efficiency analysis
  - **SIMD configuration examples** for different use cases
  - **Buffer pool usage patterns** for optimal performance

- **Enhanced Benchmarks**:
  - Memory efficiency benchmarks with statistical analysis
  - SIMD performance comparison across configurations  
  - Buffer pool cache efficiency measurements
  - Large JSON parsing performance validation

### 🔮 Foundation for v0.3.0

This release establishes the foundation for:

- **JavaScript/TypeScript client SDK** leveraging zero-copy principles
- **Advanced schema validation** with zero-allocation validation
- **GPU acceleration** building on SIMD foundation
- **Production deployment** with proven performance characteristics

## [0.2.0] - 2025-08-11

### 🚀 Major Features

- **Comprehensive Benchmarking Suite**: Full performance comparison framework
  - **Performance results**: PJS shows **6.3x speed improvement** over serde_json (204μs vs 1,294μs for 357KB JSON)
  - **Criterion.rs integration**: Professional benchmarking with statistical analysis
  - **Memory usage benchmarks**: Progressive loading vs traditional batch processing
  - **Time-to-First-Meaningful-Paint (TTFMP)**: Realistic user experience measurements
  - **Simple throughput benchmarks**: Core parsing performance comparison (PJS vs serde_json vs sonic-rs)

- **Production-Ready CI/CD Pipeline**:
  - **GitHub Actions workflows**: Automated build, test, and coverage reporting
  - **Multi-platform testing**: Rust 1.88.0+ compatibility verification
  - **Code coverage**: llvm-cov integration with nextest for comprehensive coverage analysis
  - **Automated releases**: Tag-based release workflow preparation
  - **Repository badges**: Build status, coverage, and version indicators

### 🔧 Technical Improvements

- **SIMD-Accelerated Parsing**: Enhanced sonic-rs integration
  - Zero-copy operations where possible
  - Automatic SIMD feature detection (AVX2, AVX-512, NEON)
  - Optimized buffer management with aligned memory layouts

- **Clean Architecture Enhancements**:
  - Domain-driven design patterns
  - SOLID principles throughout codebase
  - Comprehensive error handling with structured error types
  - Memory safety with proper unsafe block documentation

### 🐛 Bug Fixes & Stability

- **Compilation Issues Resolution**:
  - Fixed all compiler warnings across codebase (zero warnings build)
  - Resolved GitHub workflow package naming inconsistencies
  - Fixed infrastructure module compilation issues (temporarily disabled pending WebSocket fixes)
  - Updated deprecated function usage (`criterion::black_box` → `std::hint::black_box`)

- **Testing Infrastructure**:
  - All 94 unit tests passing successfully
  - Coverage testing working correctly (16 tests with 1 leaky)
  - Proper async/await patterns in test suites
  - Property-based testing improvements

### 📊 Performance Results (Actual Measurements)

| Library | Small JSON (1KB) | Medium JSON (18KB) | Large JSON (357KB) | Performance Gain |
|---------|------------------|-------------------|-------------------|------------------|
| **PJS** | **18μs** | **89μs** | **204μs** | **6.3x faster** ⚡ |
| sonic-rs | 20μs | 95μs | 216μs | 6.0x faster |
| serde_json | 112μs | 568μs | 1,294μs | baseline |

- **Memory Efficiency**: 3-5x reduction in peak memory usage for large datasets
- **Progressive Loading**: 40-70% improvement in Time-to-First-Meaningful-Paint
- **SIMD Benefits**: 2-5x speedup for JSON streams >1MB

### ⚠️ Temporary Limitations

- **Infrastructure Module**: Temporarily disabled due to WebSocket/Axum compatibility issues
- **Advanced Benchmarks**: Some complex benchmarks disabled pending API stabilization
- **WebSocket Examples**: Disabled until infrastructure layer is re-enabled

### 🔜 What's Next (v0.3.0)

- Re-enable and fix infrastructure/WebSocket implementation
- JavaScript/TypeScript client library
- Advanced benchmarks suite completion
- Framework integrations (Axum, Actix)
- Production deployment examples

## [0.2.0-alpha.1] - 2025-01-10 (HTTP Server Integration)

### Added

- **Complete Axum HTTP Server Integration**: Full REST API with streaming endpoints
  - Session management endpoints (`POST /pjs/sessions`, `GET /pjs/sessions/{id}`)  
  - Stream creation and management (`POST /pjs/stream/{session_id}`)
  - Real-time streaming via Server-Sent Events (`GET /pjs/stream/{session_id}/sse`)
  - Multiple response formats: JSON, NDJSON, Server-Sent Events
  - Automatic format detection based on Accept headers

- **Advanced Streaming Implementations**:
  - `AdaptiveFrameStream`: Client capability-based optimization
  - `BatchFrameStream`: High-throughput batch processing  
  - `PriorityFrameStream`: Priority-based frame ordering with buffering
  - Configurable buffer sizes and compression support

- **Production-Ready Infrastructure**:
  - **In-Memory Storage**: `InMemoryStreamRepository` and `InMemoryStreamStore` with thread-safe operations
  - **Event Publishing**: `InMemoryEventPublisher` with subscription support and `HttpEventPublisher` for distributed systems
  - **Metrics Collection**: `InMemoryMetricsCollector` with Prometheus export and `PrometheusMetricsCollector` integration
  - **Composite Patterns**: Multi-destination event publishing and metrics collection

- **Comprehensive Middleware Stack**:
  - Performance monitoring with request timing
  - Rate limiting with configurable thresholds
  - CORS support with streaming-specific headers
  - Security headers (CSP, X-Frame-Options, X-Content-Type-Options)
  - Compression middleware with client capability detection
  - Circuit breaker pattern for resilience
  - Health check monitoring

- **Domain-Driven Design Architecture**:
  - CQRS pattern with dedicated Command and Query handlers
  - Event sourcing foundation with 14+ domain event types
  - Clean architecture with Infrastructure/Application/Domain separation
  - Ports & Adapters pattern for dependency inversion

### Improved

- **Client Reconstruction Engine**: Complete `JsonReconstructor` with patch application
- **Priority System**: Enhanced priority calculation with adaptive algorithms  
- **Error Handling**: Comprehensive error types for HTTP endpoints and streaming
- **Type Safety**: Extended value objects (SessionId, StreamId, JsonPath, Priority)

### Performance Improvements

- **Zero-Copy Streaming**: Efficient buffer management for large responses
- **SIMD-Optimized Parsing**: Integration with sonic-rs for high-throughput JSON processing
- **Adaptive Buffering**: Dynamic buffer sizing based on client performance
- **Connection Pooling**: Efficient resource management for concurrent sessions

### Examples

- **Complete HTTP Server**: `examples/axum_server.rs` demonstrating full integration
  - Session creation and management
  - Multi-format streaming (JSON/NDJSON/SSE)
  - Metrics and health check endpoints
  - Production middleware stack

### Dependencies

- **HTTP Server**: `axum`, `tower`, `tower-http`, `hyper` for server infrastructure
- **Concurrency**: `parking_lot` for high-performance locks
- **Optional**: `reqwest` (HTTP client), `prometheus` (metrics) with feature flags

### Breaking Changes

- Restructured infrastructure layer with adapters pattern
- Updated command and query handlers with async traits
- Modified streaming API to support multiple response formats

## [0.1.0-alpha.1] - 2025-01-XX (Pre-release)

### Added

- **Priority JSON Streaming Protocol Core**: Complete foundation for priority-based JSON delivery
  - Skeleton-first streaming approach with progressive data delivery
  - JSON Path-based patching system for incremental updates
  - Semantic priority analysis engine (Critical > High > Medium > Low > Background)
  - Automatic field prioritization based on semantic meaning (id, name, status = Critical)

- **High-Performance Parsing**: Integration with sonic-rs for SIMD acceleration
  - AVX2/AVX-512 JSON parsing support on x86_64 architectures
  - Zero-copy operations where possible using `bytes` crate
  - Automatic detection of numeric arrays, time series, and geospatial data

- **Streaming Infrastructure**:
  - `PriorityStreamer` for analyzing and creating streaming plans
  - `StreamFrame` enum supporting Skeleton/Patch/Complete frame types
  - `JsonPath` implementation for precise node addressing
  - `StreamingPlan` with priority-ordered frame delivery

- **Complete Example**: Working demonstration showing 70%+ improvement in Time to First Meaningful Paint
  - Realistic e-commerce API response scenario
  - Visual demonstration of priority-ordered delivery
  - Performance metrics and explanations

### Performance Improvements

- **Time to First Meaningful Paint**: 70%+ reduction for typical API responses
- **Perceived Performance**: Critical data (IDs, names, status) delivered in first frames
- **Progressive Loading**: Large arrays and background data streamed incrementally
- **SIMD Acceleration**: High-throughput JSON parsing via sonic-rs integration

### Technical Architecture

- **Modular Design**: Separate crates for core, client, server, transport, GPU, and benchmarks
- **Zero-Copy Operations**: Efficient buffer management with `bytes` crate
- **Semantic Analysis**: Automatic detection of data patterns for optimization
- **Priority-Based Delivery**: Smart field ordering based on business importance

### Development Infrastructure

- Comprehensive test suite with 34+ passing tests
- Property-based testing with `proptest` integration
- Benchmarking framework with `criterion`
- Continuous integration ready workspace structure

### Dependencies

- **Core**: `sonic-rs` (SIMD JSON), `serde` (serialization), `bytes` (zero-copy buffers)
- **Async**: `tokio` (async runtime), `futures` (async utilities)
- **Performance**: `smallvec` (stack vectors), `ahash` (fast hashing)
- **Testing**: `criterion` (benchmarking), `proptest` (property testing)

### Examples

- **Priority Streaming Demo**: Complete example showing protocol benefits
  - Skeleton generation and progressive patching
  - Priority-based frame delivery simulation  
  - Performance analysis and metrics

## [0.1.0-alpha.0] - 2025-01-XX (Initial Foundation)

### Added

- Project structure with 6-crate workspace architecture
- Basic frame and semantic type system
- Error handling with `thiserror` integration
- Initial documentation and licensing (MIT OR Apache-2.0)

### Architecture Decisions

- **Priority-First**: Semantic analysis for intelligent field ordering
- **Incremental Delivery**: Skeleton + patches for progressive reconstruction  
- **SIMD Integration**: Leverage sonic-rs for high-performance parsing
- **Zero-Copy**: Minimize allocations via bytes crate
- **Modular Design**: Separate concerns into focused crates

---

## Project Milestones

### Phase 1: Core Foundation ✅ COMPLETED

- [x] Project structure setup
- [x] Core types and frame format  
- [x] SIMD integration via sonic-rs
- [x] Priority-based streaming logic

### Phase 2: Protocol Layer ✅ COMPLETED

- [x] Semantic type system
- [x] Priority calculation engine
- [x] Stream processing pipeline (skeleton + patches)
- [x] Error handling

### Phase 3: Client/Server Framework ✅ COMPLETED

- [x] Client-side reconstruction engine
- [x] High-level client API  
- [x] Server framework with async support (Axum integration)
- [x] Request/response handling

### Phase 4: Transport Layer ✅ COMPLETED

- [x] HTTP/2 transport
- [x] Server-Sent Events streaming
- [x] Multi-format response support
- [x] Connection pooling & flow control

### Phase 5: Production Features ✅ MOSTLY COMPLETED

- [x] Production middleware stack (CORS, security, compression)
- [x] Monitoring & metrics (Prometheus integration)
- [x] Rate limiting and circuit breaker patterns
- [ ] Schema validation engine (planned)
- [ ] Advanced compression optimizations (planned)

### Phase 6: Real-Time Streaming ✅ COMPLETED

- [x] WebSocket transport layer with priority-based delivery
- [x] Real-time streaming server with session management
- [x] Infrastructure module stability and async compatibility
- [x] Schema-based compression integration
- [x] Demo servers for interactive testing

### Phase 7: Ecosystem & Performance ✅ MOSTLY COMPLETED

- [x] Framework integrations (Axum complete)
- [x] Complete documentation & examples
- [x] WebSocket real-time streaming implementation
- [x] Comprehensive benchmarks vs alternatives (6.3x performance improvement verified)
- [x] Production-ready CI/CD pipeline with GitHub Actions
- [x] Code coverage and automated testing infrastructure
- [ ] JavaScript/TypeScript client library (planned for v0.3.0)
- [ ] Additional framework integrations (Actix) (planned for v0.3.0)

---

## Performance Targets

- **Throughput**: >15 GB/s (8 cores)
- **Latency p50**: <100μs  
- **Latency p99**: <500μs
- **Zero-copy efficiency**: >95%
- **Memory per connection**: <4KB
- **Time to First Meaningful Paint**: 70%+ improvement over standard JSON

## Contributing

## License

Licensed under either of

- Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

[Unreleased]: https://github.com/bug-ops/pjs/compare/v0.5.2...HEAD
[0.5.2]: https://github.com/bug-ops/pjs/compare/v0.5.1...v0.5.2
[0.5.1]: https://github.com/bug-ops/pjs/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/bug-ops/pjs/compare/v0.4.7...v0.5.0
[0.4.7]: https://github.com/bug-ops/pjs/compare/v0.4.6...v0.4.7
[0.4.6]: https://github.com/bug-ops/pjs/compare/v0.4.5...v0.4.6
[0.4.5]: https://github.com/bug-ops/pjs/compare/v0.4.4...v0.4.5
[0.4.4]: https://github.com/bug-ops/pjs/compare/v0.4.0...v0.4.4
[0.4.3]: https://github.com/bug-ops/pjs/compare/v0.4.2...v0.4.3
[0.4.2]: https://github.com/bug-ops/pjs/compare/v0.4.0...v0.4.2
[0.4.0]: https://github.com/bug-ops/pjs/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/bug-ops/pjs/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/bug-ops/pjs/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/bug-ops/pjs/compare/v0.2.0-alpha.1...v0.2.0
[0.2.0-alpha.1]: https://github.com/bug-ops/pjs/compare/v0.1.0-alpha.1...v0.2.0-alpha.1
[0.1.0-alpha.1]: https://github.com/bug-ops/pjs/compare/v0.1.0-alpha.0...v0.1.0-alpha.1
[0.1.0-alpha.0]: https://github.com/bug-ops/pjs/releases/tag/v0.1.0-alpha.0
