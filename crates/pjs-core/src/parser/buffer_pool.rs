//! Buffer pool system for zero-copy parsing with memory management
//!
//! This module provides a memory pool system to minimize allocations during
//! JSON parsing, with support for different buffer sizes and reuse strategies.

use crate::domain::{DomainResult, DomainError};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// Buffer pool that manages reusable byte buffers for parsing
#[derive(Debug)]
pub struct BufferPool {
    pools: Arc<Mutex<HashMap<BufferSize, BufferBucket>>>,
    config: PoolConfig,
    stats: Arc<Mutex<PoolStats>>,
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
    size: BufferSize,
    last_access: Instant,
}

/// SIMD-aligned buffer with metadata
#[derive(Debug)]
pub struct AlignedBuffer {
    data: Vec<u8>,
    capacity: usize,
    alignment: usize,
    created_at: Instant,
    last_used: Instant,
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
            pools: Arc::new(Mutex::new(HashMap::new())),
            config,
            stats: Arc::new(Mutex::new(PoolStats::new())),
        }
    }

    /// Get buffer of specified size, reusing if available
    pub fn get_buffer(&self, size: BufferSize) -> DomainResult<PooledBuffer> {
        if self.config.track_stats {
            self.increment_allocations();
        }

        let mut pools = self.pools.lock()
            .map_err(|_| DomainError::InternalError("Failed to acquire pool lock".to_string()))?;

        if let Some(bucket) = pools.get_mut(&size) {
            if let Some(mut buffer) = bucket.buffers.pop() {
                buffer.last_used = Instant::now();
                bucket.last_access = Instant::now();
                
                if self.config.track_stats {
                    self.increment_cache_hits();
                }
                
                return Ok(PooledBuffer::new(buffer, Arc::clone(&self.pools), size));
            }
        }

        // No buffer available, create new one
        if self.config.track_stats {
            self.increment_cache_misses();
        }

        let buffer = AlignedBuffer::new(size as usize, self.config.simd_alignment)?;
        Ok(PooledBuffer::new(buffer, Arc::clone(&self.pools), size))
    }

    /// Get buffer with at least the specified capacity
    pub fn get_buffer_with_capacity(&self, min_capacity: usize) -> DomainResult<PooledBuffer> {
        let size = BufferSize::for_capacity(min_capacity);
        self.get_buffer(size)
    }

    /// Perform cleanup of old unused buffers
    pub fn cleanup(&self) -> DomainResult<CleanupStats> {
        let mut pools = self.pools.lock()
            .map_err(|_| DomainError::InternalError("Failed to acquire pool lock".to_string()))?;

        let now = Instant::now();
        let mut freed_buffers = 0;
        let mut freed_memory = 0;

        pools.retain(|_size, bucket| {
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
            
            // Keep bucket if it has buffers or was accessed recently
            !bucket.buffers.is_empty() || 
            now.duration_since(bucket.last_access) < self.config.buffer_ttl
        });

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
        let stats = self.stats.lock()
            .map_err(|_| DomainError::InternalError("Failed to acquire stats lock".to_string()))?;
        Ok(stats.clone())
    }

    /// Get current memory usage across all pools
    pub fn current_memory_usage(&self) -> DomainResult<usize> {
        let pools = self.pools.lock()
            .map_err(|_| DomainError::InternalError("Failed to acquire pool lock".to_string()))?;

        let usage = pools.values()
            .map(|bucket| bucket.buffers.iter().map(|b| b.capacity).sum::<usize>())
            .sum();

        Ok(usage)
    }

    // Private statistics methods
    
    fn increment_allocations(&self) {
        if let Ok(mut stats) = self.stats.lock() {
            stats.total_allocations += 1;
        }
    }

    fn increment_cache_hits(&self) {
        if let Ok(mut stats) = self.stats.lock() {
            stats.cache_hits += 1;
        }
    }

    fn increment_cache_misses(&self) {
        if let Ok(mut stats) = self.stats.lock() {
            stats.cache_misses += 1;
        }
    }

    fn increment_cleanup_count(&self) {
        if let Ok(mut stats) = self.stats.lock() {
            stats.cleanup_count += 1;
        }
    }

    fn update_current_memory_usage(&self, delta: i64) {
        if let Ok(mut stats) = self.stats.lock() {
            stats.current_memory_usage = (stats.current_memory_usage as i64 + delta).max(0) as usize;
            stats.peak_memory_usage = stats.peak_memory_usage.max(stats.current_memory_usage);
        }
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
    /// Create new aligned buffer
    fn new(capacity: usize, alignment: usize) -> DomainResult<Self> {
        // Align capacity to SIMD boundaries
        let aligned_capacity = (capacity + alignment - 1) & !(alignment - 1);
        
        let mut data = Vec::with_capacity(aligned_capacity);
        
        // Ensure the buffer is properly aligned in memory
        let ptr = data.as_ptr() as usize;
        if ptr % alignment != 0 {
            // If not aligned, we need to allocate extra space and adjust
            data = Vec::with_capacity(aligned_capacity + alignment);
            unsafe {
                let base = data.as_mut_ptr();
                let aligned = ((base as usize + alignment - 1) & !(alignment - 1)) as *mut u8;
                data.set_len((aligned as usize - base as usize) + aligned_capacity);
            }
        }

        let now = Instant::now();
        Ok(Self {
            data,
            capacity: aligned_capacity,
            alignment,
            created_at: now,
            last_used: now,
        })
    }

    /// Get mutable slice to buffer data
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.data
    }

    /// Get immutable slice to buffer data
    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }

    /// Clear buffer contents but keep allocated memory
    pub fn clear(&mut self) {
        self.data.clear();
        self.last_used = Instant::now();
    }

    /// Get buffer capacity
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Check if buffer is SIMD-aligned
    pub fn is_aligned(&self) -> bool {
        (self.data.as_ptr() as usize) % self.alignment == 0
    }
}

