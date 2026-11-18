---
aliases:
  - Buffer Pool Plan (as built)
tags:
  - sdd
  - plan
  - parser
  - memory
created: 2026-11-15
status: implemented
related:
  - "[[040-buffer-pool-aligned-alloc/spec]]"
verified: 2026-11-15
---

# Plan 040: Buffer Pool and Aligned Allocator (as built)

Spec: [[040-buffer-pool-aligned-alloc/spec|040]]. Index: [[MOC-specs]].

## Architecture

```mermaid
flowchart LR
  C["caller"] -->|acquire(BufferSize)| BP["BufferPool"]
  BP -->|validate_buffer_size| SV["SecurityValidator"]
  BP -->|"idle bytes + size <= max_total_memory"| CHK{"budget"}
  BP --> DM["DashMap<BufferSize, BufferBucket>"]
  DM -->|hit: pop| PB["PooledBuffer"]
  DM -->|miss| AB["AlignedBuffer::new"]
  AB --> AA["AlignedAllocator (ZST)"]
  AA --> GA["std::alloc -> #[global_allocator]"]
  PB -->|Drop: clear + push if bucket < max| DM
  BP -->|cleanup: TTL sweep| DM
```

Layers: `aligned_alloc.rs` (unsafe allocation primitive, `DomainResult`), `buffer_pool.rs` (safe wrapper types plus pool). Both live under `parser/` for historical reasons but have no JSON knowledge.

## Design decisions

- Buckets are five fixed sizes, each 8x the previous (1 KiB .. 4 MiB); no per-request sizing.
- `DashMap` gives per-shard locking for buckets; the stats counters sit behind one `parking_lot::Mutex` because they are write-light.
- Memory is tracked incrementally in a shared atomic `MemoryBudget` (live and peak bytes), charging each buffer its size-class capacity from allocation until it is freed. `max_total_memory` bounds handed-out plus retained bytes, `current_memory_usage()` is O(1), and `PoolStats` reports the same live and peak values. A buffer is re-pooled on drop only while it still has its class capacity and layout alignment (#747).
- Config fields are private and derived only from `ValidatedSecurityConfig` so the pool limits and its validator cannot diverge.
- The allocator is a ZST so `AlignedBuffer` can call `aligned_allocator()` in `Drop` without storing a handle.

## Public API surface

`pjson_rs::parser::{AlignedAllocator, aligned_allocator, BufferPool, BufferSize, PoolConfig, PooledBuffer, SimdType, global_buffer_pool}` and, from `parser::buffer_pool`: `AlignedBuffer`, `PoolStats`, `CleanupStats`, `initialize_global_buffer_pool`.

## Contracts

| Function | Preconditions | Postconditions | Undefined / notes |
|---|---|---|---|
| `AlignedAllocator::alloc_aligned` (unsafe) | none beyond caller owning the result | non-null pointer aligned to `alignment`, valid for `size` bytes, uninitialised | `size == 0` is passed to `Layout`/`alloc`; zero-size allocation through `std::alloc::alloc` is undefined behaviour, callers (`AlignedBuffer::new`) never pass 0 |
| `AlignedAllocator::realloc_aligned` (unsafe) | `ptr` and `old_layout` from `alloc_aligned`; `new_size > 0`; rounded size <= `isize::MAX` | pointer valid for `new_size`, old pointer invalid | preconditions are not validated here |
| `AlignedAllocator::dealloc_aligned` (unsafe) | same ptr and exact layout | memory freed | double free or layout mismatch is UB |
| `AlignedBuffer::new` | alignment power of two <= 4096 | `capacity >= max(requested, alignment)`, pointer aligned | uninitialised contents, `len == 0` |
| `AlignedBuffer::set_len` (unsafe) | `new_len <= capacity`, prefix initialised | `len == new_len` | only a `debug_assert` backs the bound |
| `BufferPool::acquire` | none | `len == 0` buffer, capacity >= size | errors per spec table |
| `PooledBuffer` drop | none | buffer cleared and pooled or freed | |
| `initialize_global_buffer_pool` | called before first `global_buffer_pool()` | pool configured | later call returns `InternalError` |

## Testing strategy

- Unit tests in `buffer_pool.rs` (alignment, overflow, reserve boundaries, pooling, limits, SIMD constructors, memory safety) and `aligned_alloc.rs`.
- Integration: `tests/parser_allocator_comprehensive.rs` (alignment matrix, realloc grow/shrink/same, concurrency via `Arc`, `global_allocator_name`), `tests/zero_copy_integration_test.rs` (`test_buffer_pool_integration`).
- CI runs the suite twice per OS: system allocator and `mimalloc` ([[078-ci-pipeline/spec|078]]).
- No fuzz target and no benchmark for the pool.

## Security notes

- The memory-safety surface is: `NonNull<u8>` ownership in `AlignedBuffer`, `Send`/`Sync` impls (sound because there is no interior mutability and mutation requires `&mut`), `slice::from_raw_parts` bounded by `len <= capacity`.
- Resource bounds: `max_buffer_size`, `max_total_memory` (idle only), `max_buffers_per_bucket`, TTL sweep on demand.
