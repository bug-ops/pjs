//! Aligned memory allocation for SIMD buffer pools.
//!
//! Wraps `std::alloc::{alloc, dealloc, realloc}` with [`Layout`]-based
//! alignment, suitable for AVX2 (32-byte), AVX-512 (64-byte), and NEON
//! (16-byte) SIMD operations.
//!
//! All allocations route through the process-wide `#[global_allocator]`,
//! so when the `mimalloc` feature is enabled this transparently uses mimalloc.

use crate::domain::{DomainError, DomainResult};
use std::{
    alloc::{Layout, alloc, dealloc, realloc},
    ptr::NonNull,
};

/// Aligned memory allocator for SIMD operations.
///
/// Zero-sized: holds no state. All calls delegate to the global allocator
/// via [`std::alloc`], which routes through whatever `#[global_allocator]`
/// is registered (mimalloc when the `mimalloc` feature is enabled, otherwise
/// the system allocator).
#[derive(Debug, Default, Clone, Copy)]
pub struct AlignedAllocator;

impl AlignedAllocator {
    /// Construct a new allocator handle (zero cost).
    pub const fn new() -> Self {
        Self
    }

    /// Allocate `size` bytes aligned to `alignment` (must be a power of two).
    ///
    /// Returns a non-null pointer owned by the caller. The caller must
    /// deallocate it via [`AlignedAllocator::dealloc_aligned`] with the same
    /// `Layout`.
    ///
    /// # Safety
    ///
    /// The returned pointer is valid for `size` bytes. The caller is
    /// responsible for ensuring the pointer is not used after deallocation.
    pub unsafe fn alloc_aligned(&self, size: usize, alignment: usize) -> DomainResult<NonNull<u8>> {
        if !alignment.is_power_of_two() {
            return Err(DomainError::InvalidInput(format!(
                "Alignment {} is not a power of 2",
                alignment
            )));
        }

        let layout = Layout::from_size_align(size, alignment)
            .map_err(|e| DomainError::InvalidInput(format!("Invalid layout: {}", e)))?;

        // SAFETY: layout is valid (size and alignment validated above).
        let ptr = unsafe { alloc(layout) };
        if ptr.is_null() {
            return Err(DomainError::ResourceExhausted(format!(
                "Failed to allocate {} bytes with alignment {}",
                size, alignment
            )));
        }

        // SAFETY: alloc returned non-null.
        Ok(unsafe { NonNull::new_unchecked(ptr) })
    }

    /// Reallocate to `new_size`, preserving the original alignment.
    ///
    /// # Safety
    ///
    /// `ptr` must have been returned by [`AlignedAllocator::alloc_aligned`]
    /// with `old_layout`. After this call, `ptr` is no longer valid —
    /// use the returned pointer instead.
    pub unsafe fn realloc_aligned(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        new_size: usize,
    ) -> DomainResult<NonNull<u8>> {
        // SAFETY: caller upholds layout match.
        let new_ptr = unsafe { realloc(ptr.as_ptr(), old_layout, new_size) };
        if new_ptr.is_null() {
            return Err(DomainError::ResourceExhausted(format!(
                "Failed to reallocate to {} bytes",
                new_size
            )));
        }

        // SAFETY: realloc returned non-null.
        Ok(unsafe { NonNull::new_unchecked(new_ptr) })
    }

    /// Deallocate memory previously returned by [`AlignedAllocator::alloc_aligned`].
    ///
    /// # Safety
    ///
    /// `ptr` must have been returned by [`AlignedAllocator::alloc_aligned`]
    /// with exactly `layout`. Double-free or mismatched layout is undefined
    /// behavior.
    pub unsafe fn dealloc_aligned(&self, ptr: NonNull<u8>, layout: Layout) {
        // SAFETY: caller upholds layout match.
        unsafe { dealloc(ptr.as_ptr(), layout) };
    }
}

static ALIGNED_ALLOCATOR: AlignedAllocator = AlignedAllocator::new();

/// Returns the process-wide aligned allocator handle.
///
/// The returned reference is a zero-cost singleton — [`AlignedAllocator`] is
/// zero-sized, so callers may also construct one inline with
/// `AlignedAllocator::new()` at no extra cost.
pub fn aligned_allocator() -> &'static AlignedAllocator {
    &ALIGNED_ALLOCATOR
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aligned_allocation() {
        let allocator = AlignedAllocator::new();

        unsafe {
            for alignment in [16, 32, 64, 128, 256] {
                let ptr = allocator.alloc_aligned(1024, alignment).unwrap();

                assert_eq!(
                    ptr.as_ptr() as usize % alignment,
                    0,
                    "Pointer not aligned to {} bytes",
                    alignment
                );

                let layout = Layout::from_size_align(1024, alignment).unwrap();
                allocator.dealloc_aligned(ptr, layout);
            }
        }
    }

    #[test]
    fn test_reallocation() {
        let allocator = AlignedAllocator::new();

        unsafe {
            let alignment = 64;
            let initial_size = 1024;
            let new_size = 2048;

            let ptr = allocator.alloc_aligned(initial_size, alignment).unwrap();
            let layout = Layout::from_size_align(initial_size, alignment).unwrap();

            std::ptr::write_bytes(ptr.as_ptr(), 0xAB, initial_size);

            let new_ptr = allocator.realloc_aligned(ptr, layout, new_size).unwrap();

            assert_eq!(
                new_ptr.as_ptr() as usize % alignment,
                0,
                "Reallocated pointer not aligned"
            );

            let first_byte = std::ptr::read(new_ptr.as_ptr());
            assert_eq!(first_byte, 0xAB, "Data not preserved during reallocation");

            let new_layout = Layout::from_size_align(new_size, alignment).unwrap();
            allocator.dealloc_aligned(new_ptr, new_layout);
        }
    }

    #[test]
    fn test_invalid_alignment() {
        let allocator = AlignedAllocator::new();
        unsafe {
            assert!(allocator.alloc_aligned(1024, 0).is_err());
            assert!(allocator.alloc_aligned(1024, 3).is_err());
            assert!(allocator.alloc_aligned(1024, 17).is_err());
        }
    }

    #[test]
    fn test_aligned_allocator_singleton() {
        let a = aligned_allocator();
        let b = aligned_allocator();
        assert!(std::ptr::eq(a, b));
    }
}
