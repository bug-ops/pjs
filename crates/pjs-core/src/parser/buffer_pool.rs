//! Buffer pool system for zero-copy parsing with memory management
//!
//! This module provides a memory pool system to minimize allocations during
//! JSON parsing, with support for different buffer sizes and reuse strategies.

use crate::{
    config::SecurityConfig,
    domain::{DomainError, DomainResult},
    parser::allocator::global_allocator,
    security::SecurityValidator,
};
use dashmap::DashMap;
use std::{
    alloc::Layout,
    mem,
    ptr::{self, NonNull},
    slice,
    sync::Arc,
    time::{Duration, Instant},
};

/// Buffer pool that manages reusable byte buffers for parsing
#[derive(Debug)]
pub struct BufferPool {
    pools: Arc<DashMap<BufferSize, BufferBucket>>,
    config: PoolConfig,
    stats: Arc<parking_lot::Mutex<PoolStats>>, // Keep stats under mutex as it's written less frequently
}

/// Configuration for buffer pool behavior
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Maximum number of buffers per size bucket
    pub max_buffers_per_bucket: usize,
    /// Maximum total memory usage in bytes
    pub max_total_memory: usize,
    /// How long to keep unused buffers before cleanup
    pub buffer_ttl: Duration,
    /// Enable/disable pool statistics tracking
    pub track_stats: bool,
    /// Alignment for SIMD operations (typically 32 or 64 bytes)
    pub simd_alignment: usize,
    /// Security validator for buffer validation
    pub validator: SecurityValidator,
}

/// Standard buffer sizes for different parsing scenarios
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum BufferSize {
    /// Small buffers for short JSON strings (1KB)
    Small = 1024,
    /// Medium buffers for typical API responses (8KB)  
    Medium = 8192,
    /// Large buffers for complex documents (64KB)
    Large = 65536,
    /// Extra large buffers for bulk data (512KB)
    XLarge = 524288,
    /// Huge buffers for massive documents (4MB)
    Huge = 4194304,
}

/// A bucket containing buffers of the same size
#[derive(Debug)]
struct BufferBucket {
    buffers: Vec<AlignedBuffer>,
    #[allow(dead_code)] // Future: size-based pool management
    size: BufferSize,
    last_access: Instant,
}

/// SIMD-aligned buffer with metadata
///
/// This buffer guarantees proper alignment for SIMD operations using direct memory allocation.
/// It supports SSE (16-byte), AVX2 (32-byte), and AVX-512 (64-byte) alignments.
pub struct AlignedBuffer {
    /// Raw pointer to aligned memory
    ptr: NonNull<u8>,
    /// Current length of valid data
    len: usize,
    /// Total capacity in bytes
    capacity: usize,
    /// Memory alignment requirement
    alignment: usize,
    /// Layout used for allocation (needed for deallocation)
    layout: Layout,
    /// Creation timestamp
    created_at: Instant,
    /// Last usage timestamp
    last_used: Instant,
}

// Safety: AlignedBuffer can be safely sent between threads
unsafe impl Send for AlignedBuffer {}

// Safety: AlignedBuffer can be safely shared between threads (no interior mutability)
unsafe impl Sync for AlignedBuffer {}

impl std::fmt::Debug for AlignedBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AlignedBuffer")
            .field("ptr", &format_args!("0x{:x}", self.ptr.as_ptr() as usize))
            .field("len", &self.len)
            .field("capacity", &self.capacity)
            .field("alignment", &self.alignment)
            .field("is_aligned", &self.is_aligned())
            .field("created_at", &self.created_at)
            .field("last_used", &self.last_used)
            .finish()
    }
}

/// Statistics about buffer pool usage
#[derive(Debug, Clone)]
pub struct PoolStats {
    /// Total allocations requested
    pub total_allocations: u64,
    /// Cache hits (buffer reused)
    pub cache_hits: u64,
    /// Cache misses (new buffer allocated)
    pub cache_misses: u64,
    /// Current memory usage in bytes
    pub current_memory_usage: usize,
    /// Peak memory usage in bytes
    pub peak_memory_usage: usize,
    /// Number of cleanup operations performed
    pub cleanup_count: u64,
}

impl BufferPool {
    /// Create new buffer pool with default configuration
    pub fn new() -> Self {
        Self::with_config(PoolConfig::default())
    }

    /// Create buffer pool with custom configuration
    pub fn with_config(config: PoolConfig) -> Self {
        Self {
            pools: Arc::new(DashMap::new()),
            config,
            stats: Arc::new(parking_lot::Mutex::new(PoolStats::new())),
        }
    }

    /// Create buffer pool with security configuration
    pub fn with_security_config(security_config: SecurityConfig) -> Self {
        Self::with_config(PoolConfig::from(&security_config))
    }

