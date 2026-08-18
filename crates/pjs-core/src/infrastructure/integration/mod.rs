//! Infrastructure integration utilities.
//!
//! Object pooling and SIMD-accelerated serialization helpers. Not currently
//! exercised by any production code path — their only caller was the
//! `StreamingAdapter`/`UniversalAdapter` layer removed in #487; evaluating
//! whether to remove these too is tracked in a follow-up issue.

/// Object pooling utilities.
pub mod object_pool;
/// SIMD-accelerated frame processing helpers.
pub mod simd_acceleration;
