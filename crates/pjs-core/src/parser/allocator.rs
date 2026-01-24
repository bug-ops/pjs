//! Custom allocator support for high-performance buffer management
//!
//! This module provides abstraction over different memory allocators
//! (system, jemalloc, mimalloc) with SIMD-aware alignment support.

use crate::domain::{DomainError, DomainResult};
use std::{
    alloc::{Layout, alloc, dealloc, realloc},
    ptr::NonNull,
};

/// Memory allocator backend
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AllocatorBackend {
    /// System allocator (default)
    #[default]
    System,
    /// Jemalloc allocator (high-performance, good for heavy allocation workloads)
    #[cfg(feature = "jemalloc")]
    Jemalloc,
    /// Mimalloc allocator (Microsoft's allocator, good for multi-threaded workloads)
    #[cfg(feature = "mimalloc")]
    Mimalloc,
}

impl AllocatorBackend {
    /// Get the current active allocator
    pub fn current() -> Self {
        #[cfg(feature = "jemalloc")]
        {
            Self::Jemalloc
        }

        #[cfg(all(feature = "mimalloc", not(feature = "jemalloc")))]
        {
            Self::Mimalloc
        }

        #[cfg(all(not(feature = "jemalloc"), not(feature = "mimalloc")))]
        {
            Self::System
        }
    }

    /// Get allocator name for debugging
    pub fn name(&self) -> &'static str {
        match self {
            Self::System => "system",
            #[cfg(feature = "jemalloc")]
            Self::Jemalloc => "jemalloc",
            #[cfg(feature = "mimalloc")]
            Self::Mimalloc => "mimalloc",
        }
    }
}

/// SIMD-aware memory allocator with support for different backends
pub struct SimdAllocator {
    #[allow(dead_code)] // Future: backend-specific allocation strategies
    backend: AllocatorBackend,
}

impl SimdAllocator {
    /// Create a new allocator with the current backend
    pub fn new() -> Self {
        Self {
            backend: AllocatorBackend::current(),
        }
    }

    /// Create allocator with specific backend
    pub fn with_backend(backend: AllocatorBackend) -> Self {
        Self { backend }
    }

    /// Allocate aligned memory for SIMD operations
    ///
    /// # Safety
    /// Returns raw pointer that must be properly deallocated
    pub unsafe fn alloc_aligned(&self, size: usize, alignment: usize) -> DomainResult<NonNull<u8>> {
        // Validate alignment
        if !alignment.is_power_of_two() {
            return Err(DomainError::InvalidInput(format!(
                "Alignment {} is not a power of 2",
                alignment
            )));
        }

        let layout = Layout::from_size_align(size, alignment)
            .map_err(|e| DomainError::InvalidInput(format!("Invalid layout: {}", e)))?;

        // Use jemalloc-specific aligned allocation if available
        #[cfg(feature = "jemalloc")]
        if matches!(self.backend, AllocatorBackend::Jemalloc) {
            return unsafe { self.jemalloc_alloc_aligned(size, alignment) };
        }

        // Use mimalloc-specific aligned allocation if available
        #[cfg(feature = "mimalloc")]
        if matches!(self.backend, AllocatorBackend::Mimalloc) {
            return unsafe { self.mimalloc_alloc_aligned(size, alignment) };
        }

        // Fall back to system allocator
        // Safety: layout is valid as checked above
        unsafe {
            let ptr = alloc(layout);
            if ptr.is_null() {
                return Err(DomainError::ResourceExhausted(format!(
                    "Failed to allocate {} bytes with alignment {}",
                    size, alignment
                )));
            }

            Ok(NonNull::new_unchecked(ptr))
        }
    }

    /// Reallocate aligned memory
    ///
    /// # Safety
    /// Caller must ensure ptr was allocated with same alignment
    pub unsafe fn realloc_aligned(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        new_size: usize,
    ) -> DomainResult<NonNull<u8>> {
        let _new_layout = Layout::from_size_align(new_size, old_layout.align())
            .map_err(|e| DomainError::InvalidInput(format!("Invalid layout: {}", e)))?;

        #[cfg(feature = "jemalloc")]
        if matches!(self.backend, AllocatorBackend::Jemalloc) {
            return unsafe { self.jemalloc_realloc_aligned(ptr, old_layout, new_size) };
        }

        #[cfg(feature = "mimalloc")]
        if matches!(self.backend, AllocatorBackend::Mimalloc) {
            return unsafe { self.mimalloc_realloc_aligned(ptr, old_layout, new_size) };
        }

        // System allocator realloc
        unsafe {
            let new_ptr = realloc(ptr.as_ptr(), old_layout, new_size);
            if new_ptr.is_null() {
                return Err(DomainError::ResourceExhausted(format!(
                    "Failed to reallocate to {} bytes",
                    new_size
                )));
            }

            Ok(NonNull::new_unchecked(new_ptr))
        }
    }

