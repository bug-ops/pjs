//! Comprehensive tests for the aligned allocator module.
//!
//! Covers:
//! - Memory allocation with different alignments
//! - Reallocation (grow, shrink, same size)
//! - Deallocation safety
//! - Edge cases (zero size, max alignment, boundary values)
//! - Error handling for invalid alignments
//! - Data integrity across realloc
//! - Concurrent use via `Arc`
//! - `global_allocator_name` diagnostic function

use pjson_rs::global_allocator_name;
use pjson_rs::parser::aligned_alloc::{AlignedAllocator, aligned_allocator};
use std::alloc::Layout;

// === AlignedAllocator Creation ===

#[test]
fn test_aligned_allocator_creation() {
    let _allocator = AlignedAllocator::new();
}

#[test]
fn test_aligned_allocator_default() {
    // AlignedAllocator is a unit struct — Default yields the same value as new()
    let _allocator = AlignedAllocator;
}

// === Aligned Allocation ===

#[test]
fn test_alloc_aligned_16_bytes() {
    let allocator = AlignedAllocator::new();
    unsafe {
        let ptr = allocator.alloc_aligned(1024, 16).unwrap();
        assert_eq!(ptr.as_ptr() as usize % 16, 0);
        let layout = Layout::from_size_align(1024, 16).unwrap();
        allocator.dealloc_aligned(ptr, layout);
    }
}

#[test]
fn test_alloc_aligned_32_bytes() {
    let allocator = AlignedAllocator::new();
    unsafe {
        let ptr = allocator.alloc_aligned(2048, 32).unwrap();
        assert_eq!(ptr.as_ptr() as usize % 32, 0);
        let layout = Layout::from_size_align(2048, 32).unwrap();
        allocator.dealloc_aligned(ptr, layout);
    }
}

#[test]
fn test_alloc_aligned_64_bytes() {
    let allocator = AlignedAllocator::new();
    unsafe {
        let ptr = allocator.alloc_aligned(4096, 64).unwrap();
        assert_eq!(ptr.as_ptr() as usize % 64, 0);
        let layout = Layout::from_size_align(4096, 64).unwrap();
        allocator.dealloc_aligned(ptr, layout);
    }
}

#[test]
fn test_alloc_aligned_128_bytes() {
    let allocator = AlignedAllocator::new();
    unsafe {
        let ptr = allocator.alloc_aligned(8192, 128).unwrap();
        assert_eq!(ptr.as_ptr() as usize % 128, 0);
        let layout = Layout::from_size_align(8192, 128).unwrap();
        allocator.dealloc_aligned(ptr, layout);
    }
}

#[test]
fn test_alloc_aligned_256_bytes() {
    let allocator = AlignedAllocator::new();
    unsafe {
        let ptr = allocator.alloc_aligned(16384, 256).unwrap();
        assert_eq!(ptr.as_ptr() as usize % 256, 0);
        let layout = Layout::from_size_align(16384, 256).unwrap();
        allocator.dealloc_aligned(ptr, layout);
    }
}

