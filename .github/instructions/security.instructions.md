---
applyTo: "crates/pjs-core/src/security/**/*.rs"
---

# Security Layer Instructions

Security-critical code requiring complete test coverage.

## Coverage Requirement

- 100% test coverage required - no exceptions
- All error paths must be tested
- Edge cases and boundary conditions mandatory

## Bounded Allocations

- ALWAYS use bounded allocations to prevent DoS
- NEVER allow unbounded memory growth from external input
- Arena allocations must have size limits

## Rate Limiting

- ALWAYS implement rate limiting for external-facing APIs
- Use token bucket or sliding window algorithms
- Configuration must be tunable per deployment

## Compression Bomb Prevention

- Detect and reject decompression bombs
- Enforce maximum decompressed size limits
- Track compression ratios for anomaly detection

## Unsafe Code Requirements

- ALL `unsafe` blocks require `#![deny(unsafe_op_in_unsafe_fn)]`
- 100% test coverage for unsafe code
- Document justification for each unsafe block
- Minimize unsafe scope to smallest possible region

## Error Handling

- Error messages NEVER leak sensitive information
- NEVER expose internal paths or stack traces to users
- Log detailed errors server-side, return generic messages to clients

## Input Validation

- Validate all external input at boundaries
- Enforce depth limits for nested structures
- Enforce size limits for all variable-length data
- Reject malformed input early

## Testing Checklist

- [ ] All public functions have tests
- [ ] Error paths tested
- [ ] Boundary conditions tested
- [ ] Malicious input patterns tested
- [ ] Resource exhaustion scenarios tested