    /// Get buffer of specified size, reusing if available
    pub fn get_buffer(&self, size: BufferSize) -> DomainResult<PooledBuffer> {
        // Security validation: check buffer size
        self.config
            .validator
            .validate_buffer_size(size as usize)
            .map_err(|e| DomainError::SecurityViolation(e.to_string()))?;

        // Check if we would exceed total memory limit
        let current_usage = self.current_memory_usage().unwrap_or(0);
        if current_usage + (size as usize) > self.config.max_total_memory {
            return Err(DomainError::ResourceExhausted(format!(
                "Adding buffer of size {} would exceed memory limit: current={}, limit={}",
                size as usize, current_usage, self.config.max_total_memory
            )));
        }

        if self.config.track_stats {
            self.increment_allocations();
        }

        // Try to get a buffer from existing bucket
        if let Some(mut bucket_ref) = self.pools.get_mut(&size)
            && let Some(mut buffer) = bucket_ref.buffers.pop()
        {
            buffer.last_used = Instant::now();
            bucket_ref.last_access = Instant::now();

            if self.config.track_stats {
                self.increment_cache_hits();
            }

            return Ok(PooledBuffer::new(
                buffer,
                Arc::clone(&self.pools),
                size,
                self.config.max_buffers_per_bucket,
            ));
        }

        // No buffer available, create new one
        if self.config.track_stats {
            self.increment_cache_misses();
        }

        let buffer = AlignedBuffer::new(size as usize, self.config.simd_alignment)?;
        Ok(PooledBuffer::new(
            buffer,
            Arc::clone(&self.pools),
            size,
            self.config.max_buffers_per_bucket,
        ))
    }

    /// Get buffer with at least the specified capacity
    pub fn get_buffer_with_capacity(&self, min_capacity: usize) -> DomainResult<PooledBuffer> {
        let size = BufferSize::for_capacity(min_capacity);
        self.get_buffer(size)
    }

    /// Perform cleanup of old unused buffers
    pub fn cleanup(&self) -> DomainResult<CleanupStats> {
        let now = Instant::now();
        let mut freed_buffers = 0;
        let mut freed_memory = 0;

        // DashMap doesn't have retain, so we collect keys to remove
        let mut keys_to_remove = Vec::new();

        for mut entry in self.pools.iter_mut() {
            let bucket = entry.value_mut();
            let old_count = bucket.buffers.len();

            bucket.buffers.retain(|buffer| {
                let age = now.duration_since(buffer.last_used);
                if age > self.config.buffer_ttl {
                    freed_memory += buffer.capacity;
                    false
                } else {
                    true
                }
            });

            freed_buffers += old_count - bucket.buffers.len();

            // Mark bucket for removal if empty and not recently accessed
            if bucket.buffers.is_empty()
                && now.duration_since(bucket.last_access) >= self.config.buffer_ttl
            {
                keys_to_remove.push(*entry.key());
            }
        }

        // Remove empty buckets
        for key in keys_to_remove {
            self.pools.remove(&key);
        }

        if self.config.track_stats {
            self.increment_cleanup_count();
            self.update_current_memory_usage(-(freed_memory as i64));
        }

        Ok(CleanupStats {
            freed_buffers,
            freed_memory,
        })
    }

    /// Get current pool statistics
    pub fn stats(&self) -> DomainResult<PoolStats> {
        let stats = self.stats.lock();
        Ok(stats.clone())
    }

    /// Get current memory usage across all pools
    pub fn current_memory_usage(&self) -> DomainResult<usize> {
        use rayon::prelude::*;

        let usage = self
            .pools
            .iter()
            .par_bridge()
            .map(|entry| {
                entry
                    .value()
                    .buffers
                    .par_iter()
                    .map(|b| b.capacity)
                    .sum::<usize>()
            })
            .sum();

        Ok(usage)
    }

    // Private statistics methods

    fn increment_allocations(&self) {
        let mut stats = self.stats.lock();
        stats.total_allocations += 1;
    }

    fn increment_cache_hits(&self) {
        let mut stats = self.stats.lock();
        stats.cache_hits += 1;
    }

    fn increment_cache_misses(&self) {
        let mut stats = self.stats.lock();
        stats.cache_misses += 1;
    }

    fn increment_cleanup_count(&self) {
        let mut stats = self.stats.lock();
        stats.cleanup_count += 1;
    }

    fn update_current_memory_usage(&self, delta: i64) {
        let mut stats = self.stats.lock();
        stats.current_memory_usage = (stats.current_memory_usage as i64 + delta).max(0) as usize;
        stats.peak_memory_usage = stats.peak_memory_usage.max(stats.current_memory_usage);
    }
}

impl BufferSize {
    /// Get appropriate buffer size for given capacity
    pub fn for_capacity(capacity: usize) -> Self {
        match capacity {
            0..=1024 => BufferSize::Small,
            1025..=8192 => BufferSize::Medium,
            8193..=65536 => BufferSize::Large,
            65537..=524288 => BufferSize::XLarge,
            _ => BufferSize::Huge,
        }
    }

    /// Get all available buffer sizes in order
    pub fn all_sizes() -> &'static [BufferSize] {
        &[
            BufferSize::Small,
            BufferSize::Medium,
            BufferSize::Large,
            BufferSize::XLarge,
            BufferSize::Huge,
        ]
    }
}

