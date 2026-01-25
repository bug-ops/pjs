# PJS - Priority JSON Streaming Protocol

High-performance Rust library for priority-based JSON streaming with SIMD acceleration achieving 6.3x faster performance than serde_json.

## Nightly Rust Requirement

This project requires nightly Rust for `impl_trait_in_assoc_type` feature.

```bash
rustup override set nightly
```

## Build Commands

```bash
cargo build                                    # Debug build
cargo build --release                          # Release build with LTO
cargo nextest run --workspace --all-features   # Run tests
cargo clippy --workspace -- -D warnings        # Lint (zero warnings)
cargo +nightly fmt --check                     # Format check
cargo bench -p pjs-bench                       # Benchmarks
```

## Clean Architecture

Three strict layers with unidirectional dependencies:

1. **Domain** (`src/domain/`) - Pure business logic, zero external dependencies
2. **Application** (`src/application/`) - CQRS orchestration, no business logic
3. **Infrastructure** (`src/infrastructure/`) - Port implementations

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

| Layer | Minimum |
|-------|---------|
| Security | 100% |
| Domain | 80% |
| Application | 70% |
| Infrastructure | 60% |

## Allowed Clippy Warnings

- `manual_div_ceil` - Performance optimization
- `only_used_in_recursion` - Recursive algorithms
- `dead_code` - Future features

## Commit Format

Use conventional commits: `feat:`, `fix:`, `perf:`, `refactor:`, `test:`, `docs:`, `ci:`

NEVER mention co-authorship or AI generation.

## Allowed Licenses

MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Unicode-3.0, 0BSD, Zlib, Unicode-DFS-2016, MPL-2.0
