---
applyTo: "crates/pjs-core/src/domain/**/*.rs"
---

# Domain Layer Instructions

Pure business logic with zero external dependencies.

## Absolute Constraints

- NEVER import from `application/` or `infrastructure/` modules
- NEVER use `serde_json::Value` - use `JsonData` value object instead
- NEVER use external I/O (network, filesystem, database)
- NEVER use `tokio::spawn` or runtime-specific async primitives

## Value Objects (`domain/value_objects/`)

- ALWAYS immutable after creation
- ALWAYS implement `Clone`, `Debug`, `PartialEq`, `Eq`
- ALWAYS use newtype pattern for type safety (`SessionId`, `Priority`)
- ALWAYS validate in constructor, never after creation

## Domain Services (`domain/services/`)

- Pure functions operating on domain types only
- No side effects (logging, metrics, I/O)
- Dependency injection via GAT-based ports
- Return `DomainResult<T>` for errors

## Ports (Traits) (`domain/ports/`)

Use Generic Associated Types for zero-cost async:

```rust
pub trait StreamRepositoryGat {
    type FindFuture<'a>: Future<Output = DomainResult<Option<Stream>>> + Send + 'a
    where Self: 'a;
}
```

## Domain Events (`domain/events/`)

- Immutable after creation
- Named in past tense (`SessionCreated`, `StreamStarted`)

## Testing

- 80% minimum coverage required
- Unit tests in same file or `tests/` module
- Property-based tests for value objects using proptest
- No mocks needed - pure functions
