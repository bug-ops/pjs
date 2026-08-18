//! JSON parsing module: zero-copy lazy values, optional streaming
//! partial-parse support, and the supporting buffer pool / aligned
//! allocator infrastructure.

#[cfg(feature = "partial-parse")]
pub mod partial;

#[cfg(feature = "partial-parse")]
pub use partial::{
    JiterConfig, JiterPartialParser, ParseDiagnostic, PartialJsonParser, PartialParseResult,
    StreamingHint,
};

pub mod aligned_alloc;
pub mod buffer_pool;
pub mod simd;
pub mod zero_copy;

pub use aligned_alloc::{AlignedAllocator, aligned_allocator};
pub use buffer_pool::{
    BufferPool, BufferSize, PoolConfig, PooledBuffer, SimdType, global_buffer_pool,
};
pub use zero_copy::{IncrementalParser, LazyJsonValue, LazyParser, MemoryUsage, ZeroCopyParser};

/// JSON value types for initial classification
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ValueType {
    /// JSON object.
    Object,
    /// JSON array.
    Array,
    /// JSON string.
    String,
    /// JSON number (integer or float).
    Number,
    /// JSON boolean.
    Boolean,
    /// JSON null.
    Null,
}
