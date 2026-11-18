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

- `CreateSessionCommand`, `CloseSessionCommand`
- `CreateStreamCommand`, `StartStreamCommand`, `CompleteStreamCommand`
- `GenerateFramesCommand`, `BatchGenerateFramesCommand`

Commands are intent to change state. Validation happens in domain.

## Queries (`application/queries/`)

- `GetSessionQuery`, `GetActiveSessionsQuery`, `SearchSessionsQuery`
- `GetStreamQuery`, `GetStreamsForSessionQuery`, `GetStreamFramesQuery`
- `GetSessionHealthQuery`, `GetSessionStatsQuery`, `GetSystemStatsQuery`

Queries read state without side effects.

## Handlers

- `SessionCommandHandler` - processes commands
- `SessionQueryHandler`, `StreamQueryHandler`, `SystemQueryHandler` - process queries

Handlers are generic over domain ports (for example `StreamRepositoryGat`, `EventPublisherGat`) and orchestrate them; they never name a concrete adapter.

## DTOs (`application/dto/`)

- Data transfer objects for external communication
- Convert between domain types and external formats
- Validation at boundaries

## Testing

- 70% minimum coverage
- Mock domain ports for isolation
- Test orchestration flow, not business logic
- Test error propagation from domain
