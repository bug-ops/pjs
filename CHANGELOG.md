# Changelog

<!-- markdownlint-disable MD024 -->

All notable changes to the Priority JSON Streaming Protocol (PJS) project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Security

- **BREAKING** `ValidationService`'s compiled-regex cache for schema `pattern` validation (`domain::services::validation_service`) is no longer a process-wide, process-lifetime `static` — an unbounded `DashMap<String, regex::Regex>` that grew forever, with each entry able to reach the crate-default ~10 MiB compiled-program bound, so a modest number of distinct attacker-supplied patterns could retain multiple GiB for the life of the process (CWE-400). The cache is now a per-`ValidationService`-instance field of `Arc<regex::Regex>`s, reclaimed when the service is dropped, flushed once it reaches a 64-entry cap, and compiled via `RegexBuilder` with a 1 MiB size limit and a 64 KiB DFA limit instead of the crate defaults — bounding a single thread's view of the cache to roughly `64 * 1 MiB` = 64 MiB, and total process memory (accounting for `regex`'s per-thread lazy-DFA pool inside each compiled `Regex`) to roughly `64 MiB + entries * threads_touched * 64 KiB` ≈ 320 MiB at 64 threads. The 1 MiB size limit (rather than a smaller one) is a deliberate trade-off: ordinary patterns like a bare `YYYY-MM-DD` date or an IPv4-shaped pattern need up to 128 KiB, so a meaningfully tighter limit rejects realistic schema patterns, not just pathological ones — a regression test now compiles a corpus of realistic patterns (email, uuid, iso8601, ipv4, date, semver, slug, hex-color, jwt-shaped) to guard against tightening this again; patterns needing a Unicode character class under bounded repetition (e.g. `^\p{L}{1,64}$`) remain rejected as a documented limitation. Patterns longer than a new `max_pattern_length` (default 1024 bytes, configurable via `ValidationService::with_max_pattern_length`, hard-clamped to an 8 KiB ceiling) are now rejected before compilation with a new `SchemaValidationError::PatternTooLong` variant, which also bounds how much pattern text can ever appear in `InvalidPattern`/`PatternMismatch` error values. Additionally, the regex match against the input value previously ran while still holding the cache's per-shard write-lock guard, serializing unrelated requests hashing to the same `DashMap` shard; the guard is now dropped (after cloning the `Arc<Regex>`) before matching. **Breaking:** a schema `pattern` longer than 1024 bytes (or one whose compiled program exceeds 1 MiB) that previously validated successfully now fails with `PatternTooLong`/`InvalidPattern`; `ValidationService::with_max_depth` changed from an associated constructor (`ValidationService::with_max_depth(n)`) to a chained builder (`ValidationService::new().with_max_depth(n)`), matching `with_max_pattern_length`'s shape (#418)
- **BREAKING** `AxumWebSocketTransport::upgrade_handler` (`infrastructure::websocket::server`) accepted every WebSocket upgrade regardless of the request's `Origin` header, so any web page could open an authenticated WebSocket connection to a mounting application's server and drive its stream controller cross-site (CWE-346, CSWSH). A new `with_allowed_origins(Vec<String>)` builder on `AxumWebSocketTransport` restricts upgrades to an allow-list with the same config semantics as `HttpServerConfig::allowed_origins`'s CORS layer (`[]` denies all cross-origin upgrades — the default — `["*"]` allows any origin, mixing `"*"` with explicit origins fails closed; note `["*"]` is more dangerous here than CORS `Any`, since browsers attach ambient credentials to a WebSocket handshake regardless of the server's `Origin` response, unlike CORS). An entry that can never match a real `Origin` header (missing `scheme://`, a trailing path, or uppercase letters) is now logged with `warn!` instead of silently accepted — `"null"` (a legitimate `Origin` value from sandboxed iframes and `file://` pages) is exempt from this check. An upgrade request that carries an `Origin` header not on the list is rejected with `403 Forbidden` before any WebSocket frames are exchanged; a request with no `Origin` header at all is still allowed, since CSWSH requires a browser attaching ambient credentials and browsers always send `Origin` on a WebSocket handshake — this default protects browser clients while leaving native clients (`PjsWebSocketClient` and others that never send `Origin`) unaffected. **Breaking:** every browser-originated WebSocket upgrade is now refused by default (`Origin` present, allow-list empty) — deployments that need to accept cross-origin browser connections must opt in explicitly via `with_allowed_origins`; existing `AxumWebSocketTransport::new()`/`::default()` call sites remain source-compatible but change behavior for browser clients (#417)
- `axum_extension::handle_sse_stream` no longer hardcodes `Access-Control-Allow-Origin: *` on every SSE response — `PjsExtension` bolts onto an arbitrary existing router and previously imposed a permissive CORS policy regardless of what the mounting application configured (CWE-942). `HttpExtensionConfig` gained a new `allowed_origins: Vec<String>` field (default `vec![]`, same-origin only — no header at all) that opts a `PjsExtension`-mounted router into cross-origin access with a validated allowlist, sharing `axum_adapter::HttpServerConfig::allowed_origins`'s validation rules (empty denies all, `["*"]` allows any origin, mixing `"*"` with explicit origins is rejected and falls back to no CORS layer rather than panicking). **Deployments serving `pjs-js-client`'s `EventSource`-based SSE transport cross-origin must now set `allowed_origins` explicitly** — the previous unconditional wildcard is gone and is not replaced by a default-on origin list (#403)
- `add_rate_limit_headers` (`infrastructure::http::middleware`) no longer emits hardcoded `X-RateLimit-Limit: 100` / `X-RateLimit-Remaining: 99` / a fixed 60s reset+retry-after window regardless of actual configuration or client state — misleading for RFC 6585 client backoff, particularly on rejected requests, and previously self-contradictory when a 429 response's `Retry-After: 60` disagreed with a differently-configured window. `Limit` now reflects `RateLimitConfig::max_requests_per_window`; `Remaining`, `Reset`, and a 429's `Retry-After`/`retry_after` body field are all backed by two new `WebSocketRateLimiter` accessors — `remaining_for` (window-filtered per-client request count, mirroring `check_request`'s pruning) and `reset_after` (the real sliding-window expiry, derived from the client's oldest tracked request timestamp rather than approximated as `now + window_duration`) (#398)
- **BREAKING** Removed `infrastructure::http::middleware::pjs_cors_middleware`, a `pub` `from_fn`-compatible middleware that unconditionally hardcoded `Access-Control-Allow-Origin: *` on every response it wrapped — the same CWE-942 shape as the SSE header fixed above, in the same module. It was never wired into any router this crate ships (`axum_adapter`/`axum_extension` never called it); only test code referenced it. Superseded by the validated-allowlist `CorsLayer` pattern (`axum_adapter::build_cors_layer_from_origins`, `HttpExtensionConfig::allowed_origins`) — construct a `CorsLayer` directly (`tower_http::cors`) if a standalone from-fn CORS middleware is still needed (#403)

### Added

- `pjs-demo`'s `websocket-streaming-server` now parses a minimal JSON command protocol from incoming client `Message::Text` frames — `{"command":"pause"}` / `{"command":"resume"}` — and wires it to the in-flight `stream_data` frame-sending task via a `tokio::sync::watch` channel carrying a `StreamControl { paused: bool }` struct (a bare two-variant enum would have made a later third control field silently un-pause an active pause, since `watch` is latest-wins). Previously every incoming text message was only logged via `info!` and had no effect, so a stream could never be paused or resumed once started. Malformed or unrecognized commands are logged with `tracing::warn!` and ignored rather than panicking or closing the connection. `create_demo_frames` also now iterates the actual per-field data nested under the sample payload's `"dashboard"` key instead of the payload's single top-level key, so a session streams multiple frames (previously always exactly one, which also left the pause check effectively unreachable and the field-based priority table dead code); the completion signal is now gated on the same pause check as each frame. The connecting task (streaming or receive-loop, whichever loses the race) is now aborted instead of left running detached once the WebSocket connection ends (#406)

### Fixed

- `ZeroCopyParser::unescape_string` (`crates/pjs-core/src/parser/zero_copy.rs`) silently dropped `\uXXXX` Unicode escape sequences instead of decoding them — on encountering `\u`, it advanced the cursor past the six-byte escape and appended nothing to the output buffer, corrupting any string containing a Unicode escape (CWE-176; a content filter matching against the escaped source text would never see the smuggled decoded characters). `LazyJsonValue::StringOwned` is documented as "fully-unescaped", so downstream code had no signal this was happening. `\uXXXX` escapes are now decoded to their UTF-8 representation per RFC 8259, including surrogate-pair handling for supplementary-plane codepoints (a high surrogate `\uD800`-`\uDBFF` immediately followed by a low surrogate `\uDC00`-`\uDFFF`); a lone/unpaired surrogate or invalid hex digits (including a `+`/`-` sign, which `u32::from_str_radix` alone would otherwise accept) now return `DomainError::InvalidInput` instead of silently emitting wrong bytes. **Note:** inputs that previously parsed successfully with silently corrupted or dropped content — lone surrogates, malformed `\u` escapes, non-hex digits — now hard-error; this is a semver-visible behavior change for callers of the public `ZeroCopyParser` (#432)
- `AxumWebSocketTransport::handle_websocket_message` (the private handler driving the production `handle_socket` loop) and `WebSocketTransport::handle_message` (the public trait method) independently duplicated `StreamInit`/`FrameAck`/`Ping` handling — only the former recorded a newly created session in `connection_sessions` for disconnect cleanup, so a caller driving the transport via the public `handle_message` directly got sessions never associated with their connection (bounded by the existing 1h session-max-age sweep, not unbounded). `handle_websocket_message` now delegates to `handle_message`, which is the single place that both creates a session and registers it in `connection_sessions` (#415)
- `pjs-demo`'s `social::generate_social_data` could panic with "cannot sample empty range" while generating a post's `engagement` block — `rng.random_range(0..likes/3)` for `shares` whenever `likes < 3`, and `rng.random_range(likes..likes*10)` for `views` whenever `likes == 0` — both reachable from `likes = rng.random_range(0..1000)`, hitting a meaningful fraction of calls (roughly 7% of sessions for a Medium-sized social dataset), so any server path generating social-media sample data (e.g. `websocket-streaming-server`'s per-frame demo data) had a real chance of crashing the task before sending anything. `shares` now samples `0..=likes/3` (never empty); `views` now short-circuits to `0` when `likes == 0` instead of sampling an empty range
- `pjs-demo`'s shared `static/performance_comparison.html` (served as the index page of `interactive-demo-server` and `websocket-streaming-server`) no longer has a "Run Comparison" button — it called `runComparison()`, which fetched `GET /compare?dataset_type=...&size=...&network_type=...&enable_compression=...`. Neither server implements `/compare`; `interactive-demo-server` does implement a similarly-named `GET /performance`, but with a different path, different query parameters, and a different JSON response shape than this page's JS expected, so it could never have served this button correctly even by accident (see the `TODO(#409)` note added in #399). The button, `runComparison()`, `displayResults()`, `formatBytes()`, the results/loading DOM sections, their now-unused CSS, and the dataset/size/network/compression `<select>` controls that only fed that button have all been removed. The page is retitled "PJS Demo Server" and now lists the routes each of the two servers that share it actually implements, instead of leaving a bare, unlabeled header behind (#409)
- `pjs-wasm`'s `PjsParser.withSecurityConfig` and `PriorityStream.setSecurityConfig` took `SecurityConfig` by value on the Rust side. Under wasm-bindgen, passing a `#[wasm_bindgen]` struct by value into an exported function transfers ownership into Rust and invalidates the JS-side wrapper, so sharing one `SecurityConfig` instance across a parser and a stream — the exact pattern documented in the README's "Security Limits" section and the crate's module docs — threw `Error: null pointer passed to rust` on the second call. Both now take `&SecurityConfig` and clone internally; the generated TypeScript signatures (`withSecurityConfig(security: SecurityConfig): PjsParser`, `setSecurityConfig(config: SecurityConfig): void`) are unchanged, so this is not a breaking change for JS/TS consumers — the same `SecurityConfig` instance can now safely be passed to both calls (#404)
- The identical by-value ownership-transfer hazard also affected `PjsParser.withConfig` and `PriorityStream.withConfig`, which both took `PriorityConfigBuilder` by value — sharing one `PriorityConfigBuilder` between a parser and a stream (the pattern shown in the README's API reference for `withConfig`) hit the same `Error: null pointer passed to rust`. Both now take `&PriorityConfigBuilder`; `PriorityConfigBuilder` gained `#[derive(Clone)]` and its internal `build_internal` clones from the reference. Generated TypeScript signatures are unchanged, so this is likewise not a breaking change for JS/TS consumers (#404)
- `AlignedBuffer::reserve` (`crates/pjs-core/src/parser/buffer_pool.rs`) could pass an out-of-contract size to `std::alloc::realloc` from 100% safe code — undefined behavior reachable without any `unsafe` on the caller's side. The alignment-rounding step (`(new_capacity + alignment - 1) & !(alignment - 1)`) used unchecked `usize` addition, which could overflow and wrap to a small (including zero) value on inputs near `usize::MAX`, both bypassing an added `isize::MAX` bound check and reaching `realloc` with `new_size == 0` (itself a separate documented UB precondition). Both `AlignedBuffer::new` and `AlignedBuffer::reserve` now use `checked_add` before rounding, rejecting overflow with `DomainError::InvalidInput` instead of wrapping; `reserve` additionally rejects a zero or `isize::MAX`-exceeding rounded capacity, and now constructs the new `Layout` before calling the pointer-invalidating unsafe `realloc_aligned`, closing a secondary use-after-free window where a post-`realloc` layout-construction failure would leave `self.ptr` referencing an already-invalidated allocation. `AlignedAllocator::realloc_aligned`'s `# Safety` doc now states both preconditions explicitly (#402)
- `StreamSession::stats().total_bytes` was declared but never incremented anywhere, making `sort_by=total_bytes` on `GET /pjs/sessions` a silent no-op and `GetSystemStatsQuery`'s `total_bytes` always zero. `StreamSession::create_stream_patch_frames` and `create_priority_frames` now accumulate real, estimated payload bytes into `stats.total_bytes`, mirroring the existing `stats.total_frames` increments at the same call sites: `create_stream_patch_frames` takes a before/after delta of the child stream's own byte counter (already computed by `Stream::create_patch_frames`, no extra serialization); `create_priority_frames` sums `Frame::estimated_size()` over only the frames it retains after priority-based truncation. `GetSystemStatsQuery`'s `bytes_per_second` remains a separate, pre-existing issue (same class as #139/#136 — it sums a filtered/truncated session scan divided by full process uptime) and is not fixed by this change (#400)
- `pjs-demo`'s `simple-demo-server` `/pjs` endpoint now returns a real skeleton generated by `pjson_rs::PriorityStreamer::analyze`, instead of a hand-typed placeholder — the "infrastructure compilation" blocker that previously disabled `PriorityStreamer` usage no longer applies (#399)
- `pjs-demo`'s `websocket-streaming-server` `/metrics` endpoint now reports real `server_uptime_seconds` and `average_session_age_seconds`, instead of a hardcoded `server_uptime_seconds: 0` (#399)
- `pjs-demo`'s shared `static/performance_comparison.html` (served as the index page of `interactive-demo-server` and `websocket-streaming-server`) no longer auto-fetches `/compare` on page load — neither server has ever implemented that route, so every visitor to either server's homepage previously hit an immediate `alert()` error dialog on load. The "Run Comparison" button is left in place, marked `TODO(#409)`: clicking it still hits the same unimplemented route, but that's an opt-in failure rather than a forced one (#399)
- `pjs-core`'s `gat_performance_showcase` example no longer compares an unrelated no-op stub against real work, or prints unverified/false zero-allocation claims — `benchmark_response_creation` (which compared `create_streaming_response`'s literal-string stub against `create_health_response`'s real HashMap/String allocation) is removed, `benchmark_memory_allocation`'s "GAT with pooled objects" label and "Zero heap allocations for pooled responses" claim (there is no pooling on that path, and both `data.clone()` and the delegated `to_string()` allocate) are replaced with accurate wording, and several comments/println claims conflating a stack-allocated GAT `Future` type with a zero-allocation function body are reworded to describe only what's actually true (no `Box<dyn Future>` allocation) (#435)
- `pjs-core`'s `object_pool_performance` example's `benchmark_vec_allocation`/`benchmark_hashmap_allocation` loops no longer get dead-code-eliminated in release builds (both the standard and pooled allocation loops now pass their result through `std::hint::black_box`), and the printed "x faster" result now reflects the actual measured direction instead of always claiming the pooled path is faster even when it measured slower (#429)

### Changed

- `pjs-wasm`'s `PjsParser::generate_frames_internal` and `PriorityStream::generate_frames_internal` (and their `create_skeleton_with_depth`/`create_skeleton_with_limit` skeleton-building helpers) duplicated the identical frame-generation algorithm verbatim. Both now delegate to a shared `frame_generation::generate_frames`/`build_skeleton` implementation, and the old per-struct skeleton helpers (including the already-dead `PjsParser::create_skeleton` wrapper) were removed entirely; behavior and public API (`generateFrames`, `start`) are unchanged (#433)
- `pjs-demo`'s `interactive-demo-server` `/pjs-streaming` endpoint's `enable_streaming` query parameter is now wired up and authoritative when explicitly set (overriding `Accept`-header sniffing in either direction), instead of being an inert, dead-code field; both this endpoint and `/api/info` now document that it only requests the skeleton-only response — full SSE/chunked delivery is tracked separately (#163) (#399)
- **BREAKING** `SearchSessionsQuery.filters.state` (`application::queries::SessionFilters::state`) is now typed as the domain `SessionState` enum instead of an unvalidated `String` — a typo like `state: "activ"` previously matched zero sessions silently instead of being rejected. The HTTP `GET /pjs/sessions/search?state=` query parameter now requires one of `SessionState`'s exact serialized spellings (`Initializing`, `Active`, `Closing`, `Completed`, `Failed`) and rejects anything else with `400` and the API's standard `{"error": ...}` JSON body — **this includes previously-working lowercase or mixed-case values** (e.g. `?state=active`, `?state=COMPLETED`), which matched case-insensitively under the old string-based repository comparison and now fail. `?state=` (present but empty) also now rejects with `400` instead of silently matching zero sessions (#414)
- `pjs-js-client`'s `core/client.ts` no longer casts `frame` to `any` when calling `reconstructor.processSkeleton`/`applyPatch` or reading `.patches`/`.total_frames` — each call site is already narrowed to `SkeletonFrame`/`PatchFrame`/`CompleteFrame` by the preceding `frame.type === FrameType.X` check, so the casts discarded the `Frame` discriminated union's compile-time type safety for no reason. `core/frame-processor.ts`'s `FrameProcessor.validateFrame`/`processFrame` parameter type is tightened from `any` to `unknown`, since both are genuine validation boundaries for untrusted input; body-level type checking inside both functions is restored via a single scoped cast at the point the shape is asserted. No behavior change (#416)
- `pjs-js-client`'s `core/frame-processor.ts` `FrameProcessor.validatePatchOperations` parameter type is tightened from `PatchOperation` to `unknown` — the value reaching it, `Array.isArray`-narrowed `candidate.patches`, was `any[]` one line after the `unknown` boundary #416 established, making the function's own runtime guards dead code from TypeScript's perspective. A single `Record<string, unknown>` cast (the same pattern already used in `validateFrame`) restores body-level type checking. `core/json-reconstructor.ts`'s `JsonReconstructor.processSkeleton` no longer casts `frame as any` before reading `.data` — the cast was already redundant since `SkeletonFrame.data` is typed `any`. Follow-up to #416; no behavior change (#423)
- **BREAKING** `CommandValidator`'s three `validate_*` methods (`application::handlers::command_handlers`) were defined but never invoked from anywhere — not from `SessionCommandHandler::handle`, not from any test — so commands reached the domain layer unvalidated and the struct misleadingly implied an application-boundary check that did not exist. All three are now called as the first statement of the corresponding `SessionCommandHandler::handle` implementation, before the session is loaded, returning `ApplicationError::Validation` (already mapped to HTTP `400`) on failure. **Breaking:** requests that previously succeeded or failed differently now return `400`: `POST /pjs/sessions` with `max_concurrent_streams: 0` or `timeout_seconds: 0` (previously accepted with `201`, then every subsequent create-stream failed with `DomainError::TooManyStreams` → HTTP **500**, or the session was already expired on arrival); `POST .../streams` with a `null` payload; and `POST .../streams/{id}/generate-frames` with `max_frames: 0` (previously a silent empty `200`) or `max_frames > 1000` (previously unbounded on explicit values, contradicting `GenerateFramesRequest`'s doc claim that `max_frames` is bounded). `BatchGenerateFramesCommand::max_frames` is deliberately left unvalidated — `StreamSession::create_priority_frames` already bounds its output by truncation, with no capacity-based allocation. `validate_create_session` also now rejects `timeout_seconds` above a new `MAX_SESSION_TIMEOUT_SECONDS` (`domain::config::limits`, 7 days) — previously, `StreamSession::with_time_provider`'s `now + chrono::Duration::seconds(session_timeout_seconds as i64)` could panic on a `timeout_seconds` whose `as i64` cast overflowed `chrono::Duration::seconds`'s valid range (a per-request panic DoS reachable from `POST /pjs/sessions`), or silently wrap to a negative duration on `u64::MAX` and create a session already expired on arrival regardless of `timeout_seconds`'s literal value (#438)

### Removed

- **BREAKING** `pjson-rs`'s `HttpEventPublisher` (`infrastructure::adapters::event_publisher`) and the `http-client` Cargo feature (was default-on) that gated it — zero production constructors anywhere in the codebase, only reachable from `#[cfg(feature = "http-client")]`-gated code with no caller. Removing it also drops `reqwest` (and the transitive `rustls-platform-verifier`/`webpki-root-certs`, along with the now-unused `CDLA-Permissive-2.0` license allowance in `deny.toml`) from the default dependency graph. **Breaking:** `pjson_rs::infrastructure::HttpEventPublisher` and `pjson_rs::infrastructure::adapters::HttpEventPublisher` no longer exist; a downstream `Cargo.toml` declaring `features = ["http-client"]` now fails to compile with "feature does not exist" instead of silently doing nothing (#436)
- `pjs-demo`'s `crates/pjs-demo/src/servers/performance_comparison.rs` — dead source with no `[[bin]]` entry and no `mod` declaration, never compiled by any build, referencing APIs (`StreamProcessor::process_json`) that no longer exist and a request schema mismatched with its own front-end HTML. Follow-up work originally scoped to it (`estimate_priority_distribution`, tracked as #405) no longer applies to any running server (#399)
- **BREAKING** `pjson-rs`'s `application::dto::{ToDto, FromDto}` traits and their `Priority`/`Id<T>` implementations — they duplicated the standard `From`/`TryFrom` impls already defined on the same DTO types for no added behavior. Callers of `.to_dto()` migrate to `.into()`; callers of `.from_dto(x)` migrate to `x.try_into()` (`Priority`) or `x.into()` (`Id<T>`, which cannot fail) (#413)
- `pjs-bench`'s `fallback` module (`StreamerConfig`, `StreamFrame`, `PriorityStreamer` stubs whose `with_priority_threshold`/`with_memory_limit` builders silently discarded their argument), `BenchSuite`, and `BenchMetrics` — none of the four real `benches/*.rs` files consumed them, or anything else from the `pjs_bench` lib crate; their only callers were the crate's own `#[cfg(test)]` tests. Rust's "explicit imports shadow glob imports" rule meant the explicit `pub use pjson_rs::{..., StreamFrame, ...}` silently took precedence over the wildcard `pub use fallback::*;`'s own, unrelated `fallback::StreamFrame` — the two were ~170 lines apart, not adjacent. `pjs-bench/src/lib.rs`'s `pub use pjson_rs::{...}` re-export is removed for the same reason (unused — benches import from `pjson_rs`/`pjson_rs_domain` directly), and the now-unused `bytes` dependency is dropped from `pjs-bench/Cargo.toml`. Also removed the dead `features::has_streaming()` helper and the `streaming`/`default` Cargo features it reflected (same shape as the sibling `features::has_simd()` removal in #324) — `cfg!(feature = "streaming")` had no other reader and `streaming` gated nothing else. Module doc examples in `lib.rs` now reference the real `[[bench]]` target names (`cargo bench -p pjs-bench --bench <name>`) instead of filter strings that mostly matched no benchmark ID (#419)
- **BREAKING** Removed the entire `memory` module (`memory::arena` — `StringArena`, `ValueArena<T>`, `JsonArena`, `ArenaStats`, `CombinedArenaStats`) along with the crate-root re-exports `pjson_rs::{JsonArena, CombinedArenaStats}` and the `typed-arena` dependency. None of these types was constructed anywhere in production code — only in the module's own unit tests — and the actual hot-path allocator is the separate `parser::buffer_pool::{AlignedBuffer, BufferPool}`, which is unaffected. The module survived as dead public API that implied an arena-backed parsing path the crate does not have (#430)

## [0.6.3] - 2026-08-18

### Security

- WebSocket outgoing-message channels (server per-connection channel, client outgoing channel, `InMemoryEventPublisher::with_channel`) now additionally bound cumulative queued bytes via a new `infrastructure::bounded_channel::byte_bounded_channel`, on top of the existing message-count bound. Previously, a slow consumer combined with large individual messages could queue far more memory than the message-count bound alone implied — e.g. `1000 * 16 MiB` ≈ 16 GiB worst-case per WebSocket connection at default limits. Each channel now also rejects (drops, logging) a send once its byte budget is exceeded, keeping worst-case queued memory a small, predictable constant; a payload larger than the channel's *entire* budget is rejected immediately rather than being handed to the underlying `Semaphore` (which would wait forever for permits that can never all become available, permanently starving the channel). `InMemoryEventPublisher::with_channel` and its `StoredEvent` receiver items are wrapped (`Envelope<StoredEvent>`, transparently `Deref`s to `StoredEvent`) as part of this — see the `Changed` entry below (#349)
- `AxumWebSocketTransport::upgrade_handler` now configures axum/tungstenite's transport-level `max_message_size`/`max_frame_size` from the transport's `RateLimitConfig::max_frame_size` (previously left at axum's defaults of 64 MiB/16 MiB). An oversized frame is now rejected during frame assembly instead of being fully buffered first and only rejected afterward by the application-level `check_message` call, which remains as defense-in-depth. `WebSocketRateLimiter::config()` is a new accessor exposing the configuration needed to wire this up (#334)
- Outbound WebSocket sink writes now have a write deadline via a shared `infrastructure::websocket::send_with_write_timeout` helper, applied at every server and client write site — including the three rate-limit-rejection `Close` writes, which sit on the same adversary-controlled (slow-reading peer) path this fix targets. Previously, a peer that stopped reading could block a write indefinitely (the TCP send buffer never drains), wedging the connection's task — and, on the server, the `Arc<RateLimitGuard>` it holds — until the OS eventually timed out the socket. A stalled write now closes the connection instead. The deadline is `RateLimitConfig::write_timeout` on the server (new field, default 10s) and a fixed `infrastructure::websocket::WRITE_TIMEOUT` constant (10s) on the client. This bounds a single write, not throughput, so a legitimate large frame sent to a genuinely slow client can hit the same deadline as a stalled one — see `WRITE_TIMEOUT`'s doc for the reasoning; raise `write_timeout` if a deployment's expected client bandwidth needs more headroom (#325)
- HTTP rate limiter (`RateLimitMiddleware`) no longer trusts `X-Forwarded-For`/`X-Real-IP` from arbitrary clients — it keys on the real TCP peer address via axum's `ConnectInfo<SocketAddr>` by default, closing a bypass where a client could send a fresh spoofed header on every request to obtain a fresh rate-limit bucket. Proxy-header support is now opt-in via `RateLimitConfig::with_trusted_proxies`, which only honors the forwarded headers when the real peer address is in an explicit allowlist. `X-Forwarded-For` is read across all header lines with that name and parsed right-to-left, skipping entries that are themselves trusted proxies, so a client sitting behind a trusted proxy cannot forge a fresh bucket by varying the client-supplied (leftmost) entry or by adding its own header line; the walk fails closed (falls back to `X-Real-IP`, then the peer address) on the first unparseable entry rather than skipping past it. Allowlist and peer comparisons canonicalize IPv4-mapped IPv6 addresses so a dual-stack listener still matches a plain-IPv4 allowlist entry. A one-time warning is logged if a request has no `ConnectInfo` extension, since that otherwise silently collapses every client onto a single shared bucket (#336)
- `pjs-wasm`'s `SecurityConfig::setMaxArrayElements`/`setMaxObjectKeys` limits were never enforced — only the total input-byte-size check ran, so a caller who configured a tight element/key limit believing it was protected still had the full array/object walked, cloned, and re-serialized regardless of its size, up to the byte-size ceiling. `PjsParser::parse`, `PjsParser::generateFrames`, and `PriorityStream::start` now validate array/object element counts at every nesting level of the parsed JSON, before the more expensive `JsonData` conversion, closing the gap. Corrected `SECURITY.md`'s reference to a non-existent `.setMaxArraySize()` method (the real name is `.setMaxArrayElements()`) and clarified that the JSON-depth limit truncates rather than rejects, unlike the array/object-count limits (#326)
- `pjs-js-client`'s transitive devDependencies `brace-expansion` (GHSA-rgw5-rvv9-x895, DoS via unbounded intermediate arrays, pulled in via `eslint` → `minimatch`) and `js-yaml` (GHSA-5p4m-2wfm-xmqj, quadratic CPU consumption in `!!omap` resolution, pulled in via `ts-jest` → `@jest/transform` → `babel-plugin-istanbul` → `@istanbuljs/load-nyc-config`) were bumped to 5.0.9 and 4.3.1 respectively, resolving both high-severity advisories reported by `npm audit`. `brace-expansion` floated up naturally via the lockfile; `js-yaml` is held at 4.3.1 via npm override, since `@istanbuljs/load-nyc-config` still declares `^4.0.0`. Both are dev/build-toolchain-only (eslint and jest chains), not reachable from the published package output (fixed in #343, closes #337)
- `GET /metrics` no longer leaks the Prometheus recorder installation error's `Display` text in the response body on failure — the endpoint is unauthenticated, so any caller could previously read internal error details from `metrics_exporter_prometheus`. The handler now returns a fixed generic body and logs the real error server-side via `tracing::error!`, once per process, to avoid flooding logs on the unauthenticated, unrate-limited route if the failure persists (#323)
- Migrated the `gat_port!` macro's proc-macro dependency from `paste` (unmaintained, RUSTSEC-2024-0436) to its maintained fork `pastey`, and removed the now-unnecessary advisory ignore entries from `deny.toml` and `osv-scanner.toml`. `gat_port!` is `#[macro_export]`ed for external consumers, so its expansion previously used an absolute `::paste::` path, which failed to compile for external callers without a direct `paste` dependency — a pre-existing bug fixed as part of this migration by resolving the macro through a new `#[doc(hidden)] pjson_rs::__pastey` re-export (`$crate::__pastey::paste!`) instead (#312)
- `GET /pjs/sessions/{session_id}/dictionary` and `install_global_recorder` (the latter is `pub` on `pjson-rs`, so any caller relying on it for its own HTTP responses inherited the same bug) both embedded a wrapped store/build error's `Display` text verbatim into `PjsError::HttpError`, which is served as-is in the JSON error body — leaking internal error details to the HTTP client. Both now log the real error server-side via `tracing::error!` and return a fixed, generic message instead; `install_global_recorder`'s log is guarded by a `std::sync::Once` (mirroring #323's fix), since the underlying `OnceLock` re-runs the install attempt — and would otherwise re-log at ERROR — on every call while it stays empty on failure, and `metrics_handler` calls it on every request to the unauthenticated, unrate-limited `/metrics` route. `PjsError::HttpError`'s doc comment now states the invariant explicitly — the wrapped string must never carry a foreign error's `Display` text at any construction site reachable while handling a request — naming both known leak channels (`IntoResponse for PjsError`, and handlers like `metrics_handler` that build a response body directly) and the intentional exemption (`build_cors_layer`'s errors, which only ever run at router-build time from operator config and can never reach a request response) (#376)

### Changed

- **BREAKING** Consolidated the two divergent `JsonPath` types (a validated string newtype in `pjson-rs-domain` and a segmented type in `pjson-rs`) into a single canonical `JsonPath { segments: Vec<PathSegment> }` living in `pjson_rs_domain::value_objects`, and re-exported from `pjson-rs`'s `domain` module and crate root alongside the now-also-exported `PathSegment`. `pjson_rs::stream::priority::{JsonPath, PathSegment}` no longer exist; update imports to `pjson_rs_domain::value_objects::{JsonPath, PathSegment}` (or the `pjson_rs` re-export). `PathSegment::Root` and `PathSegment::Wildcard` are removed — the root path is now represented as the empty segment sequence, and `Wildcard` was constructed nowhere in the workspace. `PathSegment` is `#[non_exhaustive]` (carried over from the `pjson-rs-domain` copy that survived the merge; `pjson-rs`'s now-deleted copy was not), so external code matching on it exhaustively must add a wildcard arm. `JsonPathDto` (`pjson-rs`, `application::dto`) is deleted entirely; `JsonPath` now has its own hand-written `Serialize`/`Deserialize` (as the `Display` string), making the DTO redundant. `JsonPath::as_str`, `len`, and `is_empty` are removed (`depth()`/`segments().is_empty()` cover the same information); `last_segment()` and `last_key()` now return borrows (`Option<&PathSegment>`/`Option<&str>`) instead of owned values; `from_segments` is now fallible (`DomainResult<Self>`), validating every `Key` segment. Path validation is widened to a single rule shared by `new`/`FromStr`, `append_key`, and `from_segments`: a key is valid iff it is non-empty and contains none of `.`, `[`, `]` (previously `new`/`FromStr` additionally rejected any key with punctuation or whitespace beyond `[A-Za-z0-9_-]`, while `append_key` already accepted such keys — the three constructors now agree). `PriorityStreamFrame`'s `path` field on the SSE endpoint now serializes as the string `"$.a[0]"` instead of the structural `{"segments":[...]}` it previously emitted, aligning with `pjs-js-client`'s `type JsonPath = string` and with `FramePatch.path`'s pre-existing string wire format (unchanged). Pre-1.0 breaking change; no deprecation cycle (#379)
- `SessionCommandHandler`'s six stream/frame-lifecycle command handlers (`CreateStreamCommand`, `StartStreamCommand`, `CompleteStreamCommand`, `GenerateFramesCommand`, `BatchGenerateFramesCommand`, `CloseSessionCommand`) each repeated the same load-session and save-and-publish-events boilerplate around their one distinct domain mutation. Extracted into private `load_session`/`save_and_publish` helpers; pure refactor, no behavior change (#392)
- **BREAKING** `RateLimitError` (public on `pjson-rs`) gained a new `CapacityExceeded { max: usize }` variant as part of the #346 fix below. `RateLimitError` is not `#[non_exhaustive]`, so this breaks any external code exhaustively matching on it without a wildcard arm
- **BREAKING** Removed the `simd-avx2`, `simd-neon`, and `simd-sse42` Cargo features from `pjson-rs`. All three compiled to identical behavior as `simd-auto` — they only ever gated the same coarse SIMD-enable switch in `crates/pjs-core/build.rs` (which sets the sonic-rs backend on or off), never a distinct per-ISA code path, and their README documentation incorrectly implied each one "forced" a specific instruction set. Builds passing `--features simd-avx2`/`simd-neon`/`simd-sse42` explicitly now fail with an unknown-feature error instead of silently accepting a redundant flag. Use `simd-auto` (default, runtime-dispatches to the best available instruction set; x86_64 and aarch64) or `simd-avx512` (x86_64-only, also forwards to `sonic-rs/avx512`) instead. `crates/pjs-bench`'s own disconnected copies of these features, along with the dead `features::has_simd()` helper that only ever reflected them, were also removed (closes #324)
- `StreamSession` now sources "now" through the existing `TimeProvider` port (`domain::ports::TimeProvider`) instead of calling `chrono::Utc::now()` directly in `new`, `is_expired`, `force_close_expired`, `extend_timeout`, and every event-timestamp call site — the port existed but was never wired into the aggregate it was built for (#330)
- New `StreamSession::with_time_provider` constructor accepts an `Arc<dyn TimeProvider>` for callers that want a fake clock; `StreamSession::new` is unchanged and still defaults to `SystemTimeProvider`, so no existing caller needs to change (#330)
- `StreamSession` no longer derives `Debug` (its new `dyn TimeProvider` field can't derive it) — it now has a manual `Debug` impl producing the same field output, with the time-provider field omitted via `finish_non_exhaustive()` (#330)
- The new `time_provider` field is skipped on `Serialize`/`Deserialize` and restored to `SystemTimeProvider` on deserialize, matching prior behavior (`StreamSession` was never actually serialized with a clock identity to begin with) (#330)
- Nine timing-sensitive tests in `stream_session_comprehensive.rs` that previously used `std::thread::sleep` to make `Utc::now()` advance (10ms-1100ms delays) now drive a fake `TimeProvider` directly, making those assertions deterministic instead of load-dependent (#330)
- **BREAKING** `infrastructure::http::axum_adapter.rs` (1891 lines, mixing session CRUD, stream lifecycle, health/metrics, and dictionary route handlers in one file) split its route handler `async fn`s into `infrastructure::http::handlers::{sessions,streams,health,dictionary}`, grouped by the domain concept each serves. The public module path `infrastructure::http::dictionary::get_session_dictionary` moved to `infrastructure::http::handlers::dictionary::get_session_dictionary` as part of this split, breaking any external code importing it by that path. `axum_adapter.rs` retains router assembly, `PjsAppState`, request/response DTOs, and `PjsError`/`IntoResponse` mapping; `PjsAppState`'s fields are now `pub(crate)` (previously private) so the relocated handlers can reach them. Otherwise a pure reorganization — route paths, handler behavior, and request/response shapes are unchanged (#316)
- **BREAKING** Renamed two unrelated, same-named public enums in `infrastructure::http` that forced callers to disambiguate by fully-qualified path: `axum_extension::StreamError` is now `StreamExtensionError`, and `streaming::StreamError` (including its `infrastructure::http` re-export) is now `StreamTransportError`. No variants or behavior changed (#321)
- **BREAKING** Dropped the redundant `get_` prefix from public getters across `pjson-rs-domain` and `pjson-rs`, per Rust API Guidelines C-GETTER: `Frame::get_metadata` (renamed to `metadata_value` instead of `metadata`, since `Frame::metadata` already returns the full map), `JsonData::get_path` → `path`, `StreamSession::get_stream` → `stream`, `LazyArray::get_parsed` → `parsed`, `StreamingCompressor::get_stats` and `StreamingDecompressor::get_stats` → `stats`, `SonicParser::get_stats` → `stats`, `WebSocketRateLimiter::get_stats` → `stats`, `InMemoryMetricsCollector::get_performance_snapshot`/`get_session_metrics`/`get_stream_metrics`/`get_time_series` → `performance_snapshot`/`session_metrics`/`stream_metrics`/`time_series`, `UniversalRequest::get_header`/`get_query` → `header`/`query`, `SecureWebSocketHandler::get_security_stats` → `security_stats`. `BufferPool::get_buffer`/`get_buffer_with_capacity` deliberately became `acquire`/`acquire_with_capacity` rather than the bare `buffer`/`buffer_with_capacity` the prefix-strip would suggest: these methods run security/memory-limit validation, mutate allocation/cache-hit counters, and fallibly acquire a pooled resource — not field accessors C-GETTER governs — and `buffer` would also collide in meaning with the genuine getter `PooledBuffer::buffer() -> Option<&AlignedBuffer>` on the value they return. Pre-1.0, so no deprecation shims — call sites must update directly (#322)
- **BREAKING** `pjson_rs::infrastructure`'s public module tree — `infrastructure` itself and its `adapters`, `http`, `integration`, and `websocket` submodules — now re-exports explicit named items instead of `pub use module::*` wildcards, closing an API-auditability gap (self-documenting module boundary, safer semver diffing). Wildcard re-exports also flatten a target module's `pub mod` children, not just its items, so this narrowing additionally drops 20 flattened public module paths that previously resolved through `infrastructure::`: `adapters::*` used to flatten `event_publisher`, `frame_store`, `gat_memory_repository`, `generic_store`, `json_adapter`, `limits`, and `metrics_collector` (e.g. `infrastructure::limits::MAX_SCAN_LIMIT` no longer resolves; use `infrastructure::adapters::limits::MAX_SCAN_LIMIT`); `http::*` used to flatten `auth`, `axum_adapter`, `axum_extension`, `metrics`, `middleware`, and `streaming` (e.g. `infrastructure::axum_adapter::PjsAppState` no longer resolves; use `infrastructure::http::axum_adapter::PjsAppState`); `integration::*` used to flatten `streaming_adapter`, `universal_adapter`, `object_pool`, and `simd_acceleration` (e.g. `infrastructure::universal_adapter::UniversalAdapter` no longer resolves; use `infrastructure::integration::universal_adapter::UniversalAdapter`); `websocket::*` used to flatten `client`, `security`, and `server` (e.g. `infrastructure::security::...` no longer resolves; use `infrastructure::websocket::security::...`). All items previously reachable through these wildcards remain reachable at the same root re-export path as before — only the flattened module paths are gone; the canonical fully-qualified paths shown above always worked and still do. Pre-1.0 breaking change; no deprecation cycle (#313)
- Extracted `parse_session_id`/`parse_session_and_stream_id` helper functions (now `pub(crate)` in `axum_adapter.rs`, used by the relocated `handlers::sessions`/`handlers::streams` modules from #316 above) to deduplicate repeated `SessionId`/`StreamId` parse-and-map-error logic across 8 HTTP handler call sites; no behavior change (#320)
- **BREAKING** `InMemoryEventPublisher::with_channel` now returns `mpsc::Receiver<Envelope<StoredEvent>>` instead of `mpsc::Receiver<StoredEvent>`, as part of the #349 byte-bounded-channel fix above. `Envelope<T>` transparently `Deref`s to `T`, so field access and reads through the received value are source-compatible; call `Envelope::into_inner()` to obtain an owned `StoredEvent`. Pre-1.0 breaking change; no deprecation cycle (#349)
- **BREAKING** `RateLimitConfig` (public on `pjson-rs`, `security::rate_limit`) gained a new `pub write_timeout: Duration` field as part of the #325 write-timeout fix above. `RateLimitConfig` is not `#[non_exhaustive]`, so this breaks external code constructing it via struct literal without `..Default::default()`; it also means a serialized `RateLimitConfig` missing `write_timeout` now fails to deserialize (no `#[serde(default)]`, consistent with this struct's other fields). Pre-1.0 breaking change; no deprecation cycle (#325)
- **BREAKING** `StoredEvent` (public on `pjson-rs`, re-exported from `infrastructure::adapters`) gained a new `pub sequence: u64` field as part of the #350 eviction-order fix above. `StoredEvent` is not `#[non_exhaustive]`, so this breaks any external code constructing it via struct literal; reading fields or matching on existing ones is unaffected. Pre-1.0 breaking change; no deprecation cycle
- `pjs-js-client`: `WasmBackend`'s synthetic `Complete` frame now reports `priority: Priority.Background` (10) instead of the previously hardcoded `1`, which was not a valid `Priority` enum member. This is a disclosed value change on that frame's `priority` field for consumers reading it directly; nothing in `pjs-js-client` itself gates behavior on it
- `pjs-js-client`: `StreamStats.priorityDistribution` is now typed `Partial<Record<Priority, number>>` instead of `Record<Priority, number>` — the previous type was unsound (only priorities actually seen are ever populated) and could not be satisfied by an empty initial value. TypeScript consumers indexing this field now get `number | undefined`
- **Breaking**: dictionary substitution no longer replaces a string with a bare `JsonValue::Number` index tracked by a separate field-path list (`dict_paths`). It now encodes a self-describing sentinel-escaped string marker (`"\u{7F}<index>"`, with sentinel-doubling to escape any payload string that legitimately starts with `\u{7F}`) directly in place of the string. This closes two classes of data corruption by construction rather than by narrowing: a substituted value is structurally never a number (closing the number/index collision) and decoding needs no positional metadata at all (closing the path-string collision, e.g. a payload containing both `{"a":{"b":0}}` and a sibling `"a.b"` key). `DecompressionMetadata::dictionary_paths` and the corresponding `dict_paths` compression-metadata entry are removed (#333)
- **Breaking**: dictionary metadata is now transmitted as a single `"dict"` key holding an index-ordered JSON array of strings, replacing the previous scattered `"dict_0"`, `"dict_1"`, ... keys
- **Breaking**: `CompressionConfig::string_dict_threshold` and `CompressionConfig::min_absolute_savings` are removed, replaced by `CompressionConfig::min_net_savings: usize` (default `10`; `PjsConfig::mobile()` preset `4`). A candidate dictionary entry is kept only when its modelled `gain - cost` (in wire bytes, using the same accounting `compressed_size` reports) is positive, and `CompressionStrategy::Dictionary`/the dictionary half of `Hybrid` is selected only when the summed net saving across all kept entries clears `min_net_savings`
- `SchemaAnalyzer::total_string_bytes` (an internal ratio-normalization field, now unused) is removed
- Dictionary index assignment is now deterministic (candidates sorted descending by `count * length`, lexicographic tie-break) instead of iterating a `HashMap`, so compressed output bytes are reproducible across runs on identical input
- Added `CompressionConfig::validate()`, called from `PjsConfig::validate()`, validating the potential/ratio fields (`uuid_compression_potential`, `min_delta_potential`, `min_compression_potential`) are in `0.0..=1.0` and the threshold fields (`delta_threshold`, `run_length_threshold`) are finite and non-negative
- Sorted `[workspace.dependencies]` in the root `Cargo.toml` alphabetically; no version, feature, or attribute changes (closes #380)

### Added

- `PjsWebSocketClient::with_write_timeout`, a builder-style method letting callers override the client's per-write deadline (previously always the fixed `infrastructure::websocket::WRITE_TIMEOUT` constant, 10s) — either shorter, to free a wedged `send_task` faster in resource-constrained deployments, or longer, for large payloads over slow uplinks. Defaults to the existing constant, so default behavior is unchanged for callers that don't opt in (#358)

### Fixed

- `QueryHandlerGat<SearchSessionsQuery>` loaded every active session into memory via `find_active_sessions()` and applied filtering, sorting, and pagination in-process — the same unbounded-scan anti-pattern #136 already fixed for `GetActiveSessionsQuery`. It now builds a `SessionQueryCriteria`/`Pagination` from `SearchSessionsQuery`'s filters/sort options and delegates to `StreamRepositoryGat::find_sessions_by_criteria`, which the in-memory repository already bounds via `MAX_RESULTS_LIMIT`/`MAX_SCAN_LIMIT`. `SessionQueryCriteria` gained a new `exclude_expired` field so the default (no `state` filter) request continues to match only active, non-expired sessions — the same scope `find_active_sessions()` provided (`state == Active && !is_expired()`) — instead of silently widening to every session state; an explicit `filters.state` still opts out of that default and matches only the requested state. `ALLOWED_SORT_FIELDS` and the repository's `compare_by_field` gained `total_bytes` to keep `SessionSortField::TotalBytes` sortable through the shared pagination path. `GET /pjs/sessions/search`'s `limit`/`offset` query parameters are now clamped (`limit` to `[1, 100]`, `offset` to `MAX_PAGINATION_OFFSET`) before being handed to `Pagination`, so a client-supplied `?limit=0` or an out-of-range `?offset=` can no longer trip `Pagination::validate()`'s rejection and surface as an HTTP 500 — both now return a normal (possibly empty) page instead. The now-unused in-process `matches_filters` helper was removed; its removal also changed `filters.state` matching from a Debug-format substring match (e.g. `"e"` matched both `Active` and `Completed`) to an exact, case-insensitive match against `SessionState::as_str()`, consistent with `GetActiveSessionsQuery`'s existing repository-level matching (#391)
- `GatStreamingOrchestrator::process_concurrent_streams` (`domain::services::gat_orchestrator`) was documented and named as processing streams concurrently via GAT futures, but actually awaited them one at a time in a plain `for` loop. `stream_with_priority` only borrows `&self`, so nothing prevented real concurrency; it now uses `futures::future::try_join_all` to poll every stream's future together, still bounded by `OrchestratorConfig::max_concurrent_streams` (input beyond this count is truncated, not queued — unchanged from before). `futures` is now a required dependency of `pjson-rs` instead of being pulled in only via the `http-server` feature, since this domain-layer module compiles unconditionally and must not depend on an HTTP-transport feature flag. Note that `StreamingStats::processing_time` is measured over the whole `stream_with_priority` call (session lookup, frame processing, and event publish), so when streams run concurrently through `process_concurrent_streams`, each stream's reported value now approximates the whole batch's elapsed time rather than that stream's individual share of it — see the field's doc comment (#393)
- `PriorityStreamer::analyze()`'s `extract_patches` emitted a `Set` patch carrying an object field's full cloned value, then unconditionally recursed into that same value — so a non-empty array nested inside an object also got an `Append` patch for the same elements, and `JsonReconstructor` applied both, duplicating every such array on reconstruction (e.g. `{"items":[1,2,3]}` round-tripped to `{"items":[1,2,3,1,2,3]}`). `Set` patches now carry an array-emptied skeleton of the value instead of the full clone (`skeletonize_arrays`), with the array's own `Append` patch supplying the data, and are priority-hoisted to the highest `Append` priority anywhere in their subtree so a `Set` can never sort/apply after an `Append` it must precede. `fix`-scoped change; wire format unchanged (#394)
- `pjs-demo`'s `websocket-streaming-server` bin (`websocket_streaming.rs`) registered its WebSocket route at the literal path `/ws`, but `POST /stream`'s response (`websocket_url`) and the startup log both advertised `ws://127.0.0.1:3001/ws/{session_id}` — any client connecting to the URL it was actually given got a 404 on the upgrade handshake, since no route matched. The route now accepts `/ws/{session_id}` and binds the upgrade to that path-bound session: the handler parses and atomically claims `session_id` against the same session map `POST /stream` populates via `AppState::add_session` (now `pub`), rejecting the upgrade with `400 Bad Request` for a malformed identifier, `404 Not Found` for one unknown to the server, or `409 Conflict` if another connection already claimed it (sessions are single-owner, not fan-out — the claim and the existence check happen under one lock hold via the new `AppState::try_bind_session`, closing a TOCTOU window where two concurrent connects for the same session could otherwise both attach a stream). Previously `handle_websocket` silently minted its own unrelated, unbound `SessionId` regardless of what the client connected to. The router construction was extracted into a new `pub fn app(state: AppState) -> Router` (and the bin's `main` into `pub async fn run()`), both now reachable from the crate's library target (`pjs_demo::servers::websocket_streaming`, wired up via a new `src/bin/websocket_streaming_server.rs` entry point), so the fix has integration coverage in the new `crates/pjs-demo/tests/ws_upgrade.rs` (previously `pjs-demo` had no test infrastructure at all) (#382)
- `pjs-js-client`: `WasmBackend.connect()`/`WasmParser.initialize()` still failed to load `pjs-wasm` in Node.js after #317/#351's fix, because `loadWasmModule()` used a real ESM `import('pjs-wasm')` — `pjs-wasm/pkg` (built via `wasm-pack build --target web`) is `"type": "module"`, and this project's Jest setup runs without `--experimental-vm-modules` (the rest of the suite relies on Jest's ambient `jest`/`describe`/`expect` globals, which only exist under real ESM VM-module mode), so Jest's dynamic `import()` routed the package through its CommonJS loader and threw `SyntaxError: Unexpected token 'export'`. `loadWasmModule()`'s Node path now reads the glue file's source directly and evaluates it as a CommonJS module instead of using `import()` — the same code path runs under both Jest and plain Node.js scripts, and also shims `TextEncoder`/`TextDecoder` (used unconditionally by the glue at module-evaluation time but absent from jsdom, this project's Jest `testEnvironment`) via `require('util')`. `crates/pjs-js-client/tests/integration/wasm-backend.test.ts` also switched its direct `await import('pjs-wasm')` calls to `loadWasmModule()`, since Jest's broken dynamic import affected those call sites the same way. `.github/workflows/ci.yml`'s `js-client-test` job never downloaded the `wasm-build-web` artifact before running `npm test`, unlike `wasm-types-drift`/`wasm-package-validation`; the integration test files guard themselves with an `existsSync` check on `crates/pjs-wasm/pkg` and silently *skip* rather than fail when it's absent, which is why this went uncaught by CI through two previous fix attempts. `js-client-test` now depends on `wasm-build` and downloads `wasm-build-web` into `crates/pjs-wasm/pkg` first. This surfaced a second, GitHub-Actions-level bug: `wasm-build`'s own trigger condition already covered `changes.wasm`, but its `needs: [changes, quality]` meant it was still skipped on JS-only PRs anyway, because `quality` is deliberately skipped there (`rust == 'false'`) and a job whose `needs` includes a skipped job is itself skipped regardless of its own `if`. `wasm-types-drift` and `wasm-node-example` already had a `changes.js` clause in their own `if` for exactly this reason, but silently inherited the same skip via their `needs: [changes, wasm-build]` — so they were never actually validating JS-only PRs either, just silently green instead of failing. `wasm-build`'s trigger condition gained `changes.js` and its `needs` dropped `quality` (no data dependency; it was purely a scheduling gate that only worked because every *other* job depending on it shares `quality`'s rust-only trigger scope) — `js-client-test`, `wasm-types-drift`, and `wasm-node-example` all now actually run on JS-only PRs instead of silently skipping. `.github/workflows/ci.yml` was also added to the `changes` path filters so edits to this file trigger the jobs it affects. `wasm-package-validation` was not affected by any of this — its own `if` has no `changes.js` clause, so it was never expected to run on a JS-only PR to begin with. Fixing the loader ran these two integration test files against real WASM for the first time (no CI job had ever downloaded/built the artifact before), which surfaced that `wasm-backend.test.ts`'s repeated `(await import('pjs-wasm')).PriorityStream` pattern never actually mocked anything (`WasmBackend.startStream()` constructs its own independent `PriorityStream` internally); this is now a working mock via `installMockPriorityStream()`, which replaces the `PriorityStream` property on the shared, cached module object `loadWasmModule()` returns so the instance the backend constructs internally is the one under test. Also fixed: `should report WASM version` asserted a hardcoded stale `'0.1.0'` against the real `0.6.2`, now read from `pjs-wasm/pkg/package.json` at test time; `jest.spyOn(global, 'import')` isn't a valid Jest API, replaced with `jest.spyOn(wasmLoader, 'loadWasmModule')`; two tests' mocked `stream.start()` swallowed the exception `convertWasmFrame` throws in its own try/catch instead of letting it propagate to `startStream()`'s. `WasmBackend.disconnect()` didn't reset `wasmAvailable` back to `false` (unlike `WasmParser.dispose()`'s equivalent teardown), so `isWasmAvailable()` stayed `true` after a real disconnect — fixed to match. One test remains `test.skip`ped with a `TODO(follow-up)`: `WasmParser.stream()`'s `onComplete` callback calls `stream.free()` synchronously from within the callback while `stream.start()`'s own call is still active on the WASM call stack; `resolve()` (called immediately after, in the same callback) is never reached, permanently hanging the returned promise — diagnosis narrowed this to a likely wasm-bindgen reentrant-borrow guard silently aborting the callback, not a Jest/jsdom issue (reproduces identically under both `node` and `jsdom` test environments). Also: `loadWasmModule()`'s Node path now calls `init()` with the object form (`{ module_or_path: buffer }`) instead of the deprecated positional `default(buffer)`, which printed a deprecation warning on every real Node load (invisible in tests since `tests/setup.ts` stubs `console.warn`) and would have silently reintroduced this same bug class once wasm-bindgen drops positional-argument support; the `new Function`-evaluated glue body is now `'use strict'`, matching the real ES module's always-strict semantics; the module-load cache (`cachedNodeModulePromise`) now caches the in-flight promise instead of the resolved value, so concurrent `loadWasmModule()` callers share one wasm instance instead of each independently creating and initializing their own; and both integration test files' `pjs-wasm/pkg`-missing guard now fails loudly instead of silently `describe.skip`ping when `process.env.CI` is set — a silent skip in CI is exactly the failure mode that let this bug survive two earlier fix attempts uncaught (#383)
- `JsonPath::parent()` incorrectly returned the root path (`$`) for any path ending in an array index following a key — e.g. `$.users[0].parent()` returned `$` instead of `$.users`, silently discarding the `users` segment, because the previous string-based implementation searched for the last `.` separator and never accounted for a trailing `[N]`. The new segmented representation's `parent()` is O(1) and structurally cannot make this mistake — it simply drops the last segment (#379)
- `RateLimitMiddleware`'s HTTP rate limiter never pruned its per-IP client map (`WebSocketRateLimiter`'s internal `DashMap`), unlike the WebSocket path (`SecureWebSocketHandler`) which already called `cleanup_expired()` from a background task. Every distinct client IP accumulated a permanent entry for the process lifetime. `WebSocketRateLimiter::spawn_cleanup_task` (new, public) spawns a periodic background task (5-minute default interval) that prunes expired entries; `RateLimitMiddleware::new`, `RateLimitMiddleware::from_limiter`, and `SecureWebSocketHandler::new` (migrated from its own inline, unconditional, strong-`Arc` spawn to this shared mechanism) all call it, and the spawn is idempotent per limiter — several call sites sharing one `Arc<WebSocketRateLimiter>` spawn only one task, guarded by an `AtomicBool` claimed only once a spawn actually succeeds (not by `std::sync::Once`, which would have consumed its one call on a failed attempt and permanently blocked every later, in-runtime retry). The task holds only a `Weak` reference and exits on its own once the limiter is dropped, so it never leaks or keeps the limiter alive past its last owner, for any of the three call sites. Time-based pruning alone still leaves the map unbounded *within* a single sweep window under a burst of distinct IPs, so `WebSocketRateLimiter` also gained a hard `MAX_TRACKED_CLIENTS` (100,000) cap: once reached, requests from not-yet-tracked IPs are rejected with the new `RateLimitError::CapacityExceeded`, mapped by `RateLimitMiddleware` to `503 Service Unavailable` (not `429`, which would mislead the caller into thinking backing off its own rate would help) with a `Retry-After` tied to the cleanup sweep interval; this is a deliberate reject-new-clients (not evict-to-admit) policy — see `MAX_TRACKED_CLIENTS`'s doc comment for why evicting established clients to admit new ones would itself be an exploitable bypass. Also fixed latent underflow panics — both `cleanup_expired()`'s `Instant::now() - window_duration * 2` and `check_request()`'s `Instant::now() - window_duration` could underflow and panic on a host whose uptime is shorter than the configured window (observed to matter on Windows' QPC-backed `Instant`, which is in the CI matrix); both now use `checked_sub`. `cleanup_expired()` skips that pass on underflow (a safe no-op for a background maintenance sweep). `check_request()` fails *closed* on underflow by skipping the window-trim step rather than either panicking or discarding the client's tracked history: discarding history would let a client already at or over its limit bypass it entirely just by making one request during this narrow condition (a freshly booted host, or a deliberately crashed-and-restarted service), while denying every request outright — an earlier iteration of this fix — would instead deny even clients with no prior history at all, amounting to a hard outage for up to `window_duration` after every process start on an affected host. Keeping the untrimmed history closes the former without causing the latter: a client already over its limit stays blocked (a superset of the correctly windowed history is at least as likely to already be at/over the limit), while a client with no prior history is unaffected either way; this is a deliberate choice, documented at the `checked_sub` call site (#346)
- `InMemoryDictionaryStore` accumulated per-session training state (`DashMap<SessionId, Arc<SessionDictState>>`) with no eviction, and pushed frame-payload samples into a session's training corpus with no per-sample size bound. A background task spawned in `InMemoryDictionaryStore::new` now evicts session state idle for more than 30 minutes every 5 minutes (`InMemoryDictionaryStore::cleanup_expired` is also available for manual/test use), refreshing "idle" on writes and on reads that actually serve an already-trained dictionary (`get_dictionary` returning `Some`) — a poll on a still-training session deliberately does *not* refresh it, since doing so would let a client keep an untrained session's budget reservation alive indefinitely just by polling; a session's corpus is emptied immediately once its dictionary finishes training since it is no longer needed; and individual training samples larger than 1 MiB are skipped rather than added to the corpus. Per-sample and per-session bounds alone still allowed unbounded memory growth: a client could send `N_TRAIN - 1` (31) 1 MiB samples per session and never cross the training threshold, pinning ~31 MiB per session for the full TTL with no limit on how many concurrent sessions did this. `InMemoryDictionaryStore` now also tracks a process-wide `TOTAL_CORPUS_BYTE_BUDGET` (128 MiB) across every session's corpus that has not yet *finished* training (accumulating below `N_TRAIN`, or already snapshotted and running inside `spawn_blocking`). The reservation is released via an RAII guard (`CorpusBudgetReservation`) constructed once the corpus is snapshotted and dropped once training completes — critically, `Drop` also runs if the enclosing future is dropped before training finishes (a client disconnecting mid-request, or a timeout/`select!` cancelling the call), which a release placed only after the training `.await` would silently skip; without the guard, a client could repeat that cancellation to drive one session's reservation up toward the entire budget on its own, reopening the same "process-wide training outage" class of issue the read-bump fix above closes via a different path. Once exhausted, new samples are skipped everywhere until a reservation is released by training completion, cancellation, or TTL eviction. `CorpusBudgetReservation` captures the exact reserved amount as a plain `usize` under the same corpus-mutex hold as the training snapshot, rather than holding the session state and re-reading its live `pending_corpus_bytes` counter at drop time: training can run for seconds, during which a concurrent `train_if_ready` call on the *same* session (still possible — `dict` isn't initialized until training finishes) can push fresh samples into the now-empty corpus and grow that counter again; a drop-time re-read would sweep up and release those newer, unrelated reservations too, while the samples backing them are never consumed or bounded, letting up to ~31 MiB of budget-invisible corpus accumulate per session regardless of the 128 MiB global cap. The eviction sweep also resynchronizes the budget counter to the live session map's actual total each pass (self-healing a narrow TOCTOU window where a session's map entry could be evicted concurrently with a caller reserving bytes against it), and both release sites use a saturating compare-and-swap instead of a bare `fetch_sub`, since an undercounted global counter racing that resync could otherwise underflow to near `usize::MAX` and make every subsequent admission check see the budget as permanently exhausted (#329)
- Both `WebSocketRateLimiter::spawn_cleanup_task` and `InMemoryDictionaryStore::new` require a Tokio runtime to actually spawn their periodic cleanup task; outside one, they log a warning and skip spawning rather than panicking, so bare construction remains usable from synchronous/non-async call sites — and, for `WebSocketRateLimiter`, a later call from within a runtime can still succeed (#346, #329)
- `test_wire_stalled_write_times_out_and_closes_connection` (added alongside #364's write-timeout fix) asserted only that a stalled connection closes within its deadline, with no negative control distinguishing that closure from an unrelated closure path (e.g. rejecting the large inbound frame). Added `test_wire_stalled_write_stays_open_before_write_timeout`, a sibling test reusing the same stall setup but with a `write_timeout` far longer than its observation window. It asserts both halves of the causal claim: the connection stays open through the window, and once it does close, the elapsed time is `>= write_timeout` — proving the stalled-write phase was actually reached and that closure is genuinely gated on the timeout, not just a closure-within-window race (#357)
- `AdaptiveStreamController::start_streaming` discarded the `JoinHandle` of the per-session frame-streaming task, so a panic in that task (e.g. on a malformed frame) was silently swallowed by the runtime and the session simply stopped streaming with no diagnostic signal. The handle's `AbortHandle` is now stored on `WebSocketStreamSession` and aborted on session teardown (`remove_session`, `cleanup_expired_sessions`, and a repeated `start_streaming` call replacing an in-flight task), and a supervisor task awaits the `JoinHandle` and logs via `tracing::error!` when it resolves to a panic. `AxumWebSocketTransport::handle_socket` now tracks which streaming sessions each connection created and calls `remove_session` for all of them on connection teardown, so the abort actually happens on client disconnect in production, not just in tests; a new background sweep (`with_rate_limit_config`, weakly holding the controller so it can't keep it alive past the transport's own lifetime) also periodically removes sessions that outlive any connection (#315)
- `crates/pjs-demo` was unbuildable by any documented method (`-p pjs-demo`, `--manifest-path`, or `cd`-ing into the directory): it was excluded from the Cargo workspace while its manifest used full `{ workspace = true }` field inheritance, which Cargo cannot resolve for a crate outside the workspace — a regression of #112. Chose option (a) from #112: restore `crates/pjs-demo` to `[workspace] members`, mirroring how `crates/pjs-bench` was restored in PR #190, while keeping it out of `default-members` so a bare `cargo build`/`cargo test` still only builds `pjs-core`, `pjs-domain`, and `pjs-wasm`. Fixed one additional compile error and one clippy lint this surfaced: three `crates/pjs-demo/src/data/*.rs` files imported `rand::Rng` instead of `rand::RngExt` (`random_range`/`random_bool` moved trait in `rand` 0.10), and a clippy `unnecessary_sort_by` lint in `websocket_streaming.rs`. Documented commands (`README.md`, `.claude/rules/continuous-improvement.md`) now pass pjson-rs feature flags with the required `pjson-rs/` prefix, since `pjs-demo` declares no `[features]` of its own. CI's `rust` paths-filter now includes `crates/pjs-demo/**` so PRs touching only the demo crate trigger quality/build/test/doctest/docs checks; the coverage job excludes `pjs-demo` (untested demo binaries would dilute the reported percentage) (#318)
- `InMemoryEventPublisher.event_log` evicted at capacity by removing an arbitrary `DashMap`-iteration-order slice, not the oldest entries, and `recent_events()` compounded this by reversing that same arbitrary order and presenting it as "most recent." `StoredEvent` now carries a `sequence: u64` stamped by a monotonic counter at store time (`EventId` is a random UUIDv4 and cannot serve as an ordering key); eviction removes the lowest-`sequence` entries down to 9,000 in a single pass (correctly bounding even oversized `publish_batch` calls, not just 1,000 per call), and `recent_events()` sorts by `sequence` to reliably return the newest entries first. `publish_batch` reserves its sequence block with one `fetch_add` before parallelizing, since stamping per-event inside `rayon`'s `into_par_iter` would assign sequences in thread-scheduling order rather than batch order (#350)
- Removed stale TODO comments in `axum_adapter.rs` describing authentication and HTTP rate limiting as unimplemented; both already exist (`ApiKeyAuthLayer`/`JwtAuthLayer` in `infrastructure::http::auth`, `RateLimitMiddleware` in `infrastructure::http::middleware`) and are now documented and cross-referenced in place (#319)
- `DomainEvent::event_id()` derived a content-hash-based UUID, so two structurally-identical events (same variant, session/stream ID, and timestamp) produced the same `EventId`. `InMemoryEventPublisher.event_log` keys solely on `EventId`, so colliding events silently overwrote each other with no error. `InMemoryEventPublisher` and `HttpEventPublisher` now mint a fresh `EventId::new()` per stored/published event at publish time, guaranteeing distinct identity even for structurally-identical events. `EventPublisherGat`'s doc now states this as the identity contract implementors must follow (#328)
- Replace unbounded `mpsc` channels in the WebSocket server/client outgoing-message paths and `InMemoryEventPublisher::with_channel` with bounded channels (capacity 1000; a conservative default bounding message count, not queued bytes — see follow-up for a byte-size-aware bound), so a slow consumer can no longer cause unbounded memory growth. Call sites with a dedicated consumer task and no risk of stalling unrelated work (`PjsWebSocketClient::request_stream`) apply backpressure via `.send().await`; best-effort control-path sends that must not stall their own read loop (WebSocket server `send_frame`, client frame-ack/pong replies, event-publisher streaming channel) use `try_send` and drop-and-log on a full channel instead (#314)
- `pjs-js-client`: `WasmParser.stream()` threw immediately with `TypeError: Cannot destructure property 'PriorityStream' of 'this.wasmModule' as it is undefined` because `initialize()` never assigned the imported `pjs-wasm` module to the instance (#338)
- `pjs-js-client`: `WasmBackend.connect()` always failed in Node.js with a misleading `Failed to initialize WASM backend` error, because the generated `pjs-wasm` init function defaults to `fetch()`-ing the `.wasm` binary, and Node's `fetch` cannot load `file:` URLs. The WASM loader now detects Node.js and reads the `.wasm` binary from disk instead (#317)
- `SchemaAnalyzer::determine_strategy()` compared accumulated compression scores against fixed absolute thresholds that were never calibrated against real payload sizes, so `CompressionStrategy::Dictionary`/`Hybrid` were effectively unreachable for realistic JSON payloads even with obvious repeated-string structure. Dictionary selection is now gated on a modelled net wire-byte saving (see `min_net_savings` below) instead (#333)
- `delta_threshold` and `run_length_threshold` comparisons changed from `>` to `>=` so a score landing exactly on the threshold now selects the corresponding strategy (#333)
- `CompressedData::compressed_size` (and everything derived from it — `compression_ratio`, `compression_savings`, `CompressionStats::{bytes_saved,percentage_saved}`) previously measured only the serialized data, silently ignoring `compression_metadata`; a strategy could report a savings that its actual wire transmission did not deliver once metadata overhead was included. `compressed_size` is now always `data` serialized plus `compression_metadata` serialized (when non-empty), at every strategy's compression call site (#333)
- The dictionary index assigned during `build_dictionary` was an unbounded `u16` counter: a payload with more than `u16::MAX` distinct, individually-qualifying repeated strings panicked on the index increment in debug builds and silently wrapped to duplicate indices in release builds, collapsing distinct dictionary entries into the same `"dict"` array slot and corrupting decode with no error raised. Index assignment now stops once the `u16` index space is exhausted; remaining candidates are simply not dictionaried rather than overflowing (#333)
- A corrupted or truncated dictionary marker could resolve against a *previous*, larger frame's dictionary via `StreamingDecompressor`'s cross-frame accumulated state, instead of erroring — weakening the out-of-range marker check. Dictionary decode is now bounded to each frame's own `dictionary_map`, since every frame already carries its complete dictionary (#333)
- `RateLimitConfig::low_resource()` left `write_timeout` at the general-purpose 10s default (inherited via `..Default::default()`) instead of tightening it alongside the preset's other limits, so a resource-constrained deployment using this preset waited as long as the default to free a wedged connection task. `low_resource()` now sets `write_timeout` to 3s (30%, within the 16.7-40% range of reductions already applied to the preset's other fields). Note this doesn't shrink what an outbound write must flush in time — `max_frame_size` bounds inbound frames only — so a slow-but-honest client now needs roughly 3.3x the downlink bandwidth it needed under the 10s default to avoid being disconnected as "stalled"; see the field's doc comment for the full tradeoff (#358)
- `SearchSessionsQuery`'s `has_more` computation (`offset + sessions.len() < total_count`) was correct but its `true` branch was never exercised by any test — the 5 pre-existing `test_client_info_filter_*` tests all use datasets too small to reach it. Added `test_search_sessions_with_pagination_has_more_true`, strengthened `test_search_sessions_last_page_has_more_false` to a full (not just partial) last page so it also kills a `has_more = page.len() == limit` mutant, and added `test_search_sessions_filter_and_pagination_combined` asserting `total_count`/`has_more` reflect the post-filter count when a non-default filter and pagination are both set (closes #331)

### Documentation

- `byte_bounded_channel` can panic via `Semaphore::new`'s internal assertion if `max_queued_bytes` exceeds `Semaphore::MAX_PERMITS`; documented this with a `# Panics` section. `permits_for`'s `u32::MAX` clamp silently under-charges the byte budget for payloads above 4 GiB; documented this tradeoff on the function. `bounded_channel`'s module-level doc claimed a queued item's permit is always released "as soon as the item is received," which stopped being true once `Envelope::split` let a caller defer that release past receipt — updated to match `Envelope`'s own doc. `infrastructure::websocket::WRITE_TIMEOUT` and `RateLimitConfig::write_timeout` are two independent 10s-default constants gated behind different features with no way to share a single definition; each now carries a doc comment cross-referencing the other, so a future intentional divergence between them is distinguishable from an accidental one (#359)

### Removed

- **BREAKING** `Priority::unwrap_or` (public on `pjson-rs`/`pjson-rs-domain`). It silently discarded its `default` argument and always returned the wrapped value, making it dead weight that misled callers into believing a fallback was applied; `Priority::value()` already returns the same result unconditionally. Pre-1.0 breaking change; no deprecation cycle (#332)
- **BREAKING** `FlowControlCredits` (public on `pjson-rs`/`pjson-rs-domain`, `value_objects` module). Nothing in the workspace constructed or called it outside its own unit tests, and its `consume()` method returned a stringly-typed `Result<(), String>` error rather than a proper domain error type. Removed entirely, including its `Default` impl and dedicated unit tests; `BackpressureSignal` in the same file is unaffected. Flow control is provided by `infrastructure::bounded_channel::byte_bounded_channel`, not by a credit-tracking primitive. Pre-1.0 breaking change; no deprecation cycle (#335)
- **BREAKING** `DomainEvent::event_id()` (public on `pjson-rs`/`pjson-rs-domain`). It derived identity from a content hash, which is exactly the bug behind #328's silent event-log collisions; there is no drop-in replacement because identity is no longer a property of `DomainEvent` itself — see the `Fixed` entry above for where identity is now assigned. Pre-1.0 breaking change; no deprecation cycle (#328)

### Changed

- **BREAKING** `InMemoryEventPublisher::with_channel` now returns `mpsc::Receiver<Envelope<StoredEvent>>` instead of `mpsc::Receiver<StoredEvent>`, as part of the #349 byte-bounded-channel fix above. `Envelope<T>` transparently `Deref`s to `T`, so field access and reads through the received value are source-compatible; call `Envelope::into_inner()` to obtain an owned `StoredEvent`. Pre-1.0 breaking change; no deprecation cycle (#349)
- **BREAKING** `RateLimitConfig` (public on `pjson-rs`, `security::rate_limit`) gained a new `pub write_timeout: Duration` field as part of the #325 write-timeout fix above. `RateLimitConfig` is not `#[non_exhaustive]`, so this breaks external code constructing it via struct literal without `..Default::default()`; it also means a serialized `RateLimitConfig` missing `write_timeout` now fails to deserialize (no `#[serde(default)]`, consistent with this struct's other fields). Pre-1.0 breaking change; no deprecation cycle (#325)
- **BREAKING** `StoredEvent` (public on `pjson-rs`, re-exported from `infrastructure::adapters`) gained a new `pub sequence: u64` field as part of the #350 eviction-order fix above. `StoredEvent` is not `#[non_exhaustive]`, so this breaks any external code constructing it via struct literal; reading fields or matching on existing ones is unaffected. Pre-1.0 breaking change; no deprecation cycle
- `pjs-js-client`: `WasmBackend`'s synthetic `Complete` frame now reports `priority: Priority.Background` (10) instead of the previously hardcoded `1`, which was not a valid `Priority` enum member. This is a disclosed value change on that frame's `priority` field for consumers reading it directly; nothing in `pjs-js-client` itself gates behavior on it
- `pjs-js-client`: `StreamStats.priorityDistribution` is now typed `Partial<Record<Priority, number>>` instead of `Record<Priority, number>` — the previous type was unsound (only priorities actually seen are ever populated) and could not be satisfied by an empty initial value. TypeScript consumers indexing this field now get `number | undefined`
- **Breaking**: dictionary substitution no longer replaces a string with a bare `JsonValue::Number` index tracked by a separate field-path list (`dict_paths`). It now encodes a self-describing sentinel-escaped string marker (`"\u{7F}<index>"`, with sentinel-doubling to escape any payload string that legitimately starts with `\u{7F}`) directly in place of the string. This closes two classes of data corruption by construction rather than by narrowing: a substituted value is structurally never a number (closing the number/index collision) and decoding needs no positional metadata at all (closing the path-string collision, e.g. a payload containing both `{"a":{"b":0}}` and a sibling `"a.b"` key). `DecompressionMetadata::dictionary_paths` and the corresponding `dict_paths` compression-metadata entry are removed (#333)
- **Breaking**: dictionary metadata is now transmitted as a single `"dict"` key holding an index-ordered JSON array of strings, replacing the previous scattered `"dict_0"`, `"dict_1"`, ... keys
- **Breaking**: `CompressionConfig::string_dict_threshold` and `CompressionConfig::min_absolute_savings` are removed, replaced by `CompressionConfig::min_net_savings: usize` (default `10`; `PjsConfig::mobile()` preset `4`). A candidate dictionary entry is kept only when its modelled `gain - cost` (in wire bytes, using the same accounting `compressed_size` reports) is positive, and `CompressionStrategy::Dictionary`/the dictionary half of `Hybrid` is selected only when the summed net saving across all kept entries clears `min_net_savings`
- `SchemaAnalyzer::total_string_bytes` (an internal ratio-normalization field, now unused) is removed
- Dictionary index assignment is now deterministic (candidates sorted descending by `count * length`, lexicographic tie-break) instead of iterating a `HashMap`, so compressed output bytes are reproducible across runs on identical input
- Added `CompressionConfig::validate()`, called from `PjsConfig::validate()`, validating the potential/ratio fields (`uuid_compression_potential`, `min_delta_potential`, `min_compression_potential`) are in `0.0..=1.0` and the threshold fields (`delta_threshold`, `run_length_threshold`) are finite and non-negative
- Sorted `[workspace.dependencies]` in the root `Cargo.toml` alphabetically; no version, feature, or attribute changes (closes #380)

### CI

- Exclude `pjs-wasm` from the `--workspace` build/doctest steps on `build` and `doctest` jobs. Its transitive `web-sys` dependency declares ~1800 features; Cargo's auto-generated `--check-cfg` argument for it exceeds Windows' command-line length limit and crashed `sccache` on `windows-latest` (`os error 206`), intermittently blocking merges. `pjs-wasm`'s tests are all gated to `target_arch = "wasm32"` and its rustdoc examples are JS-only, so nothing was actually being exercised natively (#344)
- Run `npm run typecheck` in the `js-client-test` job before `npm test`, so `pjs-js-client` type errors fail CI instead of going unnoticed (#339)
- Add a `wasm-types-drift` CI job that type-checks `pjs-js-client` against the real, built `pjs-wasm` bindings (excluding the fallback ambient `pjs-wasm` type shim `js-client-test` relies on when the package isn't built), so a breaking rename or signature change in `crates/pjs-wasm`'s public API is caught instead of silently passing behind the shim
- Add a `wasm-native-tests` job running `cargo nextest run -p pjs-wasm --tests`. `crates/pjs-wasm/tests/priority_parity.rs` (a regression guard for #242's WASM/HTTP priority-computation parity) and `streaming_comprehensive.rs` use plain `#[test]` by design, so neither the `--lib --bins`-scoped nextest archive nor `wasm-pack test` (which only discovers `#[wasm_bindgen_test]`) ever executed them; they previously ran only at release-tag time via `release.yml`, after a regression had already merged (#327)

## [0.6.2] - 2026-07-27

### Security

- Upgrade `crossbeam-epoch` to 0.9.20 (resolves RUSTSEC-2026-0204, which flagged 0.9.18 and was blocking the OSV Security Scan gate in CI)

### Changed

- Dependency updates: `bitflags`, `bytes`, `clap`, `hyper`, `memchr`, `regex`, `serde`, `serde_json`, `socket2`, `thiserror`, `tokio`, and `uuid` bumped to latest compatible versions (minor-and-patch group)

### CI

- Bump `actions/cache` from v5 to v6 (#299)
- Bump `lewagon/wait-on-check-action` from 1.8.0 to 1.8.1 (#301), then to 1.9.0 (#308)
- Bump `actions/setup-node` from v6 to v7 (#303)
- Bump `actions/labeler` from v6 to v7 (#307)

## [0.6.1] - 2026-06-29

### Security

- Upgrade `anyhow` to 1.0.103 (resolves RUSTSEC-2026-0190)
- Upgrade `quinn-proto` to 0.11.15 via `Cargo.lock` (resolves RUSTSEC-2026-0185, High — transitive via `reqwest`)
- Upgrade `js-yaml` to ^4.2.0 via npm override in `pjs-js-client` (GHSA-h67p-54hq-rp68)
- Upgrade `shell-quote` to 1.8.4 via npm override in `pjs-js-client` (GHSA — newline escape bypass, Dependabot alert #42)
- Upgrade `ws` to 8.21.0, `markdown-it` to 14.2.0, `@babel/core` to 7.29.7 in `pjs-js-client` lockfile
- Suppress pyo3 advisories (RUSTSEC-2026-0176/0177, GHSA-36hh-v3qg-5jq4/chgr-c6px-7xpp) in `osv-scanner.toml`; the `python` feature of `jiter` is never activated in this project so the vulnerable code is never compiled

### Changed

- Dependency updates: `uuid`, `bytes`, and 13 other crates in the minor-and-patch group bumped to latest compatible versions

### CI

- Bump `actions/checkout` from v6 to v7 (#297)
- Bump `lewagon/wait-on-check-action` from 1.7.0 to 1.8.0 (#295)
- Bump `codecov/codecov-action` from v6 to v7 (#292)

## [0.6.0] - 2026-05-02

### Fixed

- `GET /pjs/sessions/{session_id}/streams/{stream_id}/frames` is now reachable end-to-end. The handler previously validated session/stream existence and then unconditionally returned `{"frames": [], "total_count": 0}` regardless of whether `GenerateFramesCommand` / `BatchGenerateFramesCommand` had produced any frames — a documented gap with the comment "until a `FrameStore` exists". Same shape as the resolved #224 chain (HTTP route 200 OK with no real data). Introduced a new domain port `FrameStoreGat` (with companion `FrameStorePage`) and an in-memory implementation `pjson_rs::infrastructure::adapters::InMemoryFrameStore`, bounded per stream by `pjson_rs::domain::config::DEFAULT_FRAME_HISTORY_PER_STREAM` (10 000 frames, oldest-evicted FIFO). `SessionCommandHandler` now persists frames produced by `GenerateFramesCommand` and `BatchGenerateFramesCommand` into the store; `StreamQueryHandler` reads from it and applies the `since_sequence`, `priority` (minimum threshold), and `limit` filters. Wire-level regression test `test_get_stream_frames_returns_persisted_frames` drives `create-session → create-stream → start-stream → generate-frames` over the real Axum router and asserts the GET endpoint reports the freshly produced frames (closes #269).

### Changed

- **BREAKING** `pjson_rs::infrastructure::http::HttpServerConfig` is now `#[non_exhaustive]` and the public field-init pattern (`HttpServerConfig { allowed_origins: ... }`) no longer compiles outside the defining crate. Use the new `HttpServerConfig::new(allowed_origins)` constructor or `HttpServerConfig::default()` followed by mutation of the public `allowed_origins` field. Resolves the long-standing inline `TODO(critic)` at `axum_adapter.rs` and prevents future additive fields (`allow_credentials`, `max_age`, …) from breaking downstream callers. Eleven public domain enums in `pjson_rs_domain` — `BackpressureSignal`, `PathSegment`, `JsonData`, `Schema`, `SchemaValidationError`, `SchemaType`, `SessionState`, `DomainEvent`, `FrameType`, `PatchOperation`, `StreamState` — are also now `#[non_exhaustive]`, so downstream `match` arms must include a wildcard branch. The two serde-tagged enums (`DomainEvent`, `FrameType`) carry an additional doc note that adding a variant is a wire-format breaking change at the deserialization boundary, independent of the `#[non_exhaustive]` marker (closes #274).
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

- `pjs-core` now builds without warnings when the `compression` feature is disabled. The `std::sync::Arc` and `pjson_rs_domain::value_objects::SessionId` imports in `domain/ports/dictionary_store.rs` are gated on `#[cfg(all(feature = "compression", not(target_arch = "wasm32")))]` to match the trait methods that consume them, and the `dictionary_store` field on `SessionCommandHandler` carries a matching `#[cfg_attr(..., allow(dead_code))]` because it is only read by the feature-gated `train_from_frames` path. Eliminates 3 warnings under `--no-default-features` and any feature subset that excludes `compression` (e.g. `--features mimalloc`, `--features 'simd-auto schema-validation http-server'`); CI's `--all-features` gate previously masked them (closes #271).
- `pjson-rs` and `pjs-wasm` now declare `#![warn(missing_docs)]`, and every `pub` type, trait, function, method, struct field, enum variant, and associated type/function in both crates carries a doc comment. `RUSTDOCFLAGS="--warn missing_docs" cargo doc --no-deps --all-features -p pjson-rs -p pjs-wasm` now emits 0 warnings (down from 535 in `pjson-rs` and 8 in `pjs-wasm`). Brings the two crates in line with the project rule "every `pub` type, trait, function, and method must have a doc comment" that was previously enforced only in `pjson-rs-domain` (closes #270).
- `pjson_rs::compression::SchemaCompressor` no longer panics on non-finite (`NaN`, `+Infinity`, `-Infinity`) base values when `CompressionStrategy::Delta` or `CompressionStrategy::Hybrid` is selected. The two `serde_json::Number::from_f64(*base).unwrap()` sites in `compress_with_delta` and `compress_hybrid` are replaced with explicit `ok_or_else(...)` checks that return `DomainError::CompressionError` carrying the offending field path. Library-API consumers passing user-derived `f64` values directly into `CompressionStrategy::Delta { base_values }` / `CompressionStrategy::Hybrid { numeric_deltas }` now get a typed error instead of a process panic; the `JsonData::float` boundary (hardened in PR #211, closing #176) is no longer the only line of defence. Three regression tests cover NaN base, +Infinity base, and the hybrid path with -Infinity (closes #267).
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
- `GetActiveSessionsQuery` tests now assert `has_more` correctness and the 100-item page cap (closes #208). The `SearchSessionsQuery` half of this claim was inaccurate — no test exercised its `has_more == true` branch until #331 fixed it; `SearchSessionsQuery`'s own 100-item page cap (`query_handlers.rs`'s `MAX_PAGE_SIZE`, applied on the search path) still has no dedicated test and remains a follow-up
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

[Unreleased]: https://github.com/bug-ops/pjs/compare/v0.6.3...HEAD
[0.6.3]: https://github.com/bug-ops/pjs/compare/v0.6.2...v0.6.3
[0.6.2]: https://github.com/bug-ops/pjs/compare/v0.6.1...v0.6.2
[0.6.1]: https://github.com/bug-ops/pjs/compare/v0.6.0...v0.6.1
[0.6.0]: https://github.com/bug-ops/pjs/compare/v0.5.2...v0.6.0
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