    /// Deallocate aligned memory
    ///
    /// # Safety
    /// Caller must ensure ptr was allocated with same layout
    pub unsafe fn dealloc_aligned(&self, ptr: NonNull<u8>, layout: Layout) {
        #[cfg(feature = "jemalloc")]
        if matches!(self.backend, AllocatorBackend::Jemalloc) {
            unsafe { self.jemalloc_dealloc_aligned(ptr, layout) };
            return;
        }

        #[cfg(feature = "mimalloc")]
        if matches!(self.backend, AllocatorBackend::Mimalloc) {
            unsafe { self.mimalloc_dealloc_aligned(ptr, layout) };
            return;
        }

        unsafe {
            dealloc(ptr.as_ptr(), layout);
        }
    }

    // Jemalloc-specific implementations
    #[cfg(feature = "jemalloc")]
    unsafe fn jemalloc_alloc_aligned(
        &self,
        size: usize,
        alignment: usize,
    ) -> DomainResult<NonNull<u8>> {
        use tikv_jemalloc_sys as jemalloc;

        // MALLOCX_ALIGN macro equivalent
        let align_flag = alignment.trailing_zeros() as i32;
        let ptr = unsafe { jemalloc::mallocx(size, align_flag) };
        if ptr.is_null() {
            return Err(DomainError::ResourceExhausted(format!(
                "Jemalloc failed to allocate {} bytes with alignment {}",
                size, alignment
            )));
        }

        Ok(unsafe { NonNull::new_unchecked(ptr as *mut u8) })
    }

    #[cfg(feature = "jemalloc")]
    unsafe fn jemalloc_realloc_aligned(
        &self,
        ptr: NonNull<u8>,
        _old_layout: Layout,
        new_size: usize,
    ) -> DomainResult<NonNull<u8>> {
        use tikv_jemalloc_sys as jemalloc;

        let alignment = _old_layout.align();
        let align_flag = alignment.trailing_zeros() as i32;
        let new_ptr = unsafe { jemalloc::rallocx(ptr.as_ptr() as *mut _, new_size, align_flag) };

        if new_ptr.is_null() {
            return Err(DomainError::ResourceExhausted(format!(
                "Jemalloc failed to reallocate to {} bytes",
                new_size
            )));
        }

        Ok(unsafe { NonNull::new_unchecked(new_ptr as *mut u8) })
    }

    #[cfg(feature = "jemalloc")]
    unsafe fn jemalloc_dealloc_aligned(&self, ptr: NonNull<u8>, layout: Layout) {
        use tikv_jemalloc_sys as jemalloc;

        let align_flag = layout.align().trailing_zeros() as i32;
        unsafe { jemalloc::dallocx(ptr.as_ptr() as *mut _, align_flag) };
    }

    // Mimalloc-specific implementations
    #[cfg(feature = "mimalloc")]
    unsafe fn mimalloc_alloc_aligned(
        &self,
        size: usize,
        alignment: usize,
    ) -> DomainResult<NonNull<u8>> {
        use libmimalloc_sys as mi;

        let ptr = unsafe { mi::mi_malloc_aligned(size, alignment) };
        if ptr.is_null() {
            return Err(DomainError::ResourceExhausted(format!(
                "Mimalloc failed to allocate {} bytes with alignment {}",
                size, alignment
            )));
        }

        Ok(unsafe { NonNull::new_unchecked(ptr as *mut u8) })
    }

    #[cfg(feature = "mimalloc")]
    unsafe fn mimalloc_realloc_aligned(
        &self,
        ptr: NonNull<u8>,
        _old_layout: Layout,
        new_size: usize,
    ) -> DomainResult<NonNull<u8>> {
        use libmimalloc_sys as mi;

        let alignment = _old_layout.align();
        let new_ptr =
            unsafe { mi::mi_realloc_aligned(ptr.as_ptr() as *mut _, new_size, alignment) };

        if new_ptr.is_null() {
            return Err(DomainError::ResourceExhausted(format!(
                "Mimalloc failed to reallocate to {} bytes",
                new_size
            )));
        }

        Ok(unsafe { NonNull::new_unchecked(new_ptr as *mut u8) })
    }

    #[cfg(feature = "mimalloc")]
    unsafe fn mimalloc_dealloc_aligned(&self, ptr: NonNull<u8>, _layout: Layout) {
        use libmimalloc_sys as mi;

        unsafe { mi::mi_free(ptr.as_ptr() as *mut _) };
    }

    /// Get allocator statistics (if available)
    pub fn stats(&self) -> AllocatorStats {
        #[cfg(feature = "jemalloc")]
        if matches!(self.backend, AllocatorBackend::Jemalloc) {
            return self.jemalloc_stats();
        }

        // Default stats for other allocators
        AllocatorStats::default()
    }

