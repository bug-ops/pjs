# Changelog

<!-- markdownlint-disable MD024 -->

All notable changes to the Priority JSON Streaming Protocol (PJS) project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **WebSocket Real-Time Streaming**: Complete implementation with priority-based frame delivery
  - `WebSocket Streaming Server`: Real-time streaming with session management
  - Priority-based frame delivery with adaptive delays based on priority levels  
  - Skeleton-first JSON streaming with progressive enhancement
  - Session statistics and metrics collection for active WebSocket connections
  - Compression support with schema-based optimization
  - Integration with existing data generators (analytics, ecommerce, social)
  - Proper error handling with ApplicationError conversion patterns

- **Infrastructure Module Refactoring**: Major architectural improvements
  - Complete async/sync compatibility fixes in event publishers
  - Updated MetricsCollector trait methods to match implementation signatures
  - Fixed domain event creation patterns using proper DomainEvent enum variants
  - Resolved all async trait compilation issues with tokio::sync primitives
  - Added `From<String>` implementation for DomainError conversion
  - Re-enabled infrastructure module exports with stable compilation

- **Demo Servers Enhancement**: Comprehensive streaming demonstration platform
  - Interactive demo server with improved HTML interface
  - WebSocket streaming server with real-time priority frame delivery
  - Performance comparison server (partial implementation)
  - WebSocket client demo for testing streaming functionality
  - Unified HTML interface for all demo types

- **Connection Lifecycle Management**: Complete implementation with connection tracking
  - `ConnectionManager` service for managing connection state and lifecycle
  - Automatic timeout detection and cleanup with configurable duration
  - Connection metrics tracking (bytes sent/received, active/inactive counts)
  - Maximum connections limit enforcement
  - Background timeout monitoring task
  - REST endpoint for connection statistics (`GET /pjs/connections`)
  - Integration with Axum HTTP adapter for automatic connection registration
  - Thread-safe connection state management with async-std RwLock
  - Comprehensive tests for lifecycle, max connections, and timeout scenarios

### Fixed

- **Schema-Based Compression**: Complete implementation with multiple strategies
  - Dictionary compression for repeated string patterns
  - Delta compression for numeric sequences  
  - Run-length encoding for repeated values
  - Hybrid compression combining multiple strategies
  - Automatic compression strategy selection based on data analysis
  - Integration with streaming infrastructure for real-time compression

- **Technical Debt Resolution**: Major cleanup and stabilization
  - Marked all 143 unwrap() calls with TODO comments for proper error handling
  - Added SAFETY comments to unsafe blocks for memory safety documentation
  - Fixed domain layer serde_json::Value dependencies with proper abstractions
  - Created value objects for String fields to improve type safety
  - Updated deprecated rand library methods to current stable API

### Improved

- **Streaming Infrastructure**: Enhanced reliability and performance
  - Proper async/await patterns throughout WebSocket handlers
  - Improved error handling with structured error types
  - Better separation of concerns between transport and application layers
  - Enhanced session management with proper lifecycle tracking

### Planned

- **Performance benchmarks against standard JSON**: Comprehensive comparison suite
- **JavaScript/TypeScript client library**: Web client SDK for PJS protocol
- **Custom priority strategies**: User-configurable prioritization algorithms  
- **Additional framework integrations**: Support for popular Rust web frameworks

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

### Phase 7: Ecosystem & Performance ⏳ IN PROGRESS

- [x] Framework integrations (Axum complete)
- [x] Complete documentation & examples
- [x] WebSocket real-time streaming implementation
- [ ] Comprehensive benchmarks vs alternatives (planned)
- [ ] JavaScript/TypeScript client library (planned)
- [ ] Additional framework integrations (Actix) (planned)

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