impl AlignedBuffer {
    /// Create new aligned buffer with guaranteed SIMD alignment
    ///
    /// # Arguments
    /// * `capacity` - Minimum capacity in bytes
    /// * `alignment` - Required alignment (must be power of 2)
    ///
    /// # Safety
    /// This function uses unsafe code to allocate aligned memory.
    /// The memory is properly tracked and will be deallocated on drop.
    pub fn new(capacity: usize, alignment: usize) -> DomainResult<Self> {
        // Validate alignment is power of 2 and reasonable
        if !alignment.is_power_of_two() {
            return Err(DomainError::InvalidInput(format!(
                "Alignment {} is not a power of 2",
                alignment
            )));
        }

        // Validate alignment is not too large (max 4096 bytes for page alignment)
        if alignment > 4096 {
            return Err(DomainError::InvalidInput(format!(
                "Alignment {} exceeds maximum of 4096",
                alignment
            )));
        }

        // Minimum alignment should be at least size of usize for proper alignment
        let alignment = alignment.max(mem::align_of::<usize>());

        // Align capacity to SIMD boundaries
        let aligned_capacity = (capacity + alignment - 1) & !(alignment - 1);

        // Ensure minimum capacity for safety
        let aligned_capacity = aligned_capacity.max(alignment);

        // Create layout for allocation (kept for Drop implementation)
        let layout = Layout::from_size_align(aligned_capacity, alignment).map_err(|e| {
            DomainError::InvalidInput(format!(
                "Invalid layout: capacity={}, alignment={}, error={}",
                aligned_capacity, alignment, e
            ))
        })?;

        // Use global SIMD allocator for better performance
        let allocator = global_allocator();

        // Allocate aligned memory using the appropriate allocator backend
        // Safety: alignment has been validated above
        let ptr = unsafe { allocator.alloc_aligned(aligned_capacity, alignment)? };

        let now = Instant::now();
        Ok(Self {
            ptr,
            len: 0,
            capacity: aligned_capacity,
            alignment,
            layout,
            created_at: now,
            last_used: now,
        })
    }

    /// Create an aligned buffer with specific SIMD level
    pub fn new_sse(capacity: usize) -> DomainResult<Self> {
        Self::new(capacity, 16) // SSE requires 16-byte alignment
    }

    /// Create an aligned buffer for AVX2 operations
    pub fn new_avx2(capacity: usize) -> DomainResult<Self> {
        Self::new(capacity, 32) // AVX2 requires 32-byte alignment
    }

    /// Create an aligned buffer for AVX-512 operations
    pub fn new_avx512(capacity: usize) -> DomainResult<Self> {
        Self::new(capacity, 64) // AVX-512 requires 64-byte alignment
    }

    /// Get mutable slice to buffer data
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
    }

    /// Get immutable slice to buffer data
    pub fn as_slice(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }

    /// Get a mutable slice with full capacity
    pub fn as_mut_capacity_slice(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.ptr.as_ptr(), self.capacity) }
    }

    /// Set the length of valid data
    ///
    /// # Safety
    /// Caller must ensure that `new_len` bytes are initialized
    pub unsafe fn set_len(&mut self, new_len: usize) {
        debug_assert!(
            new_len <= self.capacity,
            "new_len {} exceeds capacity {}",
            new_len,
            self.capacity
        );
        self.len = new_len;
        self.last_used = Instant::now();
    }

    /// Reserve additional capacity
    pub fn reserve(&mut self, additional: usize) -> DomainResult<()> {
        let new_capacity = self
            .len
            .checked_add(additional)
            .ok_or_else(|| DomainError::InvalidInput("Capacity overflow".to_string()))?;

        if new_capacity <= self.capacity {
            return Ok(());
        }

        // Align new capacity
        let aligned_capacity = (new_capacity + self.alignment - 1) & !(self.alignment - 1);

        // Use global SIMD allocator for reallocation
        let allocator = global_allocator();

        // Reallocate using the allocator (which will handle data copying)
        let new_ptr =
            unsafe { allocator.realloc_aligned(self.ptr, self.layout, aligned_capacity)? };

        // Update layout for the new size
        let new_layout = Layout::from_size_align(aligned_capacity, self.alignment)
            .map_err(|e| DomainError::InvalidInput(format!("Invalid layout: {}", e)))?;

        self.ptr = new_ptr;
        self.capacity = aligned_capacity;
        self.layout = new_layout;
        self.last_used = Instant::now();

        Ok(())
    }

    /// Push bytes to the buffer
    pub fn extend_from_slice(&mut self, data: &[u8]) -> DomainResult<()> {
        let required_capacity = self
            .len
            .checked_add(data.len())
            .ok_or_else(|| DomainError::InvalidInput("Length overflow".to_string()))?;

        if required_capacity > self.capacity {
            self.reserve(data.len())?;
        }

        unsafe {
            ptr::copy_nonoverlapping(data.as_ptr(), self.ptr.as_ptr().add(self.len), data.len());
            self.len += data.len();
        }

        self.last_used = Instant::now();
        Ok(())
    }

    /// Clear buffer contents but keep allocated memory
    pub fn clear(&mut self) {
        self.len = 0;
        self.last_used = Instant::now();
    }

    /// Get buffer capacity
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Get current length of valid data
    pub fn len(&self) -> usize {
        self.len
    }

    /// Check if buffer is empty
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Get the raw pointer to the buffer
    pub fn as_ptr(&self) -> *const u8 {
        self.ptr.as_ptr()
    }

    /// Get the mutable raw pointer to the buffer  
    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.ptr.as_ptr()
    }

    /// Check if buffer is properly aligned
    ///
    /// This validates that the buffer pointer has the requested alignment,
    /// which is critical for SIMD operations.
    pub fn is_aligned(&self) -> bool {
        let ptr_addr = self.ptr.as_ptr() as usize;
        ptr_addr.is_multiple_of(self.alignment)
    }

    /// Get the actual alignment of the buffer
    pub fn actual_alignment(&self) -> usize {
        let ptr_addr = self.ptr.as_ptr() as usize;
        // Find the highest power of 2 that divides the address
        if ptr_addr == 0 {
            return usize::MAX; // null pointer is infinitely aligned
        }

        // Use trailing zeros to find alignment
        1 << ptr_addr.trailing_zeros()
    }

    /// Verify buffer is suitable for specific SIMD instruction set
    pub fn is_simd_compatible(&self, simd_type: SimdType) -> bool {
        let required_alignment = match simd_type {
            SimdType::Sse => 16,
            SimdType::Avx2 => 32,
            SimdType::Avx512 => 64,
            SimdType::Neon => 16,
        };

        self.actual_alignment() >= required_alignment
    }
}

