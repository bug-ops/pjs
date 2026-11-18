---
aliases:
  - Buffer Pool and Aligned Allocator
  - BufferPool
  - AlignedBuffer
tags:
  - sdd
  - spec
  - parser
  - memory
  - unsafe
  - simd
created: 2026-11-15
status: implemented
priority: P2
code:
  - crates/pjs-core/src/parser/buffer_pool.rs
  - crates/pjs-core/src/parser/aligned_alloc.rs
  - crates/pjs-core/tests/parser_allocator_comprehensive.rs
  - crates/pjs-core/src/config/security.rs
related:
  - "[[038-zero-copy-lazy-json-parser/spec]]"
  - "[[041-simd-features-and-global-allocator/spec]]"
  - "[[005-string-arena-safe-interning/spec]]"
  - "[[051-security-limits-and-validated-config/spec]]"
verified: 2026-11-15
---

# Spec 040: Buffer Pool and Aligned Allocator

Part of [[MOC-specs]]. Rules in [[constitution]]. Plan: [[040-buffer-pool-aligned-alloc/plan|plan]].

> [!info] Context
> The arena allocator (`memory::arena`, see [[005-string-arena-safe-interning/spec|005]]) was removed from the crate (CHANGELOG `[Unreleased]` / Removed). `parser::buffer_pool` and `parser::aligned_alloc` are what remains of the "memory" subsystem. Nothing in production code outside this module acquires pooled buffers: the only users are `examples/zero_copy_demo.rs` and `tests/zero_copy_integration_test.rs`.

## Overview

**Problem.** Parsing and framing want reusable, SIMD-aligned byte buffers with a hard memory budget, instead of repeated allocation.

**Goal.** A size-bucketed pool of aligned buffers with RAII return-to-pool, security-validated sizes, a total-memory budget, per-bucket caps, TTL cleanup, and statistics; plus a zero-sized `AlignedAllocator` over `std::alloc` that honours the process `#[global_allocator]`.

**Out of scope.** Choosing the global allocator ([[041-simd-features-and-global-allocator/spec|041]]); a string interner; streaming compression buffers.

## User Stories

- As a parser author, I acquire a 64-byte aligned buffer, fill it, and have it returned to the pool when dropped.
- As an operator, I cap total pooled memory and idle lifetime through the security config presets.

## Functional Requirements

### FR-040-001 Aligned allocation primitive

`AlignedAllocator::alloc_aligned(size, alignment)` shall return `DomainError::InvalidInput` when `size` is zero, `alignment` is not a power of two or the `Layout` is invalid, and `DomainError::ResourceExhausted` when the allocator returns null. `realloc_aligned` and `dealloc_aligned` are `unsafe fn` and require the pointer and layout from `alloc_aligned`.
Code: `parser/aligned_alloc.rs`. Tests: `tests/parser_allocator_comprehensive.rs`, unit tests in the module.
Status: done.

### FR-040-002 Allocator is stateless

`AlignedAllocator` shall be a zero-sized `Copy` type; `aligned_allocator()` shall return a `&'static` singleton. All calls shall route through `std::alloc`, hence through the registered `#[global_allocator]`.
Status: done.

### FR-040-003 AlignedBuffer construction

`AlignedBuffer::new(capacity, alignment)` shall reject a non-power-of-two alignment and an alignment above 4096 with `InvalidInput`; raise alignment to at least `align_of::<usize>()`; round capacity up to the alignment with checked arithmetic (overflow returns `InvalidInput`, never wraps); and use at least one alignment unit of capacity (zero capacity yields capacity == alignment). `new_sse` (16), `new_avx2` (32), `new_avx512` (64) are shorthands.
Status: done. Tests cover overflow and wraparound boundaries.

### FR-040-004 AlignedBuffer growth

`reserve(additional)` shall compute `len + additional` with checked addition, round to alignment with checked addition, reject a zero result or a result above `isize::MAX` with `InvalidInput`, and otherwise `realloc` preserving contents and alignment. `extend_from_slice` shall `reserve` as needed and copy. On a rejected `reserve` the buffer shall stay usable and unchanged.
Status: done.

### FR-040-005 Uninitialised-memory discipline

