---
applyTo: "crates/pjs-core/src/parser/**/*.rs"
---

# Parser Layer Instructions

SIMD-accelerated JSON parsing achieving 6.3x faster performance than serde_json.

## Performance Requirements

- Target: 6.3x faster than serde_json for priority-based streaming
- ALWAYS benchmark before/after changes in this directory
- NEVER introduce allocations in hot paths without justification

```bash
cargo bench -p pjs-bench -- --save-baseline before
# Make changes
cargo bench -p pjs-bench -- --baseline before
```

## Zero-Copy Operations

- Use `JsonArena` for arena allocation
- Bounded memory usage prevents DoS attacks
- Depth tracking prevents stack overflow in nested JSON

## SIMD Feature Flags

Available optimizations (mutually exclusive except simd-auto):

- `simd-auto` - Auto-detect best SIMD for platform (default)
- `simd-sse42` - SSE 4.2
- `simd-avx2` - AVX2
- `simd-avx512` - AVX-512
- `simd-neon` - ARM NEON

## Memory Safety

- All `unsafe` blocks require `#![deny(unsafe_op_in_unsafe_fn)]` compliance
- 100% test coverage for any unsafe code
- Bounded arena allocations to prevent DoS

## Security Requirements

- 100% test coverage for parser security checks
- Input validation for all external data
- Depth limits for nested structures
- Size limits for strings and arrays

## Hot Path Locations

Changes to these require benchmarking:

- `sonic.rs` - SIMD parsing core
- `zero_copy.rs` - Zero-copy string handling
- Token iteration paths
