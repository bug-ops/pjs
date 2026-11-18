---
applyTo: "crates/pjs-core/src/infrastructure/**/*.rs"
---

# Infrastructure Layer Instructions

Concrete implementations of domain ports with thread-safe concurrency.

## Domain Port Implementation

- ALWAYS implement domain port traits (GATs)
- NEVER leak infrastructure concerns into domain layer
- Use dependency injection for all external dependencies

## GAT Implementation Pattern

```rust
impl StreamRepositoryGat for GatInMemoryStreamRepository {
    type FindFuture<'a> = impl Future<Output = DomainResult<Option<Stream>>> + Send + 'a
    where Self: 'a;

    fn find(&self, id: &StreamId) -> Self::FindFuture<'_> {
        async move {
            // implementation
        }
    }
}
```

## Lock-Free Concurrency

- ALWAYS use `DashMap` for concurrent access (never `Mutex<HashMap>`)
- ALWAYS use `Arc` for zero-copy sharing of immutable data
- NEVER use `Mutex` in hot paths
- NEVER hold locks across await points

```rust
// WRONG - lock held across await
let guard = map.lock().await;
let value = guard.get(&key);
some_async_op(value).await;  // BAD!

// CORRECT - clone and release
let value = {
    let guard = map.lock().await;
    guard.get(&key).cloned()
};
some_async_op(&value).await;
```

## Adapters (`infrastructure/adapters/`)

- `GatInMemoryStreamRepository` - implements `StreamRepositoryGat`
- `GatInMemoryStreamStore` - implements `StreamStoreGat`
- `InMemoryFrameStore` - frame storage
- `InMemoryEventPublisher` - implements `EventPublisherGat`
- `InMemoryMetricsCollector` - metrics collection

## HTTP Integration (`infrastructure/http/`)

- Router assembly, shared state, and error mapping in `axum_adapter.rs`
- Route handlers grouped by domain concept in `handlers/{sessions,streams,health,dictionary}.rs`
- Middleware in `middleware.rs`; API key and JWT layers in `auth.rs`
- Streaming responses (SSE and NDJSON) in `streaming.rs`
- Connection limits and graceful shutdown in `serve.rs` and `shutdown.rs` (`serve_with_limits`, `ShutdownSignal`)
- Prometheus `/metrics` in `metrics.rs` (`metrics` feature)
- No stub middleware or placeholder extension points: an HTTP component either works end to end or does not exist

## WebSocket Integration (`infrastructure/websocket/`)

- Wire protocol and message types in `protocol.rs`; server, client, controller and planner in their own modules
- Every inbound limit (frame size, message rate, sessions per connection, write timeout) is explicit and bounded
- The server drains on the same `ShutdownSignal` as the HTTP listener

## Testing

- 60% minimum coverage
- Integration tests with real implementations
- Test concurrency with tokio multi-threaded runtime
- Test lock-free behavior under contention
