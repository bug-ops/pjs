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

- Use `parser::buffer_pool::{AlignedBuffer, BufferPool}` for reusable, aligned scratch buffers
- Bounded memory usage prevents DoS attacks
- Depth tracking prevents stack overflow in nested JSON

## SIMD Feature Flags

- `simd-auto` - Enable the sonic-rs SIMD backend, runtime-dispatched to the best available instruction set (AVX-512/AVX2/SSE4.2/NEON) for the host CPU (default)
- `simd-avx512` - x86_64-only. Additionally forwards to `sonic-rs/avx512`; requires `RUSTFLAGS="-C target-cpu=native"` to take effect

## Memory Safety

- All `unsafe` blocks require `#![deny(unsafe_op_in_unsafe_fn)]` compliance
- 100% test coverage for any unsafe code
- Bounded buffer-pool allocations to prevent DoS

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