#[test]
fn test_alloc_aligned_multiple_allocations() {
    let allocator = AlignedAllocator::new();
    unsafe {
        let mut ptrs = Vec::new();
        for &alignment in &[16usize, 32, 64, 128, 256] {
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

// === Reallocation ===

#[test]
fn test_realloc_aligned_grow() {
    let allocator = AlignedAllocator::new();
    unsafe {
        let alignment = 64;
        let initial_size = 1024;
        let new_size = 2048;

        let ptr = allocator.alloc_aligned(initial_size, alignment).unwrap();
        let layout = Layout::from_size_align(initial_size, alignment).unwrap();

        std::ptr::write_bytes(ptr.as_ptr(), 0xAB, initial_size);

        let new_ptr = allocator.realloc_aligned(ptr, layout, new_size).unwrap();

        assert_eq!(new_ptr.as_ptr() as usize % alignment, 0);
        let first_byte = std::ptr::read(new_ptr.as_ptr());
        assert_eq!(first_byte, 0xAB);

        let new_layout = Layout::from_size_align(new_size, alignment).unwrap();
        allocator.dealloc_aligned(new_ptr, new_layout);
    }
}

#[test]
fn test_realloc_aligned_shrink() {
    let allocator = AlignedAllocator::new();
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
    let allocator = AlignedAllocator::new();
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

// === Deallocation ===

#[test]
fn test_dealloc_aligned() {
    let allocator = AlignedAllocator::new();
    unsafe {
        let ptr = allocator.alloc_aligned(1024, 64).unwrap();
        let layout = Layout::from_size_align(1024, 64).unwrap();
        allocator.dealloc_aligned(ptr, layout);
    }
}

#[test]
fn test_alloc_dealloc_cycle() {
    let allocator = AlignedAllocator::new();
    unsafe {
        for _ in 0..100 {
            let ptr = allocator.alloc_aligned(1024, 64).unwrap();
            let layout = Layout::from_size_align(1024, 64).unwrap();
            allocator.dealloc_aligned(ptr, layout);
        }
    }
}

// === Global Allocator Accessor ===

#[test]
fn test_aligned_allocator_accessor() {
    let allocator = aligned_allocator();
    let _ = allocator;
}

#[test]
fn test_aligned_allocator_singleton() {
    let a = aligned_allocator();
    let b = aligned_allocator();
    assert!(std::ptr::eq(a, b));
}

// === Global Allocator Name ===

#[test]
fn test_global_allocator_name() {
    let name = global_allocator_name();
    assert!(
        name == "mimalloc" || name == "system",
        "Unexpected allocator name: {name}"
    );

    #[cfg(all(feature = "mimalloc", not(target_arch = "wasm32")))]
    assert_eq!(name, "mimalloc");

    #[cfg(not(all(feature = "mimalloc", not(target_arch = "wasm32"))))]
    assert_eq!(name, "system");
}

// === Error Handling ===

#[test]
fn test_invalid_alignment_not_power_of_two() {
    let allocator = AlignedAllocator::new();
    unsafe {
        let result = allocator.alloc_aligned(1024, 17);
        assert!(result.is_err());
    }
}

#[test]
fn test_invalid_alignment_zero() {
    let allocator = AlignedAllocator::new();
    unsafe {
        let result = allocator.alloc_aligned(1024, 0);
        assert!(result.is_err());
    }
}

#[test]
fn test_invalid_alignment_three() {
    let allocator = AlignedAllocator::new();
    unsafe {
        let result = allocator.alloc_aligned(1024, 3);
        assert!(result.is_err());
    }
}

#[test]
fn test_alloc_invalid_alignment_large_not_power_of_two() {
    let allocator = AlignedAllocator::new();
    unsafe {
        let result = allocator.alloc_aligned(1024, 100);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("power of 2"));
    }
}

// === Edge Cases ===

#[test]
fn test_alloc_small_size() {
    let allocator = AlignedAllocator::new();
    unsafe {
        let ptr = allocator.alloc_aligned(1, 16).unwrap();
        assert_eq!(ptr.as_ptr() as usize % 16, 0);
        let layout = Layout::from_size_align(1, 16).unwrap();
        allocator.dealloc_aligned(ptr, layout);
    }
}

#[test]
fn test_alloc_large_size() {
    let allocator = AlignedAllocator::new();
    unsafe {
        let size = 10 * 1024 * 1024; // 10 MB
        let ptr = allocator.alloc_aligned(size, 64).unwrap();
        assert_eq!(ptr.as_ptr() as usize % 64, 0);
        let layout = Layout::from_size_align(size, 64).unwrap();
        allocator.dealloc_aligned(ptr, layout);
    }
}

#[test]
fn test_alloc_page_aligned() {
    let allocator = AlignedAllocator::new();
    unsafe {
        let page_size = 4096;
        let ptr = allocator.alloc_aligned(page_size, page_size).unwrap();
        assert_eq!(ptr.as_ptr() as usize % page_size, 0);
        let layout = Layout::from_size_align(page_size, page_size).unwrap();
        allocator.dealloc_aligned(ptr, layout);
    }
}

#[test]
fn test_alignment_boundary_values() {
    let allocator = AlignedAllocator::new();
    unsafe {
        let ptr1 = allocator.alloc_aligned(100, 1).unwrap();
        let layout1 = Layout::from_size_align(100, 1).unwrap();
        allocator.dealloc_aligned(ptr1, layout1);

        for &align in &[2usize, 4, 8, 16, 32, 64, 128, 256, 512, 1024] {
            let ptr = allocator.alloc_aligned(1024, align).unwrap();
            assert_eq!(ptr.as_ptr() as usize % align, 0);
            let layout = Layout::from_size_align(1024, align).unwrap();
            allocator.dealloc_aligned(ptr, layout);
        }
    }
}

#[test]
fn test_alloc_dealloc_different_sizes() {
    let allocator = AlignedAllocator::new();
    unsafe {
        for size in [1, 8, 64, 256, 1024, 4096, 16384] {
            let ptr = allocator.alloc_aligned(size, 64).unwrap();
            assert_eq!(ptr.as_ptr() as usize % 64, 0);
            let layout = Layout::from_size_align(size, 64).unwrap();
            allocator.dealloc_aligned(ptr, layout);
        }
    }
}

// === Data Integrity ===

#[test]
fn test_data_integrity_after_realloc() {
    let allocator = AlignedAllocator::new();
    unsafe {
        let alignment = 64;
        let initial_size = 512;
        let new_size = 1024;

        let ptr = allocator.alloc_aligned(initial_size, alignment).unwrap();
        let layout = Layout::from_size_align(initial_size, alignment).unwrap();

        for i in 0..initial_size {
            std::ptr::write(ptr.as_ptr().add(i), (i % 256) as u8);
        }

        let new_ptr = allocator.realloc_aligned(ptr, layout, new_size).unwrap();

        for i in 0..initial_size {
            let byte = std::ptr::read(new_ptr.as_ptr().add(i));
            assert_eq!(byte, (i % 256) as u8, "Data corrupted at offset {i}");
        }

        let new_layout = Layout::from_size_align(new_size, alignment).unwrap();
        allocator.dealloc_aligned(new_ptr, new_layout);
    }
}

#[test]
fn test_realloc_preserve_data_patterns() {
    let allocator = AlignedAllocator::new();
    unsafe {
        let alignment = 64;
        let initial_size = 256;
        let new_size = 512;

        let ptr = allocator.alloc_aligned(initial_size, alignment).unwrap();
        let layout = Layout::from_size_align(initial_size, alignment).unwrap();

        for i in 0..initial_size {
            std::ptr::write(ptr.as_ptr().add(i), ((i * 7) % 256) as u8);
        }

        let new_ptr = allocator.realloc_aligned(ptr, layout, new_size).unwrap();

        for i in 0..initial_size {
            let byte = std::ptr::read(new_ptr.as_ptr().add(i));
            assert_eq!(
                byte,
                ((i * 7) % 256) as u8,
                "Pattern corrupted at offset {i}"
            );
        }

        let new_layout = Layout::from_size_align(new_size, alignment).unwrap();
        allocator.dealloc_aligned(new_ptr, new_layout);
    }
}

// === Stress and Concurrency ===

#[test]
fn test_allocator_stress_test() {
    let allocator = AlignedAllocator::new();
    unsafe {
        let mut allocations = Vec::new();
        for i in 0..50 {
            let size = (i + 1) * 256;
            let alignment = 64;
            let ptr = allocator.alloc_aligned(size, alignment).unwrap();
            let layout = Layout::from_size_align(size, alignment).unwrap();
            allocations.push((ptr, layout));
        }
        for (ptr, layout) in allocations {
            allocator.dealloc_aligned(ptr, layout);
        }
    }
}

#[test]
fn test_multiple_allocators() {
    let allocator1 = AlignedAllocator::new();
    let allocator2 = AlignedAllocator::new();
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
fn test_concurrent_allocations_single_allocator() {
    use std::sync::Arc;
    use std::thread;

    let allocator = Arc::new(AlignedAllocator::new());
    let mut handles = vec![];

    for _ in 0..4 {
        let allocator_clone = Arc::clone(&allocator);
        let handle = thread::spawn(move || unsafe {
            let ptr = allocator_clone.alloc_aligned(1024, 64).unwrap();
            let layout = Layout::from_size_align(1024, 64).unwrap();

            std::ptr::write_bytes(ptr.as_ptr(), 0xFF, 1024);
            let first_byte = std::ptr::read(ptr.as_ptr());
            assert_eq!(first_byte, 0xFF);

            allocator_clone.dealloc_aligned(ptr, layout);
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }
}

// === Layout Validation ===

#[test]
fn test_layout_from_size_align_validation() {
    assert!(Layout::from_size_align(1024, 16).is_ok());
    assert!(Layout::from_size_align(1024, 0).is_err());
    assert!(Layout::from_size_align(1024, 3).is_err());
}

#[test]
fn test_realloc_invalid_new_size_with_layout() {
    let allocator = AlignedAllocator::new();
    unsafe {
        let ptr = allocator.alloc_aligned(1024, 64).unwrap();
        let layout = Layout::from_size_align(1024, 64).unwrap();

        // Try to realloc to extremely large size that might fail on some systems.
        // Should not panic — either succeeds or returns ResourceExhausted.
        let new_size = usize::MAX / 2;
        let result = allocator.realloc_aligned(ptr, layout, new_size);

        // Always free original in case realloc returned null without consuming ptr.
        // (on failure, realloc does not free the original)
        if let Ok(new_ptr) = result {
            let new_layout = Layout::from_size_align(new_size, 64).unwrap();
            allocator.dealloc_aligned(new_ptr, new_layout);
        } else {
            allocator.dealloc_aligned(ptr, layout);
        }
    }
}
