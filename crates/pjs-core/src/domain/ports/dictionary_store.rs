//! Port: resolve trained zstd dictionaries by session id.
//!
//! Intentionally **not** under `ports::gat` — the GAT pattern is reserved for
//! hot-path ports where allocation cost matters. This port is invoked at most
//! once per HTTP dictionary request and once per sample submission. A boxed
//! future avoids pulling in `async-trait` as a dependency while staying
//! compatible with the project's nightly-first ethos.

use std::future::Future;
use std::pin::Pin;
#[cfg(all(feature = "compression", not(target_arch = "wasm32")))]
use std::sync::Arc;

#[cfg(all(feature = "compression", not(target_arch = "wasm32")))]
use pjson_rs_domain::value_objects::SessionId;

#[cfg(all(feature = "compression", not(target_arch = "wasm32")))]
use crate::compression::zstd::ZstdDictionary;

use crate::Result;

/// Heap-allocated future returned by [`DictionaryStore`] methods.
///
/// Using `Pin<Box<dyn Future + Send + '_>>` instead of `async_trait` keeps the
/// dependency tree clean. The lifetime `'a` ties the future to the store borrow.
pub type DictionaryFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;

/// Port for resolving and accumulating per-session zstd dictionaries.
///
/// Implementations must be `Send + Sync` so they can be shared behind
/// `Arc<dyn DictionaryStore>` across Tokio tasks.
///
/// The trait methods are gated on `#[cfg(all(feature = "compression", not(target_arch = "wasm32")))]`
/// so the trait itself compiles when the feature is disabled (the `dictionary_store`
/// field on `PjsAppState` still exists, it just exposes no callable methods).
///
/// # Contract
///
/// - `get_dictionary` returns `Ok(None)` until training completes.
/// - `train_if_ready` accumulates samples and fires training exactly once when
///   the configured threshold is reached. Subsequent calls are no-ops.
pub trait DictionaryStore: Send + Sync {
    /// Returns the trained dictionary for `session_id`.
    ///
    /// Returns `Ok(None)` when training has not yet completed (fewer than the
    /// configured number of samples or the session is unknown).
    ///
    /// # Examples
    ///
    /// ```rust
    /// # #[cfg(all(feature = "compression", not(target_arch = "wasm32")))]
    /// # {
    /// use std::sync::Arc;
    /// use pjson_rs::domain::ports::dictionary_store::{DictionaryStore, NoopDictionaryStore};
    /// use pjson_rs_domain::value_objects::SessionId;
    ///
    /// # tokio_test::block_on(async {
    /// let store = NoopDictionaryStore;
    /// let sid = SessionId::new();
    /// let result = store.get_dictionary(sid).await.unwrap();
    /// assert!(result.is_none(), "Noop store always returns None");
    /// # });
    /// # }
    /// ```
    #[cfg(all(feature = "compression", not(target_arch = "wasm32")))]
    fn get_dictionary<'a>(
        &'a self,
        session_id: SessionId,
    ) -> DictionaryFuture<'a, Option<Arc<ZstdDictionary>>>;

    /// Append `sample` to the training corpus for `session_id`.
    ///
    /// When the corpus reaches the configured threshold, training fires exactly
    /// once (race-free). Subsequent calls after training are no-ops.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # #[cfg(all(feature = "compression", not(target_arch = "wasm32")))]
    /// # {
    /// use std::sync::Arc;
    /// use pjson_rs::domain::ports::dictionary_store::{DictionaryStore, NoopDictionaryStore};
    /// use pjson_rs_domain::value_objects::SessionId;
    ///
    /// # tokio_test::block_on(async {
    /// let store = NoopDictionaryStore;
    /// let sid = SessionId::new();
    /// // No-op: always succeeds without training.
    /// store.train_if_ready(sid, b"sample".to_vec()).await.unwrap();
    /// # });
    /// # }
    /// ```
    #[cfg(all(feature = "compression", not(target_arch = "wasm32")))]
    fn train_if_ready<'a>(
        &'a self,
        session_id: SessionId,
        sample: Vec<u8>,
    ) -> DictionaryFuture<'a, ()>;

    // TODO(#144 follow-up): add remove(SessionId) for session-end eviction.
}

/// No-op [`DictionaryStore`] that always returns `Ok(None)`.
///
/// Used by `PjsAppState::new` so the existing three-argument constructor stays
/// wire-compatible. Callers that need the dictionary endpoint must upgrade to
/// `PjsAppState::with_dictionary_store(...)` with a concrete implementation.
///
/// # Examples
///
/// ```rust
/// use pjson_rs::domain::ports::dictionary_store::NoopDictionaryStore;
/// let store = NoopDictionaryStore;
/// // NoopDictionaryStore is Send + Sync, so it can be wrapped in Arc.
/// let _: std::sync::Arc<dyn pjson_rs::domain::ports::dictionary_store::DictionaryStore> =
///     std::sync::Arc::new(store);
/// ```
#[derive(Debug, Default, Clone)]
pub struct NoopDictionaryStore;

impl DictionaryStore for NoopDictionaryStore {
    #[cfg(all(feature = "compression", not(target_arch = "wasm32")))]
    fn get_dictionary<'a>(
        &'a self,
        _: SessionId,
    ) -> DictionaryFuture<'a, Option<Arc<ZstdDictionary>>> {
        Box::pin(async { Ok(None) })
    }

    #[cfg(all(feature = "compression", not(target_arch = "wasm32")))]
    fn train_if_ready<'a>(&'a self, _: SessionId, _: Vec<u8>) -> DictionaryFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(all(feature = "compression", not(target_arch = "wasm32")))]
    #[tokio::test]
    async fn test_noop_get_dictionary_returns_none() {
        let store = NoopDictionaryStore;
        let sid = SessionId::new();
        let result = store.get_dictionary(sid).await.unwrap();
        assert!(result.is_none());
    }

    #[cfg(all(feature = "compression", not(target_arch = "wasm32")))]
    #[tokio::test]
    async fn test_noop_train_if_ready_is_ok() {
        let store = NoopDictionaryStore;
        let sid = SessionId::new();
        let result = store.train_if_ready(sid, b"sample data".to_vec()).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_noop_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<NoopDictionaryStore>();
    }
}
