---
applyTo: "crates/pjs-core/src/application/**/*.rs"
---

# Application Layer Instructions

Use case orchestration with CQRS pattern.

## CQRS Pattern

- Commands modify state, return `ApplicationResult<()>`
- Queries read state, return `ApplicationResult<T>`
- Separate handlers for commands vs queries

## Absolute Constraints

- NEVER contain business logic - orchestration only
- NEVER access infrastructure directly - use injected ports
- ALWAYS delegate business rules to domain services

## Commands (`application/commands/`)

- `CreateSessionCommand`
- `StartStreamCommand`
- `StopStreamCommand`

Commands are intent to change state. Validation happens in domain.

## Queries (`application/queries/`)

- `GetSessionQuery`
- `ListActiveSessionsQuery`
- `GetMetricsQuery`

Queries read state without side effects.

## Handlers

- `CommandHandler` - processes commands
- `QueryHandler` - processes queries

Handlers orchestrate domain services and ports.

## DTOs (`application/dto/`)

- Data transfer objects for external communication
- Convert between domain types and external formats
- Validation at boundaries

## Services

- `SessionService` - session lifecycle orchestration
- `StreamingService` - stream orchestration

Services compose domain operations into use cases.

## Testing

- 70% minimum coverage
- Mock domain ports for isolation
- Test orchestration flow, not business logic
- Test error propagation from domain
