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
impl StreamRepositoryGat for InMemoryStreamRepository {
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

- `InMemoryStreamRepository` - implements `StreamRepositoryGat`
- `InMemoryEventPublisher` - implements `EventPublisherGat`
- `InMemoryMetricsCollector` - metrics collection

## HTTP Integration (`infrastructure/http/`)

- Axum handlers in `axum_adapter.rs`
- Middleware in `middleware.rs`
- Streaming responses in `streaming.rs`

## Testing

- 60% minimum coverage
- Integration tests with real implementations
- Test concurrency with tokio multi-threaded runtime
- Test lock-free behavior under contention
