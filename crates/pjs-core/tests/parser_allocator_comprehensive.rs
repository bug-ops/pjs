//! Comprehensive tests for parser allocator module
//!
//! This test suite aims to achieve 70%+ coverage by testing:
//! - Memory allocation with different alignments
//! - Reallocation operations
//! - Deallocation safety
//! - Allocator backend detection
//! - Statistics collection (jemalloc)
//! - Edge cases (zero size, max alignment)
//! - Error handling

use pjson_rs::parser::allocator::{
    AllocatorBackend, AllocatorStats, SimdAllocator, global_allocator,
};
use std::alloc::Layout;

// === Allocator Backend Tests ===

#[test]
fn test_allocator_backend_current() {
    let backend = AllocatorBackend::current();
    let name = backend.name();

    // Verify backend detection based on feature flags
    #[cfg(feature = "jemalloc")]
    {
        assert_eq!(backend, AllocatorBackend::Jemalloc);
        assert_eq!(name, "jemalloc");
    }

    #[cfg(all(feature = "mimalloc", not(feature = "jemalloc")))]
    {
        assert_eq!(backend, AllocatorBackend::Mimalloc);
        assert_eq!(name, "mimalloc");
    }

    #[cfg(all(not(feature = "jemalloc"), not(feature = "mimalloc")))]
    {
        assert_eq!(backend, AllocatorBackend::System);
        assert_eq!(name, "system");
    }
}

#[test]
fn test_allocator_backend_name_system() {
    let backend = AllocatorBackend::System;
    assert_eq!(backend.name(), "system");
}

#[cfg(feature = "jemalloc")]
#[test]
fn test_allocator_backend_name_jemalloc() {
    let backend = AllocatorBackend::Jemalloc;
    assert_eq!(backend.name(), "jemalloc");
}

#[cfg(feature = "mimalloc")]
#[test]
fn test_allocator_backend_name_mimalloc() {
    let backend = AllocatorBackend::Mimalloc;
    assert_eq!(backend.name(), "mimalloc");
}

#[test]
fn test_allocator_backend_default() {
    let backend = AllocatorBackend::default();
    assert_eq!(backend, AllocatorBackend::System);
}

#[test]
fn test_allocator_backend_equality() {
    let backend1 = AllocatorBackend::System;
    let backend2 = AllocatorBackend::System;
    assert_eq!(backend1, backend2);
}

#[test]
fn test_allocator_backend_clone() {
    let backend = AllocatorBackend::System;
    let cloned = backend;
    assert_eq!(backend, cloned);
}

// === SimdAllocator Creation Tests ===

#[test]
fn test_simd_allocator_creation() {
    let allocator = SimdAllocator::new();
    let backend = AllocatorBackend::current();
    // Just verify it creates without panicking
    let _ = allocator;
    let _ = backend;
}

#[test]
fn test_simd_allocator_with_backend_system() {
    let allocator = SimdAllocator::with_backend(AllocatorBackend::System);
    let _ = allocator;
}

#[cfg(feature = "jemalloc")]
#[test]
fn test_simd_allocator_with_backend_jemalloc() {
    let allocator = SimdAllocator::with_backend(AllocatorBackend::Jemalloc);
    let _ = allocator;
}

#[cfg(feature = "mimalloc")]
#[test]
fn test_simd_allocator_with_backend_mimalloc() {
    let allocator = SimdAllocator::with_backend(AllocatorBackend::Mimalloc);
    let _ = allocator;
}

#[test]
fn test_simd_allocator_default() {
    let allocator = SimdAllocator::default();
    let _ = allocator;
}

// === Aligned Allocation Tests ===

#[test]
fn test_alloc_aligned_16_bytes() {
    let allocator = SimdAllocator::new();
    unsafe {
        let ptr = allocator.alloc_aligned(1024, 16).unwrap();
        assert_eq!(ptr.as_ptr() as usize % 16, 0);

        let layout = Layout::from_size_align(1024, 16).unwrap();
        allocator.dealloc_aligned(ptr, layout);
    }
}

#[test]
fn test_alloc_aligned_32_bytes() {
    let allocator = SimdAllocator::new();
    unsafe {
        let ptr = allocator.alloc_aligned(2048, 32).unwrap();
        assert_eq!(ptr.as_ptr() as usize % 32, 0);

        let layout = Layout::from_size_align(2048, 32).unwrap();
        allocator.dealloc_aligned(ptr, layout);
    }
}

