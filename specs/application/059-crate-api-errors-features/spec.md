---
aliases:
  - Crate API surface, error type and feature matrix
  - pjson-rs public API
tags:
  - sdd
  - spec
  - api
  - errors
  - features
  - cargo
created: 2026-11-15
status: partial
related:
  - "[[constitution]]"
  - "[[MOC-specs]]"
  - "[[052-domain-gat-ports/spec|Domain GAT ports]]"
  - "[[055-application-commands/spec|Application commands]]"
  - "[[018-config-validation-startup/spec|Config validation (018)]]"
  - "[[017-axum-error-http-status-mapping/spec|Error to HTTP mapping (017)]]"
  - "[[006-sonic-rs-primary-parser-path/spec|sonic-rs parser path (006)]]"
  - "[[003-sonic-rs-avx512-feature/spec|AVX-512 feature (003)]]"
code:
  - crates/pjs-core/src/lib.rs
  - crates/pjs-core/src/error.rs
  - crates/pjs-core/src/global_alloc.rs
  - crates/pjs-core/Cargo.toml
  - crates/pjs-core/build.rs
  - rust-toolchain.toml
  - .github/workflows/ci.yml
verified: 2026-11-18
---

# Spec 059: Crate Public API Surface, `pjson_rs::Error` and Feature-Flag Matrix

