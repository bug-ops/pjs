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

PJS requires **nightly Rust 1.89+** for Generic Associated Types (GAT) features:

```bash
# Install nightly Rust
rustup install nightly

# Set nightly as override for this project
cd pjs
rustup override set nightly

# Verify nightly is active
rustc --version  # Should show "nightly"
```

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

- **Domain Layer (`pjson-rs-domain`)**: Minimum 80% coverage (enforced in CI)
- **Core Library (`pjson-rs`)**: Minimum 70% coverage (recommended)
- **Infrastructure Layer**: Minimum 60% coverage (recommended)
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
#[tokio::test]
async fn test_session_lifecycle() {
    let service = SessionService::new(/* deps */);
    let session_id = service.create_session().await.unwrap();
    assert!(service.get_session(&session_id).await.is_ok());
}
```

**Property-Based Tests:**
```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn priority_values_are_valid(p in 0u8..=100) {
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
cargo test --workspace --doc

# 5. Check coverage (optional but recommended)
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
   cargo clippy --workspace --all-features -- -D warnings
   cargo nextest run --workspace --all-features
   cargo test --workspace --doc
   ```

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
   - Tests (all platforms: Linux, macOS, Windows)
   - Coverage thresholds
   - Security scanning (OSV)
   - WASM builds (if applicable)

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
│   ├── value_objects/   # Priority, SessionId, Schema, JsonData
│   ├── entities/        # StreamSession, Stream
│   ├── events/          # Domain events
│   └── ports/           # GAT-based traits
├── pjs-core/            # Rust implementation
│   ├── application/     # CQRS handlers, use cases
│   ├── infrastructure/  # HTTP, WebSocket, repositories
│   ├── parser/          # SIMD JSON parsing
│   └── stream/          # Priority streaming engine
├── pjs-wasm/            # WebAssembly bindings
├── pjs-bench/           # Performance benchmarks
└── pjs-demo/            # Demo applications
```

### Architectural Rules (Enforced)

1. **Domain Layer** must not depend on infrastructure or application layers
2. **Domain Layer** must not import `serde_json::Value` (use `JsonData` instead)
3. **Application Layer** orchestrates but contains no business logic
4. **Infrastructure Layer** implements domain ports (traits)
5. All async abstractions use **GATs** (Generic Associated Types), not `async_trait`

### Adding New Features

**Example: Adding a new validation rule**

1. Update domain value object: `domain/value_objects/schema.rs`
2. Implement validation logic: `domain/services/validation_service.rs`
3. Add DTOs: `application/dto/schema_dto.rs`
4. Add integration tests: `crates/pjs-core/tests/`
5. Add benchmarks: `crates/pjs-bench/benches/`

## Feature Flags

PJS uses feature flags to minimize compile times and binary size. Be mindful when adding dependencies:

### Core Features (Default)
- `simd-auto` - Auto-detect SIMD support
- `schema-validation` - Schema validation engine

### Optional Features
- `compression` - Schema-based compression
- `http-server` - Axum HTTP server
- `websocket-client` - WebSocket client
- `websocket-server` - WebSocket server
- `http-client` - HTTP event publishing
- `prometheus-metrics` - Prometheus integration

### Memory Allocators (Mutually Exclusive)
- `jemalloc` - Use jemalloc allocator
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

4. Document in README.md feature table

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
- jemalloc works best here

### Testing Locally

If you can't test on all platforms, CI will catch issues, but try to consider:

```bash
# Run platform-specific tests
cargo nextest run --workspace --all-features

# Check for platform-specific warnings
cargo clippy --target x86_64-pc-windows-msvc
cargo clippy --target x86_64-apple-darwin
cargo clippy --target x86_64-unknown-linux-gnu
```

## Performance Benchmarks

When making performance-related changes, always benchmark:

```bash
# Save baseline before changes
cargo bench -p pjs-bench -- --save-baseline before

# Make your changes

# Compare against baseline
cargo bench -p pjs-bench -- --baseline before

# View results
open target/criterion/report/index.html
```

**Benchmark Suites:**
- `simple_throughput.rs` - Parser performance
- `memory_benchmarks.rs` - Arena allocation efficiency
- `streaming_benchmarks.rs` - Progressive loading speed

## Documentation

### Code Documentation

Use clear, concise doc comments:

```rust
/// Parses JSON with priority-based frame generation.
///
/// # Arguments
///
/// * `json` - Input JSON string
/// * `min_priority` - Minimum priority threshold (0-100)
///
/// # Returns
///
/// Vector of frames ordered by priority
///
/// # Example
///
/// ```
/// use pjson_rs::parser::parse;
/// let frames = parse(r#"{"id": 123}"#, 50);
/// ```
///
/// # Errors
///
/// Returns `ParseError` if JSON is invalid
pub fn parse(json: &str, min_priority: u8) -> Result<Vec<Frame>, ParseError> {
    // ...
}
```

### Architecture Documentation

Update relevant docs when changing architecture:

- `docs/architecture/SPECIFICATION.md` - Protocol specification
- `CLAUDE.md` - Project instructions for AI assistants
- `.local/` - Internal project documentation

## Getting Help

- **GitHub Discussions**: https://github.com/bug-ops/pjs/discussions
- **Issues**: https://github.com/bug-ops/pjs/issues
- **Documentation**: https://docs.rs/pjson-rs

## License

By contributing to PJS, you agree that your contributions will be licensed under both the MIT License and Apache License 2.0, at the user's option.