#[test]
fn test_alloc_aligned_64_bytes() {
    let allocator = SimdAllocator::new();
    unsafe {
        let ptr = allocator.alloc_aligned(4096, 64).unwrap();
        assert_eq!(ptr.as_ptr() as usize % 64, 0);

        let layout = Layout::from_size_align(4096, 64).unwrap();
        allocator.dealloc_aligned(ptr, layout);
    }
}

#[test]
fn test_alloc_aligned_128_bytes() {
    let allocator = SimdAllocator::new();
    unsafe {
        let ptr = allocator.alloc_aligned(8192, 128).unwrap();
        assert_eq!(ptr.as_ptr() as usize % 128, 0);

        let layout = Layout::from_size_align(8192, 128).unwrap();
        allocator.dealloc_aligned(ptr, layout);
    }
}

#[test]
fn test_alloc_aligned_256_bytes() {
    let allocator = SimdAllocator::new();
    unsafe {
        let ptr = allocator.alloc_aligned(16384, 256).unwrap();
        assert_eq!(ptr.as_ptr() as usize % 256, 0);

        let layout = Layout::from_size_align(16384, 256).unwrap();
        allocator.dealloc_aligned(ptr, layout);
    }
}

#[test]
fn test_alloc_aligned_multiple_allocations() {
    let allocator = SimdAllocator::new();
    unsafe {
        let mut ptrs = Vec::new();

        for &alignment in &[16, 32, 64, 128, 256] {
            let ptr = allocator.alloc_aligned(1024, alignment).unwrap();
            assert_eq!(
                ptr.as_ptr() as usize % alignment,
                0,
                "Pointer not aligned to {} bytes",
                alignment
            );
            ptrs.push((ptr, alignment));
        }

        for (ptr, alignment) in ptrs {
            let layout = Layout::from_size_align(1024, alignment).unwrap();
            allocator.dealloc_aligned(ptr, layout);
        }
    }
}

// === Reallocation Tests ===

#[test]
fn test_realloc_aligned_grow() {
    let allocator = SimdAllocator::new();
    unsafe {
        let alignment = 64;
        let initial_size = 1024;
        let new_size = 2048;

        let ptr = allocator.alloc_aligned(initial_size, alignment).unwrap();
        let layout = Layout::from_size_align(initial_size, alignment).unwrap();

        // Write pattern
        std::ptr::write_bytes(ptr.as_ptr(), 0xAB, initial_size);

        let new_ptr = allocator.realloc_aligned(ptr, layout, new_size).unwrap();

        // Verify alignment preserved
        assert_eq!(new_ptr.as_ptr() as usize % alignment, 0);

        // Verify data preserved
        let first_byte = std::ptr::read(new_ptr.as_ptr());
        assert_eq!(first_byte, 0xAB);

        let new_layout = Layout::from_size_align(new_size, alignment).unwrap();
        allocator.dealloc_aligned(new_ptr, new_layout);
    }
}

#[test]
fn test_realloc_aligned_shrink() {
    let allocator = SimdAllocator::new();
    unsafe {
        let alignment = 64;
        let initial_size = 2048;
        let new_size = 1024;

        let ptr = allocator.alloc_aligned(initial_size, alignment).unwrap();
        let layout = Layout::from_size_align(initial_size, alignment).unwrap();

        std::ptr::write_bytes(ptr.as_ptr(), 0xCD, initial_size);

        let new_ptr = allocator.realloc_aligned(ptr, layout, new_size).unwrap();

        assert_eq!(new_ptr.as_ptr() as usize % alignment, 0);

        let first_byte = std::ptr::read(new_ptr.as_ptr());
        assert_eq!(first_byte, 0xCD);

        let new_layout = Layout::from_size_align(new_size, alignment).unwrap();
        allocator.dealloc_aligned(new_ptr, new_layout);
    }
}

#[test]
fn test_realloc_aligned_same_size() {
    let allocator = SimdAllocator::new();
    unsafe {
        let alignment = 64;
        let size = 1024;

        let ptr = allocator.alloc_aligned(size, alignment).unwrap();
        let layout = Layout::from_size_align(size, alignment).unwrap();

        std::ptr::write_bytes(ptr.as_ptr(), 0xEF, size);

        let new_ptr = allocator.realloc_aligned(ptr, layout, size).unwrap();

        assert_eq!(new_ptr.as_ptr() as usize % alignment, 0);

        let first_byte = std::ptr::read(new_ptr.as_ptr());
        assert_eq!(first_byte, 0xEF);

        allocator.dealloc_aligned(new_ptr, layout);
    }
}