    #[cfg(feature = "jemalloc")]
    fn jemalloc_stats(&self) -> AllocatorStats {
        use tikv_jemalloc_ctl::{epoch, stats};

        // Update the statistics cache
        if let Err(e) = epoch::mib().map(|mib| mib.advance()) {
            eprintln!("Failed to advance jemalloc epoch: {}", e);
            return AllocatorStats::default();
        }

        // Query statistics
        let allocated = stats::allocated::read().unwrap_or(0);
        let resident = stats::resident::read().unwrap_or(0);
        let metadata = stats::metadata::read().unwrap_or(0);

        AllocatorStats {
            allocated_bytes: allocated,
            resident_bytes: resident,
            metadata_bytes: metadata,
            backend: self.backend,
        }
    }
}

impl Default for SimdAllocator {
    fn default() -> Self {
        Self::new()
    }
}

/// Allocator statistics
#[derive(Debug, Clone, Default)]
pub struct AllocatorStats {
    /// Total allocated bytes
    pub allocated_bytes: usize,
    /// Resident memory in bytes
    pub resident_bytes: usize,
    /// Metadata overhead in bytes
    pub metadata_bytes: usize,
    /// Backend being used
    pub backend: AllocatorBackend,
}

/// Global allocator instance
static GLOBAL_ALLOCATOR: std::sync::OnceLock<SimdAllocator> = std::sync::OnceLock::new();

/// Get global SIMD allocator
pub fn global_allocator() -> &'static SimdAllocator {
    GLOBAL_ALLOCATOR.get_or_init(SimdAllocator::new)
}

/// Initialize global allocator with specific backend
pub fn initialize_global_allocator(backend: AllocatorBackend) -> DomainResult<()> {
    GLOBAL_ALLOCATOR
        .set(SimdAllocator::with_backend(backend))
        .map_err(|_| {
            DomainError::InternalError("Global allocator already initialized".to_string())
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allocator_backend_detection() {
        let backend = AllocatorBackend::current();
        println!("Current allocator backend: {}", backend.name());

        #[cfg(feature = "jemalloc")]
        assert_eq!(backend, AllocatorBackend::Jemalloc);

        #[cfg(all(feature = "mimalloc", not(feature = "jemalloc")))]
        assert_eq!(backend, AllocatorBackend::Mimalloc);

        #[cfg(all(not(feature = "jemalloc"), not(feature = "mimalloc")))]
        assert_eq!(backend, AllocatorBackend::System);
    }

    #[test]
    fn test_aligned_allocation() {
        let allocator = SimdAllocator::new();

        unsafe {
            // Test various alignments
            for alignment in [16, 32, 64, 128, 256].iter() {
                let ptr = allocator.alloc_aligned(1024, *alignment).unwrap();

                // Verify alignment
                assert_eq!(
                    ptr.as_ptr() as usize % alignment,
                    0,
                    "Pointer not aligned to {} bytes",
                    alignment
                );

                // Clean up
                let layout = Layout::from_size_align(1024, *alignment).unwrap();
                allocator.dealloc_aligned(ptr, layout);
            }
        }
    }

    #[test]
    fn test_reallocation() {
        let allocator = SimdAllocator::new();

        unsafe {
            let alignment = 64;
            let initial_size = 1024;
            let new_size = 2048;

            // Initial allocation
            let ptr = allocator.alloc_aligned(initial_size, alignment).unwrap();
            let layout = Layout::from_size_align(initial_size, alignment).unwrap();

            // Write some data
            std::ptr::write_bytes(ptr.as_ptr(), 0xAB, initial_size);

            // Reallocate
            let new_ptr = allocator.realloc_aligned(ptr, layout, new_size).unwrap();

            // Verify alignment is preserved
            assert_eq!(
                new_ptr.as_ptr() as usize % alignment,
                0,
                "Reallocated pointer not aligned"
            );

            // Verify data is preserved
            let first_byte = std::ptr::read(new_ptr.as_ptr());
            assert_eq!(first_byte, 0xAB, "Data not preserved during reallocation");

            // Clean up
            let new_layout = Layout::from_size_align(new_size, alignment).unwrap();
            allocator.dealloc_aligned(new_ptr, new_layout);
        }
    }

    #[cfg(feature = "jemalloc")]
    #[test]
    fn test_jemalloc_stats() {
        let allocator = SimdAllocator::with_backend(AllocatorBackend::Jemalloc);

        // Allocate some memory
        unsafe {
            let ptr = allocator.alloc_aligned(1024 * 1024, 64).unwrap();

            let stats = allocator.stats();
            assert!(stats.allocated_bytes > 0);
            println!("Jemalloc stats: {:?}", stats);

            // Clean up
            let layout = Layout::from_size_align(1024 * 1024, 64).unwrap();
            allocator.dealloc_aligned(ptr, layout);
        }
    }
}
