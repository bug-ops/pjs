# Contributing to PJS

Thank you for your interest in contributing to PJS (Priority JSON Streaming Protocol). This document provides guidelines and instructions for contributing to the project.

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [Getting Started](#getting-started)
- [Development Workflow](#development-workflow)
- [Testing Requirements](#testing-requirements)
- [Code Quality Standards](#code-quality-standards)
- [Pull Request Process](#pull-request-process)
- [Project Architecture](#project-architecture)
- [Feature Flags](#feature-flags)
- [Cross-Platform Considerations](#cross-platform-considerations)

## Code of Conduct

This project adheres to the Rust Project Code of Conduct. By participating, you are expected to uphold this code. Please report unacceptable behavior to the project maintainers. See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) for details.

## Getting Started

### Prerequisites

`pjson-rs` requires **nightly Rust** and has no MSRV. The nightly gates are `impl_trait_in_assoc_type` (GAT-based ports) and, with the `metrics` feature, `once_cell_try`; GATs themselves are stable. `rust-toolchain.toml` pins a dated nightly, so rustup installs and selects it automatically. `pjson-rs-domain` and `pjs-wasm` declare MSRV 1.89, checked in CI.

```bash
# Verify the pinned nightly is active
rustc --version  # Should show "nightly"
```

#### Bumping the nightly pin

`rust-toolchain.toml` is the single source of truth for the dated nightly. CI and release jobs install it through the local composite action `.github/actions/setup-rust`, which reads the channel from that file (the `msrv` job is the only exception and pins `1.89`). To bump:

1. Change `channel` in `rust-toolchain.toml` to the new `nightly-YYYY-MM-DD`.
2. Run the full check suite locally on the new nightly (fmt, clippy, nextest, doc tests, doc gate).
3. Open one PR with that change; CI must pass on it. No other file needs editing.

### Clone and Build

```bash
# Clone the repository
git clone https://github.com/bug-ops/pjs.git
cd pjs

# Build the project
cargo build

# Run tests to verify setup
cargo nextest run --workspace
```

### Install Development Tools

```bash
# cargo-nextest for faster test execution
cargo install cargo-nextest

# cargo-llvm-cov for code coverage
cargo install cargo-llvm-cov

# wasm-pack for WebAssembly builds (optional)
curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh
```

## Development Workflow

### Branch Naming Convention

Use descriptive branch names following this pattern:

- `feat/description` - New features
- `fix/description` - Bug fixes
- `refactor/description` - Code refactoring
- `docs/description` - Documentation changes
- `chore/description` - Maintenance tasks
- `test/description` - Test improvements
- `perf/description` - Performance improvements

Examples:
```bash
git checkout -b feat/add-custom-priority-strategy
git checkout -b fix/windows-instant-overflow
git checkout -b refactor/gat-migration
```

### Commit Message Convention

Write clear, descriptive commit messages:

**Format:**
```
<type>(<scope>): <subject>

<body>

<footer>
```

**Types:**
- `feat` - New feature
- `fix` - Bug fix
- `refactor` - Code refactoring
- `docs` - Documentation changes
- `test` - Test improvements
- `perf` - Performance improvements
- `chore` - Maintenance tasks
- `ci` - CI/CD changes
- `style`, `build`, `release` - Formatting, build system and release preparation

Follow [Conventional Commits 1.0.0](https://www.conventionalcommits.org/en/v1.0.0/). Mark breaking changes with `!` after the type or scope (`feat(http)!: ...`) and a `BREAKING CHANGE:` footer. Scopes are crate or module names: `pjson-rs`, `pjson-rs-domain`, `wasm`, `parser`, `stream`, `http`, `security`, `bench`, `ci`.

**Examples:**
```
feat(parser): add SIMD-accelerated JSON parsing

Implement AVX2-optimized parsing for 6x performance gain on large payloads.
Uses sonic-rs backend with zero-copy operations.

Closes #123
```

```
fix(metrics): prevent Windows Instant overflow in time series

Use checked_sub() to handle duration calculation when cutoff exceeds
program uptime. Fixes panic on Windows in metrics collector.

Fixes #456
```

**Important:** Keep commit messages concise and professional. Do not use emojis or informal language.

## Testing Requirements

### Running Tests

PJS uses `cargo-nextest` for test execution:

```bash
# Run all tests
cargo nextest run --workspace

# Run tests with all features
cargo nextest run --workspace --all-features

# Run tests for specific crate
cargo nextest run -p pjson-rs
cargo nextest run -p pjson-rs-domain
cargo nextest run -p pjs-wasm

# Run specific test by name
cargo nextest run test_schema_validation

# Run tests with output visible
cargo nextest run --nocapture

# Run doctests
cargo test --workspace --doc
```

### Fuzzing

Fuzz targets live in the standalone `fuzz/` crate (its own workspace, excluded from the root workspace) and require `cargo-fuzz` and nightly Rust:

```bash
cargo install cargo-fuzz

# Build all targets
cargo fuzz build

# List targets
cargo fuzz list

# Replay the committed seeds deterministically (what CI gates on)
cargo fuzz run ws_message_decode fuzz/seeds/ws_message_decode -- -runs=0

# Explore for 60 seconds into the git-ignored scratch corpus, seeds as extra input
cargo fuzz run ws_message_decode "$PWD/fuzz/corpus/ws_message_decode" "$PWD/fuzz/seeds/ws_message_decode" -- -max_total_time=60
```

libFuzzer writes every new input into the first corpus directory, so never pass `fuzz/seeds/<target>` first (or alone) when exploring: the run would add generated files to the tracked seeds. Explore into `fuzz/corpus/<target>`, minimize with `cargo fuzz cmin <target>`, and copy only selected inputs into `fuzz/seeds/<target>` deliberately.

The `fuzzing` entry points of `pjson-rs` are compiled only under `--cfg fuzzing` (`cargo fuzz` sets it); to build or lint the fuzz crate directly use `RUSTFLAGS="--cfg fuzzing" cargo build --manifest-path fuzz/Cargo.toml`.

The fuzz crate builds without `serde_json/float_roundtrip`, so it runs the production float parsing path; `ws_message_decode` therefore compares round trips exactly against `reparsed` (one serialize/parse cycle, in the `pjs_fuzz` library, `fuzz/src/lib.rs`). The opt-in `float-roundtrip` feature is covered by `crates/pjs-core/tests/float_roundtrip.rs`.

Seeds are tiny hand-written inputs under `fuzz/seeds/<target>/`. Do not commit crash artifacts or generated corpora; file an issue for a crash instead. CI replays seeds on every relevant change (`fuzz` job) and explores each target for 60 seconds on the weekly schedule and on manual dispatch (`fuzz-explore` job, non-gating).

### Code Coverage

All contributions must maintain or improve code coverage:

```bash
# Generate coverage report with HTML output
cargo llvm-cov nextest --workspace --all-features --html
open target/llvm-cov/html/index.html

# Generate coverage for specific crate
cargo llvm-cov nextest -p pjson-rs --html

# Check coverage summary
cargo llvm-cov nextest --workspace --summary-only
```

**Coverage Requirements:**

- **Domain Layer (`pjson-rs-domain`)**: Minimum 80% coverage
- **Core Library (`pjson-rs`)**: Minimum 70% coverage
- **Infrastructure Layer**: Minimum 60% coverage

CI uploads coverage to Codecov (`.github/codecov.yml`); the checks are informational and do not block merging, so reviewers enforce these targets.
- **Security-Critical Code**: 100% coverage required
  - `security/` module (rate limiting, compression bomb detection)
  - `domain/services/validation_service.rs`
  - `parser/` security checks

**Coverage Quality Guidelines:**

- All public APIs must have test coverage
- Error paths must be tested
- Edge cases and boundary conditions required
- Security-critical code requires comprehensive tests
- Zero unsafe code without 100% test coverage

### Writing Tests

**Unit Tests:**
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_ordering() {
        let high = Priority::new(90).unwrap();
        let low = Priority::new(10).unwrap();
        assert!(high > low);
    }
}
```

**Integration Tests:**
```rust
// In crates/pjs-core/tests/
use pjson_rs::{JsonData, StreamSession};

#[test]
fn test_streams_require_an_active_session() {
    let mut session = StreamSession::new(Default::default());
    assert!(session.create_stream(JsonData::Null).is_err());

    session.activate().unwrap();
    assert!(session.create_stream(JsonData::Null).is_ok());
}
```

Tests that need the `websocket-server`, `websocket-client` or `http-server` features declare them through `required-features` in `crates/pjs-core/Cargo.toml`, so run them with `--all-features`.

**Property-Based Tests:**
```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn priority_values_are_valid(p in 1u8..=255) {
        let priority = Priority::new(p);
        assert!(priority.is_ok());
    }
}
```

## Code Quality Standards

All code must pass strict quality checks before merging:

### Formatting

```bash
# Check formatting (required before commit)
cargo +nightly fmt --all --check

# Auto-format code
cargo +nightly fmt --all
```

### Linting

```bash
# Run clippy in strict mode (zero warnings allowed)
cargo clippy --workspace --all-features --all-targets -- -D warnings
```

**Important:** All clippy warnings must be resolved. The CI enforces `-D warnings` (deny warnings).

### Pre-Commit Checklist

Before committing, run:

```bash
# 1. Format code
cargo +nightly fmt --all

# 2. Check for clippy warnings
cargo clippy --workspace --all-features --all-targets -- -D warnings

# 3. Run all tests
cargo nextest run --workspace --all-features

# 4. Verify doctests
cargo test --workspace --doc --all-features

# 5. Verify rustdoc links (same flags as the CI `docs` job)
RUSTDOCFLAGS="--deny rustdoc::broken_intra_doc_links --deny rustdoc::private_intra_doc_links --deny rustdoc::redundant_explicit_links" \
  cargo doc --no-deps --all-features -p pjson-rs -p pjson-rs-domain

# 6. Check coverage (optional but recommended)
cargo llvm-cov nextest --workspace --summary-only
```

## Pull Request Process

### Before Opening a PR

1. Ensure your branch is up to date with `main`:
   ```bash
   git fetch origin
   git rebase origin/main
   ```

2. Run the full CI check locally:
   ```bash
   cargo +nightly fmt --all --check
   cargo clippy --workspace --all-features --all-targets -- -D warnings
   cargo nextest run --workspace --all-features
   cargo test --workspace --doc --all-features
   ```

   CI builds with `RUSTFLAGS="-D warnings"`; export it locally to reproduce warning failures.

3. Update documentation if needed:
   - Update `CHANGELOG.md` with your changes
   - Update README.md if adding new features
   - Add/update code examples
   - Update architecture documentation if changing structure

### PR Title and Description

**Title Format:**
```
<type>(<scope>): <description>
```

**Description Template:**
```markdown
## Summary

Brief description of what this PR does and why.

## Changes

- Change 1
- Change 2
- Change 3

## Testing

Describe how you tested these changes:

- [ ] Unit tests added/updated
- [ ] Integration tests added/updated
- [ ] Manual testing performed
- [ ] Coverage maintained/improved

## Checklist

- [ ] Code follows project style guidelines
- [ ] Tests pass locally
- [ ] Coverage requirements met
- [ ] Documentation updated
- [ ] CHANGELOG.md updated
- [ ] No clippy warnings
- [ ] Formatted with `cargo +nightly fmt`

## Related Issues

Closes #123
```

### Review Process

1. **Automated Checks**: CI must pass before review
   - Code quality (formatting, clippy)
   - Tests (Linux, macOS, Windows; system allocator and `mimalloc`)
   - Feature matrix (`cargo hack --each-feature`)
   - MSRV 1.89 check of `pjson-rs-domain` and `pjs-wasm`
   - Miri on the `unsafe` code in `parser::buffer_pool` and `parser::aligned_alloc`
   - Fuzz seed replay (`fuzz` job)
   - Coverage upload (informational)
   - Security scanning (OSV, `cargo deny`)
   - WASM builds, bundle size and type drift check (if applicable)
   - JS client tests on Node 22 and 24 (if applicable)
   - Instruction-count benchmarks via gungraun (report-only, never gating)

2. **Code Review**: At least one maintainer approval required
   - Architecture compliance
   - Code quality and readability
   - Test coverage
   - Documentation completeness

3. **Merge**: Squash and merge to `main` after approval

## Project Architecture

PJS follows **Clean Architecture** with **Domain-Driven Design**. Understanding this is crucial for contributions:

### Layer Structure

```
crates/
├── pjs-domain/          # Pure business logic (WASM-compatible)
│   ├── value_objects/   # Priority, SessionId, StreamId, Schema, JsonData, JsonPath
│   ├── entities/        # Frame, Stream
│   ├── events/          # Domain events
│   └── services/        # Pure domain services (priority, skeleton)
├── pjs-core/            # Rust implementation
│   ├── domain/          # Re-exports pjs-domain; aggregates (StreamSession),
│   │                    # GAT ports, services, validated config types
│   ├── application/     # CQRS commands, queries, handlers, DTOs
│   ├── infrastructure/  # HTTP (axum), WebSocket, adapters, repositories
│   ├── parser/          # SIMD JSON parsing, buffer pool
│   ├── stream/          # Priority streaming engine
│   ├── compression/     # Compression with bomb protection
│   ├── security/        # Rate limiting, compression bomb detection
│   └── config/          # Runtime configuration
├── pjs-wasm/            # WebAssembly bindings
├── pjs-bench/           # Performance benchmarks
└── pjs-demo/            # Demo applications
```

### Architectural Rules (Enforced)

1. **Domain Layer** must not depend on infrastructure or application layers
2. **Domain Layer** must not import `serde_json::Value` (use `JsonData` instead)
3. **Application Layer** orchestrates but contains no business logic
4. **Infrastructure Layer** implements domain ports (traits)
5. Async ports (`domain/ports/`) use **GATs** (`impl Trait` in associated types, nightly), not `async_trait`

### Adding New Features

**Example: Adding a new validation rule**

1. Update domain value object: `crates/pjs-domain/src/value_objects/schema.rs`
2. Implement validation logic: `crates/pjs-core/src/domain/services/validation_service.rs`
3. Add DTOs: `crates/pjs-core/src/application/dto/schema_dto.rs`
4. Add integration tests: `crates/pjs-core/tests/`
5. Add benchmarks: `crates/pjs-bench/benches/`

## Feature Flags

PJS uses feature flags to minimize compile times and binary size. Be mindful when adding dependencies:

### Default Features
- `simd-auto` - Auto-detect SIMD support at runtime (x86_64 and aarch64)
- `schema-validation` - Schema validation engine
- `compression` - Compression codecs (zlib/gzip, brotli, zstd)
- `http-server` - Axum HTTP server
- `websocket-server` / `websocket-client` - WebSocket transport

### SIMD Features
- `simd-avx512` - Forwards to `sonic-rs/avx512` (x86_64-only; requires `RUSTFLAGS="-C target-cpu=native"` to take effect)

### Optional Features
- `http-auth-jwt` - JWT authentication for the HTTP server (implies `http-server`)
- `metrics` - Prometheus `/metrics` endpoint (implies `http-server`; needs the `once_cell_try` nightly feature)
- `partial-parse` - Partial JSON parsing via `jiter`
- `float-roundtrip` - Exact `f64` round trips in `serde_json` parsing

### Memory Allocators (Optional)
- `mimalloc` - Use mimalloc allocator

### Adding New Features

1. Add feature to `Cargo.toml`:
   ```toml
   [features]
   my-feature = ["dep:some-crate"]
   ```

2. Gate code with feature flag:
   ```rust
   #[cfg(feature = "my-feature")]
   pub mod my_feature {
       // Feature-specific code
   }
   ```

3. Update CI to test with new feature:
   ```yaml
   # .github/workflows/ci.yml
   - run: cargo test --features my-feature
   ```

4. Document in the README feature table
5. Run `cargo hack check -p pjson-rs --each-feature --no-dev-deps` (the CI `feature-matrix` job) to confirm each feature builds on its own

## Cross-Platform Considerations

PJS supports Linux, macOS, and Windows. All contributions must work on all platforms.

### Platform-Specific Code

Use conditional compilation when necessary:

```rust
#[cfg(target_os = "windows")]
fn platform_specific() {
    // Windows implementation
}

#[cfg(not(target_os = "windows"))]
fn platform_specific() {
    // Unix implementation
}
```

### Common Platform Issues

**Windows:**
- Time precision: Use `checked_sub()` for `Instant` calculations
- Path separators: Use `std::path::PathBuf`
- Line endings: Git handles CRLF/LF automatically

**macOS:**
- File system is case-insensitive by default
- Test on both Intel and Apple Silicon if possible

**Linux:**
- Default CI platform

### Testing Locally

If you can't test on all platforms, CI will catch issues, but try to consider:

```bash
# Run platform-specific tests
cargo nextest run --workspace --all-features

# Check for platform-specific warnings
cargo clippy --target x86_64-pc-windows-msvc
cargo clippy --target aarch64-apple-darwin
cargo clippy --target x86_64-unknown-linux-gnu
```

## Performance Benchmarks

When making performance-related changes, always benchmark:

```bash
# Save baseline before changes (Criterion flags need an explicit --bench target)
cargo bench -p pjs-bench --bench simple_throughput -- --save-baseline before

# Make your changes

# Compare against baseline
cargo bench -p pjs-bench --bench simple_throughput -- --baseline before

# Full suite incl. client_apply_patches (needs the pjs_bench cfg)
RUSTFLAGS="-C target-cpu=native --cfg pjs_bench" cargo bench -p pjs-bench

# View results
open target/criterion/report/index.html
```

See `crates/pjs-bench/README.md` (Before/After Comparison) for the full A/B procedure.

**Benchmark Suites:**
- `simple_throughput.rs` - Parser performance
- `buffer_pool.rs` - Aligned buffer pool
- `priority_streamer.rs` - Priority streaming
- `http_streaming.rs`, `http_serve_lifecycle.rs` - HTTP adapter
- `websocket_controller.rs` - WebSocket controller
- `client_apply_patches.rs` - Client patch application (needs `--cfg pjs_bench`)
- `gungraun_parser.rs` - Instruction counts (Linux only; CI report-only job)

## Documentation

### Code Documentation

Use clear, concise doc comments:

```rust
/// Creates a stream in this session.
///
/// # Examples
///
/// ```
/// use pjson_rs::{JsonData, StreamSession};
///
/// let mut session = StreamSession::new(Default::default());
/// session.activate().unwrap();
/// assert!(session.create_stream(JsonData::Null).is_ok());
/// ```
///
/// # Errors
///
/// Returns an error if the session is not active or already holds its
/// maximum number of concurrent streams.
pub fn create_stream(&mut self, source_data: JsonData) -> DomainResult<StreamId> {
    // ...
}
```

### Architecture Documentation

Update relevant docs when changing architecture:

- `docs/architecture/SPECIFICATION.md` - Protocol specification
- `specs/` - Per-capability specifications; a change to an existing capability is a delta against its `spec.md`, and `specs/MOC-specs.md` must stay in sync with the code

## Getting Help

- **GitHub Discussions**: https://github.com/bug-ops/pjs/discussions
- **Issues**: https://github.com/bug-ops/pjs/issues
- **Documentation**: https://docs.rs/pjson-rs

## License

By contributing to PJS, you agree that your contributions will be licensed under both the MIT License and Apache License 2.0, at the user's option.