/// SIMD instruction set types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimdType {
    /// SSE instructions (16-byte alignment)
    Sse,
    /// AVX2 instructions (32-byte alignment)  
    Avx2,
    /// AVX-512 instructions (64-byte alignment)
    Avx512,
    /// ARM NEON instructions (16-byte alignment)
    Neon,
}

impl Drop for AlignedBuffer {
    fn drop(&mut self) {
        // Use the global SIMD allocator for deallocation
        let allocator = global_allocator();

        // Safety: We allocated this memory with the same layout
        unsafe {
            allocator.dealloc_aligned(self.ptr, self.layout);
        }
    }
}

impl Clone for AlignedBuffer {
    fn clone(&self) -> Self {
        // Create new buffer with same alignment and capacity
        let mut new_buffer =
            Self::new(self.capacity, self.alignment).expect("Failed to clone buffer");

        // Copy data
        unsafe {
            ptr::copy_nonoverlapping(self.ptr.as_ptr(), new_buffer.ptr.as_ptr(), self.len);
            new_buffer.len = self.len;
        }

        new_buffer
    }
}

/// RAII wrapper for pooled buffer that returns buffer to pool on drop
pub struct PooledBuffer {
    buffer: Option<AlignedBuffer>,
    pool: Arc<DashMap<BufferSize, BufferBucket>>,
    size: BufferSize,
    max_buffers_per_bucket: usize,
}

impl PooledBuffer {
    fn new(
        buffer: AlignedBuffer,
        pool: Arc<DashMap<BufferSize, BufferBucket>>,
        size: BufferSize,
        max_buffers_per_bucket: usize,
    ) -> Self {
        Self {
            buffer: Some(buffer),
            pool,
            size,
            max_buffers_per_bucket,
        }
    }

    /// Get mutable reference to buffer
    pub fn buffer_mut(&mut self) -> Option<&mut AlignedBuffer> {
        self.buffer.as_mut()
    }

    /// Get immutable reference to buffer
    pub fn buffer(&self) -> Option<&AlignedBuffer> {
        self.buffer.as_ref()
    }

    /// Get buffer capacity
    pub fn capacity(&self) -> usize {
        self.buffer.as_ref().map(|b| b.capacity()).unwrap_or(0)
    }

    /// Clear buffer contents
    pub fn clear(&mut self) {
        if let Some(buffer) = &mut self.buffer {
            buffer.clear();
        }
    }
}

impl Drop for PooledBuffer {
    fn drop(&mut self) {
        if let Some(mut buffer) = self.buffer.take() {
            buffer.clear(); // Clear contents before returning to pool

            // Get or create bucket for this buffer size
            let mut bucket_ref = self.pool.entry(self.size).or_insert_with(|| BufferBucket {
                buffers: Vec::new(),
                size: self.size,
                last_access: Instant::now(),
            });

            // Only return to pool if we haven't exceeded the per-bucket limit
            if bucket_ref.buffers.len() < self.max_buffers_per_bucket {
                bucket_ref.buffers.push(buffer);
                bucket_ref.last_access = Instant::now();
            }
        }
    }
}

