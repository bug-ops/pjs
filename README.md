# PJS - Priority JSON Streaming Protocol

[![Crates.io](https://img.shields.io/crates/v/pjson-rs.svg)](https://crates.io/crates/pjson-rs)
[![Documentation](https://docs.rs/pjson-rs/badge.svg)](https://docs.rs/pjson-rs)
[![Rust Build](https://github.com/bug-ops/pjs/actions/workflows/rust.yml/badge.svg)](https://github.com/bug-ops/pjs/actions/workflows/rust.yml)
[![codecov](https://codecov.io/gh/bug-ops/pjs/branch/main/graph/badge.svg)](https://codecov.io/gh/bug-ops/pjs)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE)

**Priority-based streaming | Progressive loading | Zero-copy operations | WebAssembly ready**

High-performance Rust library for priority-based JSON streaming with SIMD acceleration. Stream large JSON responses progressively, delivering critical data first while background data loads asynchronously.

> [!IMPORTANT]
> **v0.4.7**: GAT migration (1.82x faster), HTTP adapter with CQRS, decompression algorithms with security hardening. 2,158 tests passing. Requires **nightly Rust** for zero-cost async abstractions.

## Features

- **Blazing Fast** - SIMD-accelerated parsing, GAT-based zero-cost abstractions (1.82x faster than async_trait)
- **Smart Streaming** - Priority-based delivery sends critical data first, skeleton-first rendering
- **Memory Efficient** - Optimized progressive loading, bounded memory usage, zero-copy operations
- **WebAssembly** - Browser and Node.js support with compact bundle (~70KB gzipped)
- **Secure** - Defense-in-depth decompression protection, DoS prevention, input validation
- **Schema Aware** - Automatic compression and semantic analysis
- **Production Ready** - Clean Architecture, 87.35% test coverage, Prometheus metrics

## Performance

| Benchmark | Performance Gain | Notes |
|-----------|-----------------|-------|
| **GAT Async** | **1.82x faster** | Static dispatch eliminates virtual calls |
| **Small JSON** | Competitive | Comparable to industry standards |
| **Medium JSON** | **~3x faster** | vs traditional parsers |
| **Large JSON** | **~6x faster** | vs traditional parsers |
| **Progressive Loading** | **~5x faster** | vs batch processing |

## Installation

```bash
cargo add pjson-rs
```

Or add to `Cargo.toml`:

```toml
[dependencies]
pjson-rs = "0.4"
```

> [!NOTE]
> Requires Rust 1.89+ (nightly). See [MSRV policy](#building) for details.

## Quick Start

### Rust HTTP Server

```rust
use pjson_rs::infrastructure::http::axum_adapter::create_pjs_router;

#[tokio::main]
async fn main() {
    let app = create_pjs_router().with_state(app_state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
    axum::serve(listener, app).await?;
}
```

Start server and test:

```bash
# Run example server
cargo run --example axum_server

# Create session
curl -X POST http://localhost:3000/pjs/sessions \
  -H "Content-Type: application/json" \
  -d '{"max_concurrent_streams": 5}'

# Stream data with priority
curl http://localhost:3000/pjs/stream/SESSION_ID/sse
```

### WebAssembly (Browser)

```bash
npm install pjs-wasm
```

#### PriorityStream API (Recommended)

```html
<script type="module">
import init, { PriorityStream, PriorityConstants } from './pkg/pjs_wasm.js';

async function main() {
    await init();

    const stream = new PriorityStream();
    stream.setMinPriority(PriorityConstants.MEDIUM());

    // Register callbacks
    stream.onFrame((frame) => {
        console.log(`${frame.type} [${frame.priority}]: ${frame.payload}`);
        if (frame.priority >= 80) {
            updateUI(JSON.parse(frame.payload)); // High priority first
        }
    });

    stream.onComplete((stats) => {
        console.log(`Completed: ${stats.totalFrames} frames in ${stats.durationMs}ms`);
    });

    // Start streaming
    stream.start(JSON.stringify({ id: 123, name: "Alice", bio: "..." }));
}

main();
</script>
```

> [!TIP]
> Use the PriorityStream API for automatic frame handling and built-in security limits. Ideal for real-time dashboards and progressive loading.

#### Simple Parser API

```html
<script type="module">
import init, { PjsParser, PriorityConstants } from './pkg/pjs_wasm.js';

async function main() {
    await init();
    const parser = new PjsParser();

    const frames = parser.generateFrames(
        JSON.stringify({ user_id: 123, name: "Alice" }),
        PriorityConstants.MEDIUM()
    );

    frames.forEach(frame => {
        if (frame.priority >= 90) {
            updateUI(frame.data); // Critical data first
        }
    });
}

main();
</script>
```

#### Interactive Demo

Try the [Browser Demo](crates/pjs-wasm/demo/) with transport switching, performance benchmarks, and real-time metrics.

### WebAssembly (Node.js)

```javascript
import init, { PjsParser } from 'pjs-wasm';
import { readFile } from 'fs/promises';

const wasmBuffer = await readFile('./node_modules/pjs-wasm/pkg/pjs_wasm_bg.wasm');
await init(wasmBuffer);

const parser = new PjsParser();
const frames = parser.generateFrames(JSON.stringify(data), 50);

frames.forEach(frame => {
    console.log(`Priority ${frame.priority}: ${frame.frame_type}`);
});
```

### Build WASM from Source

```bash
# Install wasm-pack
curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh

# Build for different targets
wasm-pack build crates/pjs-wasm --target web --release      # Browsers
wasm-pack build crates/pjs-wasm --target nodejs --release   # Node.js
wasm-pack build crates/pjs-wasm --target bundler --release  # Webpack/Rollup
```

## Use Cases

- **Real-time Dashboards** - Show key metrics instantly, load details progressively
- **Mobile Apps** - Optimize for slow networks, critical data first
- **E-commerce** - Product essentials load immediately, reviews/images follow
- **Financial Platforms** - Trading data prioritized over historical charts
- **Gaming Leaderboards** - Player rank appears instantly, full list streams in

## Building

### Prerequisites

> [!WARNING]
> This project requires **nightly Rust** for Generic Associated Types (GAT) features. Stable Rust is not supported.

```bash
rustup install nightly
rustup override set nightly
```

### Build Commands

```bash
# Standard build
cargo build --release

# Run tests with nextest
cargo nextest run --workspace

# Run benchmarks
cargo bench -p pjs-bench

# Run demo servers
cargo run --bin websocket_streaming --manifest-path crates/pjs-demo/Cargo.toml
cargo run --bin interactive_demo --manifest-path crates/pjs-demo/Cargo.toml
```

### Feature Flags

> [!TIP]
> Start with default features. Add extras only when needed to keep compile times fast.

| Feature | Description | Default |
|---------|-------------|---------|
| `simd-auto` | Auto-detect SIMD support | ✅ Yes |
| `simd-avx2` | Force AVX2 SIMD | No |
| `simd-neon` | Force ARM NEON | No |
| `schema-validation` | Schema validation engine | ✅ Yes |
| `compression` | Schema-based compression | No |
| `http-server` | Axum HTTP server | No |
| `websocket-server` | WebSocket streaming | No |
| `prometheus-metrics` | Prometheus integration | No |
| `jemalloc` | Use jemalloc allocator | No |
| `mimalloc` | Use mimalloc allocator | No |

## Security

PJS includes built-in security features to prevent DoS attacks:

```javascript
import { PriorityStream, SecurityConfig } from 'pjs-wasm';

const security = new SecurityConfig()
    .setMaxJsonSize(5 * 1024 * 1024)  // 5 MB limit
    .setMaxDepth(32);                  // 32 levels max

const stream = PriorityStream.withSecurityConfig(security);
```

**Default Limits:**

- Max JSON size: 10 MB
- Max nesting depth: 64 levels
- Max array elements: 10,000
- Max object keys: 10,000

> [!IMPORTANT]
> **v0.4.7 Security**: Delta and RLE decompression now include 4-layer defense-in-depth protection against decompression bombs (CVSS 7.5 vulnerabilities fixed).

**Decompression Security:**

- **MAX_RLE_COUNT**: 100,000 items per run
- **MAX_DELTA_ARRAY_SIZE**: 1,000,000 elements
- **MAX_DECOMPRESSED_SIZE**: 10 MB total
- **Integer overflow protection**: Checked arithmetic throughout

## Architecture

PJS follows Clean Architecture with Domain-Driven Design:

- **pjs-domain** - Pure business logic, WASM-compatible
- **pjs-wasm** - WebAssembly bindings with PriorityStream API, security limits (44 tests)
- **pjs-core** - Rust implementation with HTTP/WebSocket integration (2,100+ tests)
- **pjs-demo** - Interactive demo servers with real-time streaming
- **pjs-js-client** - TypeScript/JavaScript client with WasmBackend transport
- **pjs-bench** - Comprehensive performance benchmarks

**v0.4.7 Improvements:**

- **GAT Migration**: Zero-cost async abstractions (1.82x faster)
- **HTTP Adapter**: 8 REST endpoints with CQRS pattern
- **Security Hardening**: Decompression bomb protection, input validation
- **Test Coverage**: 87.35% (2,158 tests passing)
- **Platform Support**: Windows, Linux, macOS validated

## Contributing

Contributions welcome! Please ensure:

```bash
rustup override set nightly
cargo clippy --workspace -- -D warnings
cargo nextest run --workspace --all-features
cargo +nightly fmt --check
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.

## Resources

- [Protocol Specification](docs/architecture/SPECIFICATION.md)
- [Changelog](CHANGELOG.md)
- [Benchmarks](crates/pjs-bench/README.md)
- [Discussions](https://github.com/bug-ops/pjs/discussions)

---

*PJS: Priority-based JSON streaming for instant user experiences.*