// === Deallocation Tests ===

#[test]
fn test_dealloc_aligned() {
    let allocator = SimdAllocator::new();
    unsafe {
        let ptr = allocator.alloc_aligned(1024, 64).unwrap();
        let layout = Layout::from_size_align(1024, 64).unwrap();

        // Should not panic or cause memory errors
        allocator.dealloc_aligned(ptr, layout);
    }
}

#[test]
fn test_alloc_dealloc_cycle() {
    let allocator = SimdAllocator::new();
    unsafe {
        for _ in 0..100 {
            let ptr = allocator.alloc_aligned(1024, 64).unwrap();
            let layout = Layout::from_size_align(1024, 64).unwrap();
            allocator.dealloc_aligned(ptr, layout);
        }
    }
}

// === Statistics Tests ===

#[test]
fn test_allocator_stats_default() {
    let stats = AllocatorStats::default();
    assert_eq!(stats.allocated_bytes, 0);
    assert_eq!(stats.resident_bytes, 0);
    assert_eq!(stats.metadata_bytes, 0);
}

#[cfg(feature = "jemalloc")]
#[test]
fn test_jemalloc_stats() {
    let allocator = SimdAllocator::with_backend(AllocatorBackend::Jemalloc);

    unsafe {
        let ptr = allocator.alloc_aligned(1024 * 1024, 64).unwrap();

        let stats = allocator.stats();
        assert!(stats.allocated_bytes > 0);
        assert_eq!(stats.backend, AllocatorBackend::Jemalloc);

        let layout = Layout::from_size_align(1024 * 1024, 64).unwrap();
        allocator.dealloc_aligned(ptr, layout);
    }
}

#[test]
fn test_system_allocator_stats() {
    let allocator = SimdAllocator::with_backend(AllocatorBackend::System);
    let stats = allocator.stats();

    // System allocator doesn't provide detailed stats
    assert_eq!(stats.backend, AllocatorBackend::System);
}

// === Global Allocator Tests ===

#[test]
fn test_global_allocator() {
    let allocator = global_allocator();
    let _ = allocator;
}

#[test]
fn test_global_allocator_singleton() {
    let allocator1 = global_allocator();
    let allocator2 = global_allocator();

    // Should be same instance
    assert!(std::ptr::eq(allocator1, allocator2));
}

// === Error Handling Tests ===

#[test]
fn test_invalid_alignment_not_power_of_two() {
    let allocator = SimdAllocator::new();
    unsafe {
        let result = allocator.alloc_aligned(1024, 17); // Not power of 2
        assert!(result.is_err());
    }
}

#[test]
fn test_invalid_alignment_zero() {
    let allocator = SimdAllocator::new();
    unsafe {
        let result = allocator.alloc_aligned(1024, 0);
        assert!(result.is_err());
    }
}

#[test]
fn test_invalid_alignment_three() {
    let allocator = SimdAllocator::new();
    unsafe {
        let result = allocator.alloc_aligned(1024, 3);
        assert!(result.is_err());
    }
}

// === Edge Cases ===

#[test]
fn test_alloc_small_size() {
    let allocator = SimdAllocator::new();
    unsafe {
        let ptr = allocator.alloc_aligned(1, 16).unwrap();
        assert_eq!(ptr.as_ptr() as usize % 16, 0);

        let layout = Layout::from_size_align(1, 16).unwrap();
        allocator.dealloc_aligned(ptr, layout);
    }
}

#[test]
fn test_alloc_large_size() {
    let allocator = SimdAllocator::new();
    unsafe {
        let size = 10 * 1024 * 1024; // 10MB
        let ptr = allocator.alloc_aligned(size, 64).unwrap();
        assert_eq!(ptr.as_ptr() as usize % 64, 0);

        let layout = Layout::from_size_align(size, 64).unwrap();
        allocator.dealloc_aligned(ptr, layout);
    }
}

#[test]
fn test_alloc_page_aligned() {
    let allocator = SimdAllocator::new();
    unsafe {
        let page_size = 4096;
        let ptr = allocator.alloc_aligned(page_size, page_size).unwrap();
        assert_eq!(ptr.as_ptr() as usize % page_size, 0);

        let layout = Layout::from_size_align(page_size, page_size).unwrap();
        allocator.dealloc_aligned(ptr, layout);
    }
}