/// RAII wrapper for pooled buffer that returns buffer to pool on drop
pub struct PooledBuffer {
    buffer: Option<AlignedBuffer>,
    pool: Arc<Mutex<HashMap<BufferSize, BufferBucket>>>,
    size: BufferSize,
}

impl PooledBuffer {
    fn new(
        buffer: AlignedBuffer,
        pool: Arc<Mutex<HashMap<BufferSize, BufferBucket>>>,
        size: BufferSize,
    ) -> Self {
        Self {
            buffer: Some(buffer),
            pool,
            size,
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
            
            if let Ok(mut pools) = self.pool.lock() {
                let bucket = pools.entry(self.size).or_insert_with(|| BufferBucket {
                    buffers: Vec::new(),
                    size: self.size,
                    last_access: Instant::now(),
                });
                
                // Only return to pool if we haven't exceeded the limit
                if bucket.buffers.len() < 50 { // TODO: Use config value
                    bucket.buffers.push(buffer);
                    bucket.last_access = Instant::now();
                }
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
    /// Create configuration optimized for SIMD operations
    pub fn simd_optimized() -> Self {
        Self {
            max_buffers_per_bucket: 100,
            max_total_memory: 64 * 1024 * 1024, // 64MB
            buffer_ttl: Duration::from_secs(300), // 5 minutes
            track_stats: true,
            simd_alignment: 64, // AVX-512 alignment
        }
    }

    /// Create configuration for low-memory environments
    pub fn low_memory() -> Self {
        Self {
            max_buffers_per_bucket: 10,
            max_total_memory: 8 * 1024 * 1024, // 8MB
            buffer_ttl: Duration::from_secs(60), // 1 minute
            track_stats: false, // Reduce overhead
            simd_alignment: 32, // AVX2 alignment
        }
    }
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_buffers_per_bucket: 50,
            max_total_memory: 32 * 1024 * 1024, // 32MB
            buffer_ttl: Duration::from_secs(180), // 3 minutes
            track_stats: true,
            simd_alignment: 32, // AVX2 alignment
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
    GLOBAL_BUFFER_POOL.get_or_init(|| BufferPool::new())
}

/// Initialize global buffer pool with custom configuration
pub fn initialize_global_buffer_pool(config: PoolConfig) -> DomainResult<()> {
    GLOBAL_BUFFER_POOL.set(BufferPool::with_config(config))
        .map_err(|_| DomainError::InternalError("Global buffer pool already initialized".to_string()))?;
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
    fn test_aligned_buffer_creation() {
        let buffer = AlignedBuffer::new(1024, 64).unwrap();
        assert!(buffer.is_aligned());
        assert!(buffer.capacity() >= 1024);
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
}