`as_slice`/`as_mut_slice` shall expose only `[0, len)`. `as_mut_capacity_slice()` shall expose the full capacity as `&mut [MaybeUninit<u8>]`. `unsafe fn set_len(new_len)` requires `new_len <= capacity` and that the first `new_len` bytes are initialised.
Status: done. Test: `test_reused_buffer_exposes_no_stale_bytes`.

### FR-040-006 Pool buckets

`BufferSize` shall have exactly five variants: `Small` = 1 KiB, `Medium` = 8 KiB, `Large` = 64 KiB, `XLarge` = 512 KiB, `Huge` = 4 MiB. `BufferSize::for_capacity(n)` shall return `Some` of Small for `0..=1024`, Medium for `..=8192`, Large for `..=65536`, XLarge for `..=524288`, Huge for `..=4194304`, and `None` for every larger value.
Status: done. `acquire_with_capacity(n)` with `n` above 4 MiB returns `DomainError::InvalidInput` instead of a smaller buffer (#747).

### FR-040-007 Acquire

`BufferPool::acquire(size)` shall: validate `size` against `buffers.max_buffer_size` (`SecurityViolation`); reuse a pooled buffer of the same size if any (cache hit, never fails on the budget); otherwise, when the live bytes (handed out plus retained) plus the size-class capacity of `size` would exceed `max_total_memory`, evict idle buffers (largest size class first, oldest first) until it fits and retry the reservation once, rejecting with `ResourceExhausted` (message contains "memory limit") only if it still does not fit; without concurrent acquires that means the checked-out bytes plus the class exceed the limit, while a racing caller may take the freed room and make the call fail safe (#773). Then allocate a new aligned buffer (cache miss). Every buffer is charged its size-class capacity (`size` rounded up to the effective alignment) from allocation until it is freed; `current_memory_usage()` returns the live bytes in O(1). A returned `PooledBuffer` shall have `len == 0`.
Status: done.

### FR-040-008 Return on drop

When a `PooledBuffer` is dropped, the pool shall clear it and push it back to its bucket only if its capacity equals its size-class capacity, its layout alignment equals the pool's effective alignment, and the bucket holds fewer than `max_buffers_per_bucket` buffers; otherwise the buffer is freed and its charge released. A buffer grown or replaced while checked out is therefore freed; that growth is outside the budget.
Status: done. Test: `test_per_bucket_limit_enforcement`.

### FR-040-009 Cleanup

`BufferPool::cleanup()` shall free idle buffers older than `buffer_ttl` since last use, remove buckets that are empty and not accessed within the TTL, and return `CleanupStats { freed_buffers, freed_memory }`. Each freed buffer releases its charge from the live bytes. It is never invoked automatically.
Status: done.

### FR-040-010 Configuration

`PoolConfig` fields are private and always derive from a `ValidatedSecurityConfig`: `Default` (alignment 32), `From<&ValidatedSecurityConfig>`, `from_security_config`, presets `simd_optimized()` (alignment 64, high-throughput limits), `low_memory()` (stats off), `development()`. `BufferPool::new`, `with_config`, `with_security_config` construct pools.
Status: done.

### FR-040-011 Global pool

`global_buffer_pool()` shall lazily create a process-wide pool (`OnceLock`). `initialize_global_buffer_pool(config)` shall set it once and return `DomainError::InternalError` if it was already initialised (including by an earlier `global_buffer_pool()` call).
Status: done.

### FR-040-012 Statistics

When `track_stats` is true the pool shall count `total_allocations`, `cache_hits`, `cache_misses`, `cleanup_count`. `PoolStats::hit_ratio()` returns hits / allocations.
`PoolStats::current_memory_usage` and `peak_memory_usage` report the live bytes and their high-water mark regardless of `track_stats`.
Status: done (#747).

### FR-040-013 SIMD compatibility queries

`AlignedBuffer::is_aligned`, `actual_alignment`, and `is_simd_compatible(SimdType::{Sse, Avx2, Avx512, Neon})` (required alignments 16/32/64/16) shall report real pointer alignment.
Status: done.

## Non-Functional Requirements

- NFR-SEC-040-001: every `unsafe` block carries a `SAFETY` comment; `unsafe_op_in_unsafe_fn` is denied crate-wide (`#![deny(unsafe_op_in_unsafe_fn)]` in `lib.rs`).
- NFR-SEC-040-002: size arithmetic uses checked operations (FR-040-003/004); regression tests exist for `usize::MAX` and the wraparound boundary.
- NFR-PERF-040-001: `acquire` is O(1) with a single stats lock and no thread-pool fan-out; pooled reuse avoids allocation; verified only by the cache-hit counters in tests, no dedicated benchmark `[NEEDS CLARIFICATION: no benches/ file targets the buffer pool; confirm none is planned]`.
- NFR-CONF-040-001: default limits come from `BufferLimits::default()`: max buffer 256 MiB, total 512 MiB, TTL 300 s, 50 buffers per bucket.

## Data Model

| Type | Notes |
|---|---|
| `AlignedAllocator` | ZST, `Debug + Default + Clone + Copy` |
| `AlignedBuffer` | `ptr: NonNull<u8>`, `len`, `capacity`, `alignment`, `layout`, timestamps; `unsafe impl Send + Sync`; `Clone` deep-copies |
| `BufferPool` | `Arc<DashMap<BufferSize, BufferBucket>>`, `PoolConfig`, `Arc<parking_lot::Mutex<PoolStats>>`, `Arc<MemoryBudget>` (atomic live and peak bytes) |
| `PooledBuffer` | owns `Option<AlignedBuffer>` plus the pool handle; `buffer()`/`buffer_mut()` return `Option` |
| `PoolConfig` | `max_buffers_per_bucket`, `max_total_memory`, `buffer_ttl`, `track_stats`, `simd_alignment`, `SecurityValidator` |
| `PoolStats`, `CleanupStats`, `SimdType`, `BufferSize` | as above |

## Edge Cases and Errors

| Case | Error |
|---|---|
| alignment 0, 3, 17 | `InvalidInput("Alignment .. is not a power of 2")` |
| alignment > 4096 | `InvalidInput` |
| capacity near `usize::MAX` | `InvalidInput` ("overflows when rounding") |
| `reserve` result > `isize::MAX` | `InvalidInput` |
| allocator returns null | `ResourceExhausted` |
| `acquire` size above `max_buffer_size` | `SecurityViolation` |
| `acquire` over `max_total_memory` | idle buffers evicted first; `ResourceExhausted` ("memory limit") only if checked-out bytes leave no room |
| second `initialize_global_buffer_pool` | `InternalError` |

## Success Criteria

- `cargo nextest run -p pjson-rs` passes the buffer pool and allocator tests under both the system and `mimalloc` configurations (CI matrix).
- `cargo test --doc` passes the `as_mut_capacity_slice` example.

## Agent Boundaries

- Always: keep a `SAFETY:` comment on every `unsafe` block; use checked arithmetic for any size derived from a caller.
- Ask first: changing `BufferSize` discriminants (public, used as byte sizes); adding automatic background cleanup.
- Never: allocate aligned memory with raw FFI; bypass `SecurityValidator::validate_buffer_size`.

## Open Questions

- `AlignedBuffer::clone` uses `.expect("Failed to clone buffer")`, a panic path on allocation failure.
- `BufferLimits::max_pool_size` exists in the security config presets but is not read by `BufferPool`.
- `PooledBuffer::buffer()`/`buffer_mut()` return `Option` although the inner value is always `Some` until drop.
- A dropped buffer is pooled without checking `max_total_memory` or its TTL; only `cleanup()` removes stale buffers, and nothing calls it.
- `AlignedBuffer::new` allocates uninitialised memory; `len` starts at 0 so reads are bounded, but `as_mut_capacity_slice` is the only safe way to write into the spare capacity.
- No production caller: usage is limited to `examples/zero_copy_demo.rs` and `tests/zero_copy_integration_test.rs`; no benchmark covers the pool.

## See Also

- [[040-buffer-pool-aligned-alloc/plan|Plan (as built)]]
- [[041-simd-features-and-global-allocator/spec|041 SIMD features and global allocator]]
- [[051-security-limits-and-validated-config/spec|051 Security limits]] (`BufferLimits`, `ValidatedSecurityConfig`)
