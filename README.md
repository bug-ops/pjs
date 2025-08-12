<!-- markdownlint-disable MD024 -->
# PJS - Priority JSON Streaming Protocol

[![Crates.io](https://img.shields.io/crates/v/pjson-rs.svg)](https://crates.io/crates/pjson-rs)
[![Documentation](https://docs.rs/pjson-rs/badge.svg)](https://docs.rs/pjson-rs)
[![Rust Build](https://github.com/bug-ops/pjs/actions/workflows/rust.yml/badge.svg)](https://github.com/bug-ops/pjs/actions/workflows/rust.yml)
[![codecov](https://codecov.io/gh/bug-ops/pjs/branch/main/graph/badge.svg)](https://codecov.io/gh/bug-ops/pjs)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE)
[![Rust Version](https://img.shields.io/badge/rust-1.88%2B-blue.svg)](https://www.rust-lang.org)

**🚀 6.3x faster than serde_json | 🎯 5.3x faster progressive loading | 💾 Bounded memory usage | 🏗️ Production Ready**

> **New in v0.3.0**: Production-ready code quality with zero clippy warnings, Clean Architecture compliance, and comprehensive test coverage (196 tests). Ready for production deployment.

</div>

## 🌟 Key Features

<table>
<tr>
<td>

### ⚡ Blazing Fast

- **6.3x faster** than serde_json
- **1.71 GiB/s** throughput
- SIMD-accelerated parsing

</td>
<td>

### 🎯 Smart Streaming

- Skeleton-first delivery
- Priority-based transmission
- Progressive enhancement

</td>
<td>

### 💾 Memory Efficient

- **5.3x** faster progressive loading
- Bounded memory usage
- Zero-copy operations

</td>
</tr>
<tr>
<td>

### 🔧 Production Ready

- All tests passing
- Clean Architecture

</td>
<td>

### 📊 Schema Aware

- Automatic compression
- Semantic analysis
- Type optimization

</td>
<td>

### 🚀 Developer Friendly

- Simple API
- Drop-in replacement
- Extensive documentation

</td>
</tr>
</table>

## 🎯 The Problem

Modern web applications face a fundamental challenge: **large JSON responses block UI rendering**.

### Current State

- 📊 Analytics dashboard loads 5MB of JSON
- ⏱️ User waits 2-3 seconds seeing nothing
- 😤 User thinks app is broken and refreshes
- 🔄 The cycle repeats

### Why existing solutions fall short

| Solution | Problem |
|----------|---------|
| **Pagination** | Requires multiple round-trips, complex state management |
| **GraphQL** | Still sends complete response, just smaller |
| **JSON streaming** | No semantic understanding, can't prioritize |
| **Compression** | Reduces size but not time-to-first-byte |

## ✨ The Solution: PJS

PJS revolutionizes JSON transmission by **understanding your data semantically** and **prioritizing what matters**.

### Core Innovation: Semantic Prioritization

```rust
#[derive(JsonPriority)]
struct UserDashboard {
    #[priority(critical)]  // Sent in first 10ms
    user_id: u64,
    user_name: String,
    
    #[priority(high)]      // Sent in next 50ms
    recent_activity: Vec<Activity>,
    notifications: Vec<Notification>,
    
    #[priority(low)]       // Sent when available
    detailed_analytics: Analytics,  // 4MB of data
}
```

### Real-World Impact

```plain
Traditional JSON Loading:
[████████████████████] 100% - 2000ms - Full UI renders

PJS Loading:
[██░░░░░░░░░░░░░░░░░░] 10%  - 10ms   - Critical UI visible
[██████░░░░░░░░░░░░░░] 30%  - 50ms   - Interactive UI
[████████████████████] 100% - 2000ms - Full data loaded

User Experience: ⚡ Instant → 😊 Happy
```

## Key Features

### 🚀 Complete HTTP Server Integration

Production-ready Axum integration with full REST API, session management, and real-time streaming.

### 🎯 Advanced Streaming Implementations

- **AdaptiveFrameStream**: Client capability-based optimization
- **BatchFrameStream**: High-throughput batch processing  
- **PriorityFrameStream**: Priority-based frame ordering with buffering

### 🏗️ Domain-Driven Design Architecture

Clean architecture with CQRS pattern, event sourcing, and ports & adapters for maximum testability and maintainability.

### 📊 Production-Ready Infrastructure

- Thread-safe in-memory storage and metrics collection
- Event publishing with subscription support
- Prometheus metrics integration
- Comprehensive middleware stack (CORS, security, compression)

### 🔄 Multiple Response Formats

Automatic format detection supporting JSON, NDJSON, and Server-Sent Events based on client Accept headers.

### ⚡ SIMD-Accelerated Parsing

Powered by `sonic-rs` for blazing fast JSON processing with zero-copy operations.

### 🔄 Real-Time WebSocket Streaming

Complete WebSocket implementation with priority-based frame delivery:

- **Session Management**: Track active WebSocket connections with metrics
- **Priority-Based Delivery**: Critical data sent first with adaptive delays
- **Schema-Based Compression**: Intelligent compression using multiple strategies  
- **Progressive Enhancement**: Skeleton-first streaming with incremental updates
- **Demo Servers**: Interactive demonstrations of real-time streaming capabilities

## 🎉 What's New in v0.3.0

### 🛠️ Production-Ready Code Quality

- **Zero Clippy Warnings**: All 44+ clippy warnings resolved across entire codebase
- **Modern Format Strings**: Updated to `format!("{var}")` syntax throughout
- **Enhanced Error Handling**: Proper Result patterns and async trait compatibility
- **Memory Safety**: Fixed await-holding lock patterns and buffer alignment issues
- **196 Tests Passing**: Complete test suite with all features enabled

### 🏗️ Clean Architecture Enforcement

- **Domain Layer Isolation**: Custom `JsonData` value object replacing `serde_json::Value`
- **Type Safety**: Eliminated all architecture violations in domain layer
- **Seamless Conversion**: `From` trait implementations for `JsonData ↔ serde_json::Value`
- **Proper Boundaries**: Clear separation between domain and infrastructure errors

### 🌐 HTTP/WebSocket Modernization

- **Axum v0.8 Compatibility**: Updated route syntax from `:param` to `{param}` format
- **StreamExt Integration**: Fixed async stream processing with proper trait imports
- **Body Type Updates**: Modern HTTP body handling for latest axum/hyper versions
- **All Tests Passing**: Complete HTTP integration test suite validation

### 🔧 Technical Debt Resolution

- **Architecture Compliance**: Resolved all Clean Architecture violations
- **Lint Standards**: Zero warnings with strict linting enabled (`-D warnings`)
- **Async Patterns**: Fixed await-across-locks and other async safety issues
- **Type System**: Enhanced type safety with better generic bounds and aliases

## Benchmarks

### 🚀 **Actual Performance Results**

| Metric | serde_json | sonic-rs | PJS | PJS Advantage |
|--------|------------|----------|-----|---------------|
| **Small JSON (43B)** | 275ns | 129ns | 312ns | Competitive |
| **Medium JSON (351B)** | 1,662ns | 434ns | 590ns | **2.8x vs serde** |
| **Large JSON (357KB)** | 1,294μs | 216μs | 204μs | **6.3x vs serde, 1.06x vs sonic** |
| **Memory Efficiency** | Baseline | Fast | **5.3x faster** progressive | **Bounded memory** |
| **Progressive Loading** | Batch-only | Batch-only | **37μs** vs 198μs | **5.3x faster** |

### 🎯 **Key Performance Achievements**

- **6.3x faster** than serde_json for large JSON processing
- **1.06x faster** than sonic-rs (SIMD library) on large datasets  
- **5.3x faster** progressive loading vs traditional batch processing
- **1.71 GiB/s** sustained throughput (exceeding sonic-rs 1.61 GiB/s)

## Installation

Add PJS to your `Cargo.toml`:

```toml
[dependencies]
pjson-rs = "0.3.0"

# Optional: for HTTP server integration
axum = "0.8"
tokio = { version = "1", features = ["full"] }
```

Or use cargo:

```bash
cargo add pjson-rs
```

## Quick Start

### HTTP Server with Axum Integration

```rust
use std::sync::Arc;
use pjson_rs::{
    application::{
        handlers::{InMemoryCommandHandler, InMemoryQueryHandler},
        services::{SessionService, StreamingService},
    },
    infrastructure::{
        adapters::{InMemoryStreamRepository, InMemoryEventPublisher, InMemoryMetricsCollector},
        http::axum_adapter::{create_pjs_router, PjsAppState},
    },
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Create infrastructure
    let repository = Arc::new(InMemoryStreamRepository::new());
    let event_publisher = Arc::new(InMemoryEventPublisher::new());
    let metrics_collector = Arc::new(InMemoryMetricsCollector::new());
    
    // Create CQRS handlers
    let command_handler = Arc::new(InMemoryCommandHandler::new(
        repository.clone(), event_publisher, metrics_collector.clone()
    ));
    let query_handler = Arc::new(InMemoryQueryHandler::new(repository, metrics_collector));
    
    // Create services
    let session_service = Arc::new(SessionService::new(command_handler.clone(), query_handler.clone()));
    let streaming_service = Arc::new(StreamingService::new(command_handler));
    
    // Build Axum app
    let app = create_pjs_router()
        .with_state(PjsAppState::new(session_service, streaming_service));
    
    // Start server
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
    println!("🚀 PJS Server running on http://127.0.0.1:3000");
    axum::serve(listener, app).await?;
    
    Ok(())
}
```

### Client Usage (HTTP/SSE)

```javascript
// Create session
const sessionResponse = await fetch('/pjs/sessions', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
        max_concurrent_streams: 5,
        timeout_seconds: 3600
    })
});
const { session_id } = await sessionResponse.json();

// Start streaming
await fetch(`/pjs/stream/${session_id}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
        data: { 
            store: { name: "Demo Store", products: [...] }
        }
    })
});

// Receive real-time updates via Server-Sent Events
const eventSource = new EventSource(`/pjs/stream/${session_id}/sse`);
eventSource.onmessage = (event) => {
    const frame = JSON.parse(event.data);
    if (frame.priority >= 90) {
        renderCriticalData(frame);  // Instant rendering
    } else {
        renderProgressively(frame); // Progressive enhancement
    }
};
```

### WebSocket Streaming

```rust
use pjson_rs::{
    ApplicationResult,
    domain::value_objects::SessionId,
};
use futures::{SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[tokio::main]
async fn main() -> ApplicationResult<()> {
    // Connect to WebSocket streaming server
    let (ws_stream, _) = connect_async("ws://127.0.0.1:3001/ws")
        .await
        .expect("Failed to connect");
    
    let (mut write, mut read) = ws_stream.split();
    
    // Receive prioritized frames
    while let Some(message) = read.next().await {
        match message? {
            Message::Text(text) => {
                let frame: serde_json::Value = serde_json::from_str(&text)?;
                
                match frame["@type"].as_str() {
                    Some("pjs_frame") => {
                        let priority = frame["@priority"].as_u64().unwrap_or(0);
                        
                        if priority >= 200 {
                            println!("🚨 Critical data: {}", frame["data"]);
                        } else if priority >= 100 {
                            println!("📊 High priority: {}", frame["data"]);
                        } else {
                            println!("📝 Background data received");
                        }
                    }
                    Some("stream_complete") => {
                        println!("✅ Stream completed!");
                        break;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    
    Ok(())
}
```

### Demo Servers

Start the interactive demo to see PJS in action:

```bash
# WebSocket streaming server
cargo run --bin websocket-streaming-server

# Interactive demo with HTML interface  
cargo run --bin interactive-demo-server

# Simple demo server
cargo run --bin simple-demo-server
```

Then visit `http://127.0.0.1:3000` to see priority-based streaming in action.

## Use Cases

Perfect for:

- 📊 **Real-time dashboards** - Show key metrics instantly
- 📱 **Mobile apps** - Optimize for slow networks
- 🛍️ **E-commerce** - Load product essentials first
- 📈 **Financial platforms** - Prioritize critical trading data
- 🎮 **Gaming leaderboards** - Show player's rank immediately

## Architecture

PJS implements a clean, layered architecture following Domain-Driven Design principles:

### 1. Domain Layer

Core business logic with value objects (Priority, SessionId, JsonPath) and aggregates (StreamSession) ensuring data consistency.

### 2. Application Layer  

CQRS pattern with separate Command and Query handlers, plus high-level services (SessionService, StreamingService) orchestrating workflows.

### 3. Infrastructure Layer

Adapters implementing domain ports:

- **Storage**: In-memory repositories with thread-safe concurrent access
- **Events**: Publisher/subscriber pattern for domain event distribution  
- **Metrics**: Performance monitoring with Prometheus integration
- **HTTP**: Complete Axum server with middleware stack

### 4. Transport Abstraction

Multi-format streaming support:

- **JSON**: Standard response format
- **NDJSON**: Newline-delimited for efficient processing
- **Server-Sent Events**: Real-time browser compatibility
- Automatic format detection via Accept headers

### 5. Advanced Streaming

Intelligent frame processing:

- **Priority-based delivery**: Critical data first
- **Adaptive buffering**: Dynamic sizing based on client performance
- **Batch processing**: High-throughput chunk aggregation

## Technical Architecture

```plain
pjs/
├── crates/
│   ├── pjs-core/        # Core protocol, domain logic, and HTTP integration
│   ├── pjs-demo/        # Interactive demo servers with WebSocket streaming
│   │   ├── servers/     # Demo server implementations
│   │   ├── clients/     # WebSocket client demos  
│   │   ├── data/        # Sample data generators
│   │   └── static/      # HTML interfaces
│   ├── pjs-client/      # Client implementations (planned)
│   ├── pjs-server/      # Server framework extensions (planned)
│   ├── pjs-transport/   # Advanced transport layers (planned)
│   ├── pjs-gpu/        # GPU acceleration (planned)
│   └── pjs-bench/      # Benchmarking suite (planned)
└── examples/
    └── axum_server.rs  # Complete working HTTP server demo
```

### Current Implementation Status

- **Phase 1**: ✅ Core foundation (100% complete)
- **Phase 2**: ✅ Protocol layer (100% complete)  
- **Phase 3**: ✅ Client/Server framework (100% complete)
- **Phase 4**: ✅ Transport layer (100% complete)
- **Phase 5**: ✅ Production features (100% complete)
- **Phase 6**: ✅ Real-Time Streaming (100% complete)
- **Phase 7**: ✅ Code Quality & Production Readiness (100% complete)
- **Overall**: ~95% of core functionality implemented

## API Examples

### HTTP Endpoints

The server provides a complete REST API:

```bash
# Create a new session
POST /pjs/sessions
Content-Type: application/json
{
  "max_concurrent_streams": 10,
  "timeout_seconds": 3600,
  "client_info": "My App v1.0"
}

# Response: { "session_id": "sess_abc123", "expires_at": "..." }

# Get session info  
GET /pjs/sessions/{session_id}

# Start streaming data
POST /pjs/stream/{session_id}
Content-Type: application/json
{
  "data": { "users": [...], "products": [...] },
  "priority_threshold": 50,
  "max_frames": 100
}

# Stream frames (JSON format)
GET /pjs/stream/{session_id}/frames?format=json&priority=80

# Real-time Server-Sent Events
GET /pjs/stream/{session_id}/sse
Accept: text/event-stream

# System health check
GET /pjs/health
# Response: { "status": "healthy", "version": "0.3.0" }
```

### Working Example

A complete working server is available at `examples/axum_server.rs`. To run it:

```bash
# Start the server
cargo run --example axum_server

# Test endpoints
curl -X POST http://localhost:3000/pjs/sessions \
  -H "Content-Type: application/json" \
  -d '{"max_concurrent_streams": 5}'

# Check health  
curl http://localhost:3000/pjs/health

# View metrics
curl http://localhost:3000/examples/metrics
```

## Performance Goals

- **Throughput**: >4 GB/s with sonic-rs
- **Time to First Byte**: <10ms for critical data
- **Memory Efficiency**: 5-10x reduction vs traditional parsing
- **CPU Utilization**: Full SIMD acceleration

## Building

### Prerequisites

- Rust 1.85+
- CPU with AVX2 support (recommended for SIMD acceleration)

### Quick Start

```bash
# Clone repository
git clone https://github.com/bug-ops/pjs
cd pjs

# Build with optimizations
cargo build --release

# Run tests
cargo test --workspace

# Run the complete HTTP server example
cargo run --example axum_server

# Build with optional features
cargo build --features "http-client,prometheus-metrics"
```

### Feature Flags

- `http-client`: Enable HTTP-based event publishing
- `prometheus-metrics`: Enable Prometheus metrics collection
- `simd-auto`: Auto-detect best SIMD support (default)
- `compression`: Enable compression middleware

## Production Features

### Middleware Stack

The HTTP server includes production-ready middleware:

```rust
use pjson_rs::infrastructure::http::middleware::*;

let app = create_pjs_router()
    .layer(axum::middleware::from_fn(pjs_cors_middleware))
    .layer(axum::middleware::from_fn(security_middleware))
    .layer(axum::middleware::from_fn(health_check_middleware))
    .layer(PjsMiddleware::new()
        .with_compression(true)
        .with_metrics(true)
        .with_max_request_size(10 * 1024 * 1024))
    .with_state(app_state);
```

### Monitoring & Metrics

Built-in Prometheus metrics support:

```rust
// Automatically tracks:
// - pjs_active_sessions
// - pjs_total_sessions_created  
// - pjs_frames_processed_total
// - pjs_bytes_streamed_total
// - pjs_frame_processing_time_ms

let metrics = collector.export_prometheus();
// Expose at /metrics endpoint for Prometheus scraping
```

### Event System

Comprehensive domain event tracking:

```rust
// Events automatically generated:
// - SessionCreated, SessionActivated, SessionEnded
// - StreamStarted, StreamCompleted, FrameGenerated
// - PriorityAdjusted, ErrorOccurred

publisher.subscribe("SessionCreated", |event| {
    println!("New session: {}", event.session_id());
});
```

## Contributing

We welcome contributions! See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

### Development Setup

```bash
# Install development tools
rustup component add clippy rustfmt

# Run checks
cargo clippy --workspace
cargo fmt --check

# Run all tests
cargo test --workspace --all-features
```

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.

## Getting Started Right Now

Want to try PJS immediately? Here's the fastest way:

```bash
# Clone and run
git clone https://github.com/bug-ops/pjs
cd pjs
cargo run --example axum_server

# In another terminal, test the API
curl -X POST http://localhost:3000/pjs/sessions \
  -H "Content-Type: application/json" \
  -d '{"max_concurrent_streams": 5}'

# Try Server-Sent Events streaming  
curl -N -H "Accept: text/event-stream" \
  http://localhost:3000/pjs/stream/{session_id}/sse
```

### Running Performance Benchmarks

To verify the performance claims, run the comprehensive benchmark suite:

```bash
# Run all benchmarks
cargo bench -p pjs-bench

# Or run specific benchmarks:
cargo bench -p pjs-bench --bench simple_throughput    # Core parsing speed
cargo bench -p pjs-bench --bench memory_benchmarks    # Memory efficiency  
cargo bench -p pjs-bench --bench streaming_benchmarks # Progressive loading
```

Results show PJS **6.3x faster** than serde_json and **1.06x faster** than sonic-rs on large JSON.

The server will show:

- 🚀 Server starting message
- 📊 Health check endpoint
- 📝 Available API endpoints
- 🎯 Demo data streaming capabilities

## Roadmap

### Next Steps

- [x] Connection lifecycle management ✅
- [x] WebSocket real-time streaming ✅
- [x] Performance benchmarks vs alternatives ✅
- [ ] JavaScript/TypeScript client library
- [ ] Schema validation engine  
- [ ] Custom priority strategies

## Acknowledgments

Built with:

- [sonic-rs](https://github.com/cloudwego/sonic-rs) - Lightning fast SIMD JSON parser
- [axum](https://github.com/tokio-rs/axum) - Ergonomic web framework for Rust  
- [tokio](https://github.com/tokio-rs/tokio) - Async runtime for Rust
- [bytes](https://github.com/tokio-rs/bytes) - Efficient byte buffer management

## Community

- 📖 [Documentation](SPECIFICATION.md) - Complete protocol specification
- 📋 [Changelog](CHANGELOG.md) - Detailed version history
- 📊 [Benchmarks](crates/pjs-bench/README.md) - Comprehensive performance results
- 💬 [Discussions](https://github.com/bug-ops/pjs/discussions) - Questions and ideas

---

*PJS: Because users shouldn't wait for data they don't need yet.*
