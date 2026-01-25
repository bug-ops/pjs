---
applyTo: "**/tests/**/*.rs"
---

# Testing Instructions

Test organization and quality standards.

## Test Runner

ALWAYS use nextest instead of cargo test:

```bash
cargo nextest run --workspace --all-features
cargo nextest run -E 'test(test_name)'          # Specific test
cargo nextest run -p pjson-rs                   # Specific crate
cargo nextest run --nocapture                   # Show output
```

## Coverage Thresholds

| Layer | Minimum | Rationale |
|-------|---------|-----------|
| Security | 100% | Security-critical code |
| Domain | 80% | Business logic |
| Application | 70% | Orchestration |
| Infrastructure | 60% | Adapters |

```bash
cargo llvm-cov nextest --workspace --html
cargo llvm-cov nextest --workspace --summary-only
```

## Test Organization

### Unit Tests

- Domain: Pure functions, no mocks needed
- Application: Mock domain ports
- Infrastructure: Integration tests

### Integration Tests

Located in `crates/*/tests/`:

- `schema_validation_integration.rs` - End-to-end validation
- HTTP endpoint tests

### Performance Tests

Located in `crates/pjs-bench/benches/`:

- `simple_throughput.rs` - Parser performance
- `memory_benchmarks.rs` - Arena allocation
- `streaming_benchmarks.rs` - Progressive loading

## Test Quality Requirements

- ALWAYS test all public APIs
- ALWAYS test error paths
- ALWAYS test edge cases and boundary conditions
- ALWAYS test security-critical code comprehensively
- NEVER allow unsafe code without 100% coverage

## Property-Based Testing

Use proptest for value objects:

- Test invariants hold for all inputs
- Test roundtrip serialization
- Test boundary conditions automatically

## Naming Convention

```rust
#[test]
fn test_session_creation_with_valid_priority() { }

#[test]
fn test_session_creation_fails_with_invalid_priority() { }
```

Use descriptive names: `test_<unit>_<scenario>_<expected_result>`