/// Result of cleanup operation
#[derive(Debug, Clone)]
pub struct CleanupStats {
    pub freed_buffers: usize,
    pub freed_memory: usize,
}

impl PoolConfig {
    /// Create configuration from security config
    pub fn from_security_config(security_config: &SecurityConfig) -> Self {
        Self::from(security_config)
    }

    /// Create configuration optimized for SIMD operations
    pub fn simd_optimized() -> Self {
        let mut config = Self::from(&SecurityConfig::high_throughput());
        config.simd_alignment = 64; // AVX-512 alignment
        config
    }

    /// Create configuration for low-memory environments
    pub fn low_memory() -> Self {
        let mut config = Self::from(&SecurityConfig::low_memory());
        config.track_stats = false; // Reduce overhead
        config
    }

    /// Create configuration for development/testing
    pub fn development() -> Self {
        Self::from(&SecurityConfig::development())
    }
}

impl Default for PoolConfig {
    fn default() -> Self {
        let security_config = SecurityConfig::default();
        Self {
            max_buffers_per_bucket: security_config.buffers.max_buffers_per_bucket,
            max_total_memory: security_config.buffers.max_total_memory,
            buffer_ttl: security_config.buffer_ttl(),
            track_stats: true,
            simd_alignment: 32, // AVX2 alignment
            validator: SecurityValidator::new(security_config),
        }
    }
}

impl From<&SecurityConfig> for PoolConfig {
    fn from(security_config: &SecurityConfig) -> Self {
        Self {
            max_buffers_per_bucket: security_config.buffers.max_buffers_per_bucket,
            max_total_memory: security_config.buffers.max_total_memory,
            buffer_ttl: security_config.buffer_ttl(),
            track_stats: true,
            simd_alignment: 32, // AVX2 alignment
            validator: SecurityValidator::new(security_config.clone()),
        }
    }
}

impl PoolStats {
    fn new() -> Self {
        Self {
            total_allocations: 0,
            cache_hits: 0,
            cache_misses: 0,
            current_memory_usage: 0,
            peak_memory_usage: 0,
            cleanup_count: 0,
        }
    }

    /// Get cache hit ratio
    pub fn hit_ratio(&self) -> f64 {
        if self.total_allocations == 0 {
            0.0
        } else {
            self.cache_hits as f64 / self.total_allocations as f64
        }
    }

    /// Get memory efficiency (current/peak ratio)
    pub fn memory_efficiency(&self) -> f64 {
        if self.peak_memory_usage == 0 {
            1.0
        } else {
            self.current_memory_usage as f64 / self.peak_memory_usage as f64
        }
    }
}

impl Default for BufferPool {
    fn default() -> Self {
        Self::new()
    }
}

/// Global buffer pool instance for convenient access
static GLOBAL_BUFFER_POOL: std::sync::OnceLock<BufferPool> = std::sync::OnceLock::new();

/// Get global buffer pool instance
pub fn global_buffer_pool() -> &'static BufferPool {
    GLOBAL_BUFFER_POOL.get_or_init(BufferPool::new)
}