> [!info] Verification (2026-11-15)
> **State: partial.** The API surface and feature set exist and build with default and `--all-features`. Known defects: no per-feature CI matrix (#615), `rust-version` 1.89 is not enforceable on a nightly-only crate (#608). `CLAUDE.md` still lists a removed `jemalloc` feature.

## 1. Overview

### Problem
Consumers need a stable, documented entry point (`pjson_rs`, `pjson_rs::prelude`), a single crate-level error, and a predictable set of Cargo features.

### Goal
Record what `lib.rs` exports, what `Error` provides, and what each feature enables, with known defects.

### Out of scope
- Per-subsystem behaviour (parser, compression, security, HTTP, WebSocket): see their specs.
- WASM crate API (`pjs-wasm`), `docs/architecture/WASM_ROADMAP.md`.
- Wire protocol: `docs/architecture/SPECIFICATION.md`.

## 2. User Stories
- As a library user, I `use pjson_rs::prelude::*` and get the common domain, error and stream types.
- As an integrator, I choose features to avoid compiling axum or compression when unneeded.
- As a maintainer, I know which feature combinations are supported.

## 3. Functional Requirements

### FR-001 Crate-level attributes
The crate shall enable `#![feature(impl_trait_in_assoc_type)]` (nightly, required by GAT ports), `#![cfg_attr(feature = "metrics", feature(once_cell_try))]`, `#![warn(rust_2018_idioms)]`, `#![warn(missing_docs)]`, `#![deny(unsafe_op_in_unsafe_fn)]`. The workspace lint config does not allow `clippy::manual_div_ceil` or `clippy::only_used_in_recursion`. `rust-toolchain.toml` pins a dated nightly channel (see 078 FR-012). Upstream stabilisation is tracked in issue #580.

### FR-002 Public modules
`lib.rs` shall declare `pub mod` for `application`, `compression`, `config`, `domain`, `error`, `global_alloc`, `infrastructure`, `parser`, `security`, `stream`; `test_support` is `#[cfg(test)]` only.

### FR-003 Root re-exports
The crate root shall re-export: domain types (`DomainError`, `DomainEvent`, `DomainResult`, `Frame as DomainFrame`, `JsonPath`, `PathSegment`, `Priority`, `SessionId`, `Stream`, `StreamId`, `StreamSession`, `JsonData`, `Schema`, `SchemaId`, `SchemaType`, `SchemaValidationError`, `SchemaValidationResult`); ports (`EventPublisherGat`, `FrameSinkGat`, `FrameSinkGatExt`, `FrameSourceGat`, `StreamRepositoryGat`, `StreamStoreGat`); services (`GatOrchestratorFactory`, `GatStreamingOrchestrator`, `HealthStatus`, `OrchestratorConfig`, `ValidationService`); `PriorityDistribution`, `PriorityPercentages`; application (`ApplicationError`, `ApplicationResult`, modules `commands` and `queries`, schema DTOs); config (`ConfigError`, `ParserConfig`, `PjsConfig`, `SecurityConfig`, `SimdConfig`, `StreamingConfig`, `Validate`, `Validated`, `ValidatedCompressionConfig`, `ValidatedSecurityConfig`, limit structs); compression, parser (`LazyParser`, `ZeroCopyParser`), security and stream types; `Error`, `Result`; `SchemaRepository`; `global_allocator_name`; hidden `__pastey`.
`SecureWebSocketHandler` is re-exported when `websocket-server` is enabled. `JsonPath` and `PathSegment` come from `pjson-rs-domain` with the infallible, any-key contract of [[042-domain-crate-contract-and-value-objects/spec|spec 042]]. The websocket module re-exports `MaxFrameSize`, `SendOutcome` and `DropReason`, and `StreamOptions::patch()` exists (#700, #706).
Code: `lib.rs`.

### FR-004 Prelude
`pjson_rs::prelude` shall re-export: `ApplicationError`, `ApplicationResult`, `DomainError`, `DomainEvent`, `DomainFrame`, `DomainResult`, `Error`, `JsonData`, `JsonPath`, `JsonReconstructor`, `PathSegment`, `Priority`, `PriorityDistribution`, `PriorityPercentages`, `ProcessResult`, `Result`, `Schema`, `SchemaId`, `SchemaRepository`, `SchemaType`, `SchemaValidationError`, `SessionId`, `Stream`, `StreamId`, `StreamProcessor`, `StreamSession`, `ValidationService`.

### FR-005 Error type
`pjson_rs::Error` shall be a `#[non_exhaustive]` `#[derive(Debug, thiserror::Error)]` typed taxonomy (not `Clone`; #599) with 23 variants: `InvalidJson { position, source }`, `LimitExceeded { kind: LimitKind, actual: u64, max: u64 }`, `InvalidSessionId(SessionIdViolation)`, `CompressionBomb(CompressionBombError)`, `Codec { codec: CodecKind, operation: CodecOperation, source: io::Error }`, `CodecUnavailable { codec }`, `InvalidDictionary(DictionaryError)`, `Patch(PatchError)` (`PatchError::UnsupportedSegment` was removed in #699), `Serialization(serde_json::Error)`, `ConnectionFailed { url, source }`, `Transport`, `WriteTimeout { timeout }`, `InvalidUrl(url::ParseError)`, `AlreadyConnected`, `ChannelClosed`, `SessionLimitReached { limit: NonZeroUsize }`, `UnknownSession(SessionId)`, `SessionAlreadyExists(SessionId)`, `TooManySessions { limit }`, `RateLimited(RateLimitError)`, `FrameSequence(FrameSequenceError)`, `PlanRejected`, `UnsupportedStreamOptions`. Wrapped dependency errors are reachable through `source()` and downcastable; `Display` never repeats the source text. `Result<T> = std::result::Result<T, Error>`.
Code: `error.rs`.

### FR-006 Error helpers and conversions
`Error` shall provide predicates (`is_json_error`, `is_network_error`, `is_validation_error`, `is_security_error`) and `category() -> &'static str` returning one of `json`, `validation`, `security`, `network`, `compression`, `client`. `is_security_error` is `LimitExceeded | CompressionBomb | RateLimited`; `is_network_error` is `ConnectionFailed | Transport | WriteTimeout`; `is_validation_error` is `InvalidSessionId | Patch | FrameSequence` (and `category()` is `validation` for `FrameSequence`). `FrameSequenceError` is `#[non_exhaustive]` (`UnexpectedFrameId { expected, received }`, `FramingMismatch { expected, received }`; `FrameKind` mirrors `websocket::Framing` so the error module does not depend on features). `InvalidStreamOptions` was removed: stream options are validated at decode time (#700). `From<serde_json::Error>` keeps the source; `From<io::Error>` and `From<Utf8Error>` do not exist. Log sites print a crate `Error` through a crate-private `ErrorChain` that joins the source chain with `: `.

### FR-007 Error layering
Three error types coexist: `DomainError` (`pjs-domain`, `#[non_exhaustive]`), `ApplicationError` (wraps `DomainError`, see [[055-application-commands/spec|055]]), and `pjson_rs::Error` (parser, stream, compression, security, WebSocket). `PjsError` in `infrastructure::http` maps `ApplicationError` to HTTP ([[017-axum-error-http-status-mapping/spec|017]]). There is no `From` between `Error` and `ApplicationError`.

### FR-008 Feature matrix
`crates/pjs-core/Cargo.toml` shall define:

| Feature | Enables | Notes |
|---|---|---|
| `default` | `simd-auto`, `schema-validation`, `compression`, `http-server`, `websocket-server`, `websocket-client` | |
| `simd-auto` | none (marker) | `build.rs` emits `cfg(pjs_simd)`; no code reads `pjs_simd` since #486/#488; sonic-rs dispatches at runtime |
| `simd-avx512` | `sonic-rs/avx512` | needs `-C target-cpu=native` or `+avx512f`, `build.rs` warns otherwise ([[003-sonic-rs-avx512-feature/spec|003]]) |
| `mimalloc` | `dep:mimalloc` | registers `#[global_allocator]` on non-wasm; `global_allocator_name()` returns `"mimalloc"` or `"system"` |
| `compression` | `brotli`, `flate2`, `zstd` | gates `ZstdDictionary` and `InMemoryDictionaryStore`; the `DictionaryStore` port is not gated (#605) |
| `partial-parse` | `jiter` | gates `JiterPartialParser` and a `SecurityConfig` field |
| `schema-validation` | `regex` | enables `pattern` in `ValidationService` |
| `http-server` | `axum`, `tower`, `tower-http`, `subtle`, `hmac`, `sha2`, `getrandom`, `headers-accept`, `mediatype`, `hyper`, `hyper-util`, `async-stream`, `tokio-util` | gates `infrastructure::http` (including `ShutdownSignal`) |
| `http-auth-jwt` | `http-server`, `jsonwebtoken` | `JwtAuthLayer` |
| `metrics` | `http-server`, `metrics`, `metrics-exporter-prometheus` | also enables `once_cell_try` feature gate; `pjs_frames_total` counter ([[009-realtime-metrics-observability/spec|009]]) |
| `websocket-server` | `http-server` | gates the server module and `SecureWebSocketHandler` (#598) |
| `websocket-client` | `tokio-tungstenite`, `url` | builds on its own (#598) |
| `float-roundtrip` (off) | `serde_json/float_roundtrip` | exact f64 round-trip for serde_json parsing (~2x float-token cost, unifies to every serde_json in the build); off = best-effort, some floats differ by 1 ULP (#575) |

The `jemalloc` feature was removed (CHANGELOG, #160; README says v0.6.0); `CLAUDE.md` still lists it.

Always-on dependencies include `jiff` (`std`, `serde`; replaced `chrono`, #692) and `tokio` with `rt-multi-thread`, `sync`, `time`, `macros`, `net` (the `parking_lot` feature was removed in #709, so `cargo tree -p pjson-rs -e normal --duplicates --all-features` no longer lists `parking_lot` 0.12; `lock_api`/`parking_lot_core` duplicates remain through `dashmap`; the direct `parking_lot` dependency is unchanged). Minute-valued durations in non-test code use `Duration::from_mins` (#710).

### FR-009 Build script
`build.rs` shall declare `cfg(pjs_simd)` for `--check-cfg`, rerun on `CARGO_CFG_TARGET_FEATURE`/`CARGO_CFG_TARGET_ARCH` changes, warn when `simd-avx512` is enabled without `avx512f` or `simd-auto` without `avx2`/`sse4.2` on x86_64, and set `cfg(pjs_simd)` when either feature is enabled.

### FR-010 Package metadata
The package shall be published (`publish = true`) with `exclude = ["/target", "/benches", "/tests", "examples/*"]`, `build = "build.rs"`, trusted publishing via GitHub OIDC (`release.yml`), four `[[example]]` entries gated by features and two `[[test]]` entries with `required-features`.

## 4. Non-Functional Requirements
- NFR-BUILD-001: `cargo clippy --workspace --all-targets --all-features -- -D warnings` passes in CI (`ci.yml`).
- NFR-BUILD-002: the CI `build` job covers `ubuntu`, `macos`, `windows` x allocator `system|mimalloc` with `--features "default[,mimalloc]"` (nextest archive, `pjs-wasm` excluded); the `jemalloc` allocator was removed in #160.
- NFR-DOC-001: `#![warn(missing_docs)]`; `cargo doc --no-deps --all-features -p pjson-rs -p pjson-rs-domain` runs in CI.

## 5. Data Model
`Error` (FR-005); `prelude` (FR-004); feature table (FR-008).

## 6. Edge Cases and Errors

| Case | Result |
|---|---|
| `--no-default-features --features websocket-client` | builds (#598); the CI feature matrix checks each feature (#615) |
| `mimalloc` enabled in a binary with another `#[global_allocator]` | link-time conflict (documented in `global_alloc.rs`) |
| `simd-avx512` without AVX-512 target features | build warning, scalar fallback |
| Wrapped dependency errors (`InvalidJson`, `Codec`, `Serialization`, `ConnectionFailed`, `Transport`) | reachable via `source()`, downcastable |

## 7. Success Criteria
- `cargo build -p pjson-rs` and `cargo build -p pjson-rs --all-features` succeed on nightly.
- `Error::category()` unit tests (`error.rs`) pass for every variant.
- A per-feature build matrix exists in CI [open: #615].

## 8. Agent Boundaries
- Always: document new `pub` items; add new features to the table in FR-008; keep default features minimal for wasm consumers (`pjs-wasm` depends on `pjson-rs-domain`, not `pjson-rs`).
- Ask first: adding a root re-export or prelude item (semver surface); adding a default feature.
- Never: add a feature that is only additive when used with another undeclared feature; add a `#[global_allocator]` outside `global_alloc.rs`.

## 9. Open Questions
- Resolved by #599: `Error` is a typed, `#[non_exhaustive]` taxonomy with `source()` chains.
- `simd-auto` is effectively a no-op marker (`pjs_simd` has no readers). [NEEDS CLARIFICATION: keep as compatibility flag or remove.]
- `rust-version = "1.89"` is unenforceable with nightly-only features (#608).
- Unused dependencies `bitflags`, `maplit` (#607).
- `PjsConfig` is exported at the root while `Validate` coverage is incomplete (see [[018-config-validation-startup/spec|018]], issue #614).
- Root re-exports mix layers (domain, application, infrastructure `SchemaRepository`, stream, security); no facade grouping.
- `CLAUDE.md` mentions `jemalloc` and "Build with specific allocator" using a removed feature.

## 10. See Also
[[059-crate-api-errors-features/plan|plan]], `README.md` (feature table), `docs/architecture/SPECIFICATION.md`, [[052-domain-gat-ports/spec|052]].
