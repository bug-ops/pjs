# PJS - Priority JSON Streaming Protocol

High-performance Rust library for priority-based JSON streaming with SIMD acceleration (sonic-rs backend, runtime CPU dispatch).

## Nightly Rust Requirement

`pjson-rs` requires nightly Rust (`impl_trait_in_assoc_type`; with the `metrics` feature also `once_cell_try`) and has no MSRV. `rust-toolchain.toml` pins a dated nightly; rustup selects it automatically. `pjson-rs-domain` and `pjs-wasm` declare MSRV 1.89 and are checked on it in CI.

```bash
rustc --version  # shows the pinned nightly
```

## Build Commands

```bash
cargo build                                    # Debug build
cargo build --release                          # Release build with LTO
cargo nextest run --workspace --all-features   # Run tests
cargo clippy --workspace --all-targets --all-features -- -D warnings  # Lint (zero warnings)
cargo +nightly fmt --all --check               # Format check
cargo test --workspace --doc --all-features    # Doctests
cargo bench -p pjs-bench                       # Benchmarks
RUSTFLAGS="-C target-cpu=native --cfg pjs_bench" cargo bench -p pjs-bench  # Full suite incl. client_apply_patches
```

## Clean Architecture

Three strict layers with unidirectional dependencies:

1. **Domain** (`crates/pjs-domain/` plus `crates/pjs-core/src/domain/`) - Pure business logic; `pjs-domain` holds entities and value objects and stays WASM-compatible, `pjs-core/src/domain/` re-exports it and adds aggregates, GAT ports and services
2. **Application** (`crates/pjs-core/src/application/`) - CQRS orchestration, no business logic
3. **Infrastructure** (`crates/pjs-core/src/infrastructure/`) - Port implementations (HTTP, WebSocket, in-memory adapters)

NEVER import from a higher layer (domain cannot import application/infrastructure).

## Module Structure

```
crates/
├── pjs-core/       # Core library (published as pjson-rs)
├── pjs-domain/     # Domain types and value objects
├── pjs-bench/      # Criterion benchmarks
├── pjs-demo/       # Interactive demo servers
├── pjs-wasm/       # WebAssembly bindings
└── pjs-js-client/  # JavaScript client
```

## Code Quality Standards

- Zero clippy warnings required
- All documentation in English
- Comments only for complex logic blocks
- No emoji in commits or documentation

## Coverage Requirements

Targets; CI reports coverage to Codecov as informational, so reviewers enforce them.

| Layer | Minimum |
|-------|---------|
| Security | 100% |
| Domain | 80% |
| Application | 70% |
| Infrastructure | 60% |

## Lints

CI builds with `RUSTFLAGS="-D warnings"` and clippy `-D warnings`; the workspace sets `unsafe_op_in_unsafe_fn = "deny"`. Do not add blanket `allow` attributes; a targeted `allow` needs a one-line reason.

## Commit Format

Use Conventional Commits: `feat`, `fix`, `perf`, `refactor`, `test`, `docs`, `style`, `build`, `ci`, `chore`, `release`. Scopes are crate or module names (`pjson-rs`, `pjson-rs-domain`, `wasm`, `parser`, `stream`, `http`, `security`, `bench`, `ci`). Mark breaking changes with `!` and a `BREAKING CHANGE:` footer.

NEVER mention co-authorship or AI generation.

## Allowed Licenses

The allow-list in `deny.toml`: MIT, Apache-2.0, Apache-2.0 WITH LLVM-exception, BSD-2-Clause, BSD-3-Clause, ISC, Unicode-3.0, 0BSD, Zlib.