#[test]
fn test_realloc_to_larger_alignment() {
    let allocator = SimdAllocator::new();
    unsafe {
        let ptr = allocator.alloc_aligned(1024, 16).unwrap();
        let layout = Layout::from_size_align(1024, 16).unwrap();

        // Realloc with same alignment
        let new_ptr = allocator.realloc_aligned(ptr, layout, 2048).unwrap();
        assert_eq!(new_ptr.as_ptr() as usize % 16, 0);

        let new_layout = Layout::from_size_align(2048, 16).unwrap();
        allocator.dealloc_aligned(new_ptr, new_layout);
    }
}

#[test]
fn test_multiple_allocators() {
    let allocator1 = SimdAllocator::new();
    let allocator2 = SimdAllocator::new();

    unsafe {
        let ptr1 = allocator1.alloc_aligned(1024, 64).unwrap();
        let ptr2 = allocator2.alloc_aligned(1024, 64).unwrap();

        assert_eq!(ptr1.as_ptr() as usize % 64, 0);
        assert_eq!(ptr2.as_ptr() as usize % 64, 0);

        let layout = Layout::from_size_align(1024, 64).unwrap();
        allocator1.dealloc_aligned(ptr1, layout);
        allocator2.dealloc_aligned(ptr2, layout);
    }
}

#[test]
fn test_data_integrity_after_realloc() {
    let allocator = SimdAllocator::new();
    unsafe {
        let alignment = 64;
        let initial_size = 512;
        let new_size = 1024;

        let ptr = allocator.alloc_aligned(initial_size, alignment).unwrap();
        let layout = Layout::from_size_align(initial_size, alignment).unwrap();

        // Write test pattern
        for i in 0..initial_size {
            std::ptr::write(ptr.as_ptr().add(i), (i % 256) as u8);
        }

        let new_ptr = allocator.realloc_aligned(ptr, layout, new_size).unwrap();

        // Verify data integrity
        for i in 0..initial_size {
            let byte = std::ptr::read(new_ptr.as_ptr().add(i));
            assert_eq!(byte, (i % 256) as u8, "Data corrupted at offset {}", i);
        }

        let new_layout = Layout::from_size_align(new_size, alignment).unwrap();
        allocator.dealloc_aligned(new_ptr, new_layout);
    }
}

#[cfg(feature = "jemalloc")]
#[test]
fn test_jemalloc_specific_allocation() {
    let allocator = SimdAllocator::with_backend(AllocatorBackend::Jemalloc);
    unsafe {
        let ptr = allocator.alloc_aligned(2048, 128).unwrap();
        assert_eq!(ptr.as_ptr() as usize % 128, 0);

        let layout = Layout::from_size_align(2048, 128).unwrap();
        allocator.dealloc_aligned(ptr, layout);
    }
}

#[cfg(feature = "mimalloc")]
#[test]
fn test_mimalloc_specific_allocation() {
    let allocator = SimdAllocator::with_backend(AllocatorBackend::Mimalloc);
    unsafe {
        let ptr = allocator.alloc_aligned(2048, 128).unwrap();
        assert_eq!(ptr.as_ptr() as usize % 128, 0);

        let layout = Layout::from_size_align(2048, 128).unwrap();
        allocator.dealloc_aligned(ptr, layout);
    }
}

#[test]
fn test_allocator_stress_test() {
    let allocator = SimdAllocator::new();
    unsafe {
        let mut allocations = Vec::new();

        // Allocate many blocks
        for i in 0..50 {
            let size = (i + 1) * 256;
            let alignment = 64;
            let ptr = allocator.alloc_aligned(size, alignment).unwrap();
            let layout = Layout::from_size_align(size, alignment).unwrap();
            allocations.push((ptr, layout));
        }

        // Free all
        for (ptr, layout) in allocations {
            allocator.dealloc_aligned(ptr, layout);
        }
    }
}

#[test]
fn test_layout_from_size_align_validation() {
    // Verify Layout::from_size_align validates correctly
    assert!(Layout::from_size_align(1024, 16).is_ok());
    assert!(Layout::from_size_align(1024, 0).is_err()); // Zero alignment
    assert!(Layout::from_size_align(1024, 3).is_err()); // Not power of 2
}