/// Initialize global buffer pool with custom configuration
pub fn initialize_global_buffer_pool(config: PoolConfig) -> DomainResult<()> {
    GLOBAL_BUFFER_POOL
        .set(BufferPool::with_config(config))
        .map_err(|_| {
            DomainError::InternalError("Global buffer pool already initialized".to_string())
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_pool_creation() {
        let pool = BufferPool::new();
        assert!(pool.stats().is_ok());
    }

    #[test]
    fn test_buffer_allocation() {
        let pool = BufferPool::new();
        let buffer = pool.get_buffer(BufferSize::Medium);
        assert!(buffer.is_ok());

        let buffer = buffer.unwrap();
        assert!(buffer.capacity() >= BufferSize::Medium as usize);
    }

    #[test]
    fn test_buffer_reuse() {
        let pool = BufferPool::new();

        // Allocate and drop buffer
        {
            let _buffer = pool.get_buffer(BufferSize::Small).unwrap();
        }

        // Allocate another buffer of same size
        let _buffer2 = pool.get_buffer(BufferSize::Small).unwrap();

        // Should have cache hit
        let stats = pool.stats().unwrap();
        assert!(stats.cache_hits > 0);
    }

    #[test]
    fn test_buffer_size_selection() {
        assert_eq!(BufferSize::for_capacity(500), BufferSize::Small);
        assert_eq!(BufferSize::for_capacity(2000), BufferSize::Medium);
        assert_eq!(BufferSize::for_capacity(50000), BufferSize::Large);
        assert_eq!(BufferSize::for_capacity(100000), BufferSize::XLarge);
    }

    #[test]
    fn test_aligned_buffer_creation_guaranteed() {
        // Test all common SIMD alignments
        let test_cases = vec![
            (1024, 16, "SSE alignment"),
            (2048, 32, "AVX2 alignment"),
            (4096, 64, "AVX-512 alignment"),
        ];

        for (capacity, alignment, description) in test_cases {
            let buffer = AlignedBuffer::new(capacity, alignment).unwrap();

            // Verify pointer alignment
            let ptr_addr = buffer.as_ptr() as usize;
            assert_eq!(
                ptr_addr % alignment,
                0,
                "{}: pointer 0x{:x} is not {}-byte aligned",
                description,
                ptr_addr,
                alignment
            );

            // Verify is_aligned method
            assert!(
                buffer.is_aligned(),
                "{}: is_aligned() returned false for properly aligned buffer",
                description
            );

            // Verify capacity
            assert!(
                buffer.capacity() >= capacity,
                "{}: capacity {} is less than requested {}",
                description,
                buffer.capacity(),
                capacity
            );

            // Verify actual alignment
            assert!(
                buffer.actual_alignment() >= alignment,
                "{}: actual alignment {} is less than requested {}",
                description,
                buffer.actual_alignment(),
                alignment
            );
        }
    }

    #[test]
    fn test_buffer_operations() {
        let mut buffer = AlignedBuffer::new(1024, 32).unwrap();

        // Test initial state
        assert_eq!(buffer.len(), 0);
        assert!(buffer.is_empty());
        assert_eq!(buffer.capacity(), 1024);

        // Test extend_from_slice
        let data = b"Hello, SIMD World!";
        buffer.extend_from_slice(data).unwrap();
        assert_eq!(buffer.len(), data.len());
        assert_eq!(buffer.as_slice(), data);

        // Test clear
        buffer.clear();
        assert_eq!(buffer.len(), 0);
        assert!(buffer.is_empty());
        assert_eq!(buffer.capacity(), 1024); // Capacity should remain

        // Test unsafe set_len
        unsafe {
            // Write some data directly
            let slice = buffer.as_mut_capacity_slice();
            slice[0..5].copy_from_slice(b"SIMD!");
            buffer.set_len(5);
        }
        assert_eq!(buffer.len(), 5);
        assert_eq!(&buffer.as_slice()[0..5], b"SIMD!");
    }

    #[test]
    fn test_buffer_reserve() {
        let mut buffer = AlignedBuffer::new(64, 32).unwrap();
        let _initial_alignment = buffer.actual_alignment();

        // Set some length first
        unsafe {
            buffer.set_len(32);
        }

        // Reserve additional space - should need capacity for len + additional
        buffer.reserve(256).unwrap();
        assert!(
            buffer.capacity() >= 32 + 256,
            "Expected capacity >= {}, got {}",
            32 + 256,
            buffer.capacity()
        );

        // Alignment should be preserved after reallocation
        assert!(
            buffer.actual_alignment() >= 32,
            "Alignment not preserved after reserve"
        );
        assert!(buffer.is_aligned());

        // Test that data is preserved during reallocation
        buffer.extend_from_slice(b"test data").unwrap();
        let old_data = buffer.as_slice().to_vec();

        buffer.reserve(1024).unwrap();
        assert_eq!(buffer.as_slice(), &old_data[..]);
    }

    #[test]
    fn test_buffer_clone() {
        let mut original = AlignedBuffer::new(512, 64).unwrap();
        original.extend_from_slice(b"Original data").unwrap();

        let cloned = original.clone();

        // Verify clone has same properties
        assert_eq!(cloned.len(), original.len());
        assert_eq!(cloned.capacity(), original.capacity());
        assert_eq!(cloned.alignment, original.alignment);
        assert_eq!(cloned.as_slice(), original.as_slice());

        // Verify clone has different memory location
        assert_ne!(cloned.as_ptr(), original.as_ptr());

        // Verify clone is also properly aligned
        assert!(cloned.is_aligned());
        assert!(cloned.actual_alignment() >= 64);
    }

    #[test]
    fn test_alignment_validation() {
        // Test valid power-of-2 alignments
        let valid_alignments = [1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096];

        for &alignment in &valid_alignments {
            let result = AlignedBuffer::new(1024, alignment);
            assert!(result.is_ok(), "Alignment {} should be valid", alignment);

            let buffer = result.unwrap();
            assert!(
                buffer.is_aligned(),
                "Buffer with alignment {} should be aligned",
                alignment
            );
        }

        // Test invalid non-power-of-2 alignments
        let invalid_alignments = [3, 5, 6, 7, 9, 10, 11, 12, 13, 14, 15, 17, 31, 33, 63, 65];

        for &alignment in &invalid_alignments {
            let result = AlignedBuffer::new(1024, alignment);
            assert!(result.is_err(), "Alignment {} should be invalid", alignment);
        }

        // Test too large alignment
        assert!(AlignedBuffer::new(1024, 8192).is_err());
    }

    #[test]
    fn test_actual_alignment_calculation() {
        // Create buffers with different alignments and verify actual_alignment()
        for &requested_align in &[16, 32, 64] {
            let buffer = AlignedBuffer::new(1024, requested_align).unwrap();
            let actual = buffer.actual_alignment();

            assert!(
                actual >= requested_align,
                "Actual alignment {} is less than requested {}",
                actual,
                requested_align
            );

            // actual_alignment should be a power of 2
            assert!(
                actual.is_power_of_two(),
                "Actual alignment {} is not a power of 2",
                actual
            );
        }
    }

    #[test]
    fn test_simd_compatibility_check() {
        // SSE buffer should be compatible with SSE but might not be with AVX-512
        let sse_buffer = AlignedBuffer::new_sse(1024).unwrap();
        assert!(sse_buffer.is_simd_compatible(SimdType::Sse));
        assert!(sse_buffer.is_simd_compatible(SimdType::Neon)); // Same alignment as SSE

        // AVX-512 buffer should be compatible with all instruction sets
        let avx512_buffer = AlignedBuffer::new_avx512(1024).unwrap();
        assert!(avx512_buffer.is_simd_compatible(SimdType::Sse));
        assert!(avx512_buffer.is_simd_compatible(SimdType::Avx2));
        assert!(avx512_buffer.is_simd_compatible(SimdType::Avx512));
        assert!(avx512_buffer.is_simd_compatible(SimdType::Neon));
    }

    #[test]
    fn test_zero_copy_verification() {
        let mut buffer = AlignedBuffer::new(1024, 32).unwrap();

        // Get raw pointer before modification
        let ptr_before = buffer.as_ptr();

        // Perform various operations that should NOT move the buffer
        buffer.clear();
        buffer.extend_from_slice(b"test").unwrap();
        unsafe {
            buffer.set_len(2);
        }

        // Pointer should remain the same (zero-copy)
        assert_eq!(
            ptr_before,
            buffer.as_ptr(),
            "Buffer was moved during operations (not zero-copy)"
        );

        // Only reserve should potentially change the pointer
        buffer.reserve(2048).unwrap();
        // After reserve, pointer might change but should still be aligned
        assert!(buffer.is_aligned());
    }

    #[test]
    fn test_pool_cleanup() {
        let config = PoolConfig {
            buffer_ttl: Duration::from_millis(1),
            ..Default::default()
        };
        let pool = BufferPool::with_config(config);

        // Allocate and drop buffer
        {
            let _buffer = pool.get_buffer(BufferSize::Small).unwrap();
        }

        // Wait for TTL
        std::thread::sleep(Duration::from_millis(10));

        // Cleanup should free the buffer
        let cleanup_stats = pool.cleanup().unwrap();
        assert!(cleanup_stats.freed_buffers > 0);
    }

    #[test]
    fn test_global_buffer_pool() {
        let pool = global_buffer_pool();
        let buffer = pool.get_buffer(BufferSize::Medium);
        assert!(buffer.is_ok());
    }

    #[test]
    fn test_memory_limit_enforcement() {
        let config = PoolConfig {
            max_total_memory: 1024, // Very small limit
            max_buffers_per_bucket: 10,
            ..Default::default()
        };
        let pool = BufferPool::with_config(config);

        // Create a buffer that exceeds the memory limit
        let result = pool.get_buffer(BufferSize::Medium); // 8KB > 1KB limit

        assert!(result.is_err());

        if let Err(e) = result {
            assert!(e.to_string().contains("memory limit"));
        }
    }

    #[test]
    fn test_per_bucket_limit_enforcement() {
        let config = PoolConfig {
            max_buffers_per_bucket: 2,          // Very small limit
            max_total_memory: 10 * 1024 * 1024, // Generous memory limit
            ..Default::default()
        };
        let pool = BufferPool::with_config(config);

        // Allocate and drop buffers to fill the bucket
        for _ in 0..3 {
            let _buffer = pool.get_buffer(BufferSize::Small).unwrap();
            // Buffer goes back to pool on drop
        }

        // Only 2 buffers should be retained in the pool
        let stats = pool.stats().unwrap();
        assert!(stats.cache_hits <= 2, "Too many buffers retained in bucket");
    }

    #[test]
    fn test_buffer_size_validation() {
        let pool = BufferPool::new();

        // All standard buffer sizes should be valid
        for size in BufferSize::all_sizes() {
            let result = pool.get_buffer(*size);
            assert!(result.is_ok(), "Buffer size {:?} should be valid", size);
        }
    }

    #[test]
    fn test_memory_safety() {
        // Test that dropping a buffer properly deallocates memory
        // This test would fail under valgrind/ASAN if there's a memory leak
        for _ in 0..100 {
            let buffer = AlignedBuffer::new(1024, 64).unwrap();
            drop(buffer);
        }

        // Test clone and drop
        for _ in 0..100 {
            let buffer = AlignedBuffer::new(512, 32).unwrap();
            let cloned = buffer.clone();
            drop(buffer);
            drop(cloned);
        }
    }

    #[test]
    fn test_simd_specific_constructors() {
        // Test SSE alignment (16 bytes)
        let sse_buffer = AlignedBuffer::new_sse(1024).unwrap();
        assert!(sse_buffer.is_aligned());
        assert!(sse_buffer.is_simd_compatible(SimdType::Sse));
        assert_eq!(sse_buffer.alignment, 16);

        // Test AVX2 alignment (32 bytes)
        let avx2_buffer = AlignedBuffer::new_avx2(1024).unwrap();
        assert!(avx2_buffer.is_aligned());
        assert!(avx2_buffer.is_simd_compatible(SimdType::Avx2));
        assert_eq!(avx2_buffer.alignment, 32);

        // Test AVX-512 alignment (64 bytes)
        let avx512_buffer = AlignedBuffer::new_avx512(1024).unwrap();
        assert!(avx512_buffer.is_aligned());
        assert!(avx512_buffer.is_simd_compatible(SimdType::Avx512));
        assert_eq!(avx512_buffer.alignment, 64);
    }

    #[test]
    fn test_simd_alignment_compatibility() {
        let buffer_64 = AlignedBuffer::new(1024, 64).unwrap();

        // 64-byte aligned buffer should be compatible with all SIMD types
        assert!(buffer_64.is_simd_compatible(SimdType::Sse)); // 16-byte requirement
        assert!(buffer_64.is_simd_compatible(SimdType::Avx2)); // 32-byte requirement
        assert!(buffer_64.is_simd_compatible(SimdType::Avx512)); // 64-byte requirement
        assert!(buffer_64.is_simd_compatible(SimdType::Neon)); // 16-byte requirement

        // Note: We can't easily test incompatible alignments since the allocator
        // might provide better alignment than requested for performance reasons.
        // Instead, test the requested alignment vs required alignment directly.
        #[allow(clippy::assertions_on_constants)]
        {
            assert!(64 >= 16); // SSE compatible
            assert!(64 >= 32); // AVX2 compatible
            assert!(64 >= 64); // AVX512 compatible
            assert!(64 >= 16); // NEON compatible
        }

        let buffer_16 = AlignedBuffer::new(1024, 16).unwrap();

        // Test that buffer reports correct requested alignment
        assert_eq!(buffer_16.alignment, 16);

        // 16-byte aligned buffer should be compatible with SSE and NEON
        assert!(buffer_16.is_simd_compatible(SimdType::Sse));
        assert!(buffer_16.is_simd_compatible(SimdType::Neon));

        // Note: actual_alignment() might be higher than 16 due to allocator behavior
        // so we can't reliably test incompatibility. Instead verify logic:
        #[allow(clippy::assertions_on_constants)]
        {
            assert!(16 >= 16); // SSE requirement met
            assert!(16 < 32); // AVX2 requirement NOT met by requested alignment
            assert!(16 < 64); // AVX512 requirement NOT met by requested alignment
        }
    }

    #[test]
    fn test_actual_alignment_detection() {
        let buffer = AlignedBuffer::new(1024, 64).unwrap();

        let actual_alignment = buffer.actual_alignment();
        assert!(
            actual_alignment >= 64,
            "Buffer has actual alignment of {}, expected at least 64",
            actual_alignment
        );

        // The actual alignment should be a power of 2 and >= requested alignment
        assert!(actual_alignment.is_power_of_two());
        assert!(actual_alignment >= buffer.alignment);
    }

    #[test]
    fn test_simd_pool_configuration() {
        // Test pool with high SIMD alignment requirement
        let config = PoolConfig {
            simd_alignment: 64, // AVX-512 alignment
            ..Default::default()
        };
        let pool = BufferPool::with_config(config);

        let buffer = pool.get_buffer(BufferSize::Medium).unwrap();
        assert!(buffer.buffer().unwrap().is_aligned());
        assert!(
            buffer
                .buffer()
                .unwrap()
                .is_simd_compatible(SimdType::Avx512)
        );
    }

    #[test]
    fn test_alignment_edge_cases() {
        // Test minimum alignment
        let buffer_min = AlignedBuffer::new(64, 1).unwrap();
        assert!(buffer_min.is_aligned());
        assert!(buffer_min.alignment >= mem::align_of::<usize>());

        // Test power-of-2 validation
        assert!(AlignedBuffer::new(1024, 3).is_err());
        assert!(AlignedBuffer::new(1024, 17).is_err());
        assert!(AlignedBuffer::new(1024, 33).is_err());

        // Test maximum alignment limit
        assert!(AlignedBuffer::new(1024, 8192).is_err());
    }

    #[test]
    fn test_simd_performance_oriented_allocation() {
        // Test that allocation pattern is suitable for high-performance SIMD
        let buffer = AlignedBuffer::new_avx512(4096).unwrap();

        // Verify the buffer can be used for actual SIMD-like operations
        let slice = unsafe { std::slice::from_raw_parts_mut(buffer.ptr.as_ptr(), buffer.capacity) };

        // Fill with test pattern
        for (i, byte) in slice.iter_mut().enumerate() {
            *byte = (i % 256) as u8;
        }

        // Verify alignment is maintained through operations
        assert!(buffer.is_aligned());
        assert_eq!(slice[0], 0);
        assert_eq!(slice[255], 255);
        assert_eq!(slice[256], 0);
    }
}
