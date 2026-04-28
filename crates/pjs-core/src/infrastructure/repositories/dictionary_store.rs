//! In-memory [`DictionaryStore`] implementation with race-free per-session training.
//!
//! Available only when `feature = "compression"` is enabled and the target is
//! not `wasm32`.

use std::sync::Arc;

use dashmap::DashMap;
use pjson_rs_domain::value_objects::SessionId;
use tokio::sync::{Mutex, OnceCell};

use crate::{
    Error, Result,
    compression::zstd::{MAX_DICT_SIZE, N_TRAIN, ZstdDictCompressor, ZstdDictionary},
    domain::ports::dictionary_store::{DictionaryFuture, DictionaryStore},
    security::CompressionBombDetector,
};

/// Per-session state for corpus accumulation and one-time training.
struct SessionDictState {
    /// Training corpus. Capped at `N_TRAIN` entries; the mutex is held only
    /// during the push and snapshot — it is never held across `spawn_blocking`.
    corpus: Mutex<Vec<Vec<u8>>>,
    /// Training result. `OnceCell::get_or_try_init` guarantees that the closure
    /// runs at most once even when many tasks cross the threshold concurrently.
    dict: OnceCell<Arc<ZstdDictionary>>,
}

/// In-memory [`DictionaryStore`] that accumulates training samples per session
/// and fires a one-time background training task when the corpus is full.
///
/// Use [`InMemoryDictionaryStore::new`] and supply it to
/// `PjsAppState::with_dictionary_store(...)` to enable the dictionary endpoint.
///
/// # Session lifecycle
///
/// State grows unbounded until the process restarts. Eviction (`remove`) is
/// deferred to a follow-up (see TODO).
///
/// # Concurrency
///
/// - A [`DashMap`] provides lock-free shard-level access for per-session state lookup.
/// - `OnceCell::get_or_try_init` serialises training: only one closure runs to
///   completion regardless of how many tasks cross the `N_TRAIN` threshold.
/// - `spawn_blocking` offloads CPU-bound libzstd work off the Tokio runtime thread pool.
///
/// # Examples
///
/// ```rust
/// # #[cfg(all(feature = "compression", not(target_arch = "wasm32")))]
/// # {
/// use std::sync::Arc;
/// use pjson_rs::infrastructure::repositories::InMemoryDictionaryStore;
/// use pjson_rs::security::CompressionBombDetector;
///
/// let store = InMemoryDictionaryStore::new(
///     Arc::new(CompressionBombDetector::default()),
///     64 * 1024, // 64 KiB target dictionary size
/// );
/// # }
/// ```
pub struct InMemoryDictionaryStore {
    sessions: DashMap<SessionId, Arc<SessionDictState>>,
    bomb_detector: Arc<CompressionBombDetector>,
    /// Target dictionary size clamped to `MAX_DICT_SIZE` at construction.
    target_dict_size: usize,
}

impl InMemoryDictionaryStore {
    /// Create a new store.
    ///
    /// `target_dict_size` is **clamped** to [`MAX_DICT_SIZE`] (112 KiB). A good
    /// general default is 64 KiB — it covers most JSON schemas while keeping
    /// per-session RSS acceptable.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # #[cfg(all(feature = "compression", not(target_arch = "wasm32")))]
    /// # {
    /// use std::sync::Arc;
    /// use pjson_rs::infrastructure::repositories::InMemoryDictionaryStore;
    /// use pjson_rs::security::CompressionBombDetector;
    ///
    /// let store = InMemoryDictionaryStore::new(
    ///     Arc::new(CompressionBombDetector::default()),
    ///     64 * 1024,
    /// );
    /// # }
    /// ```
    pub fn new(bomb_detector: Arc<CompressionBombDetector>, target_dict_size: usize) -> Self {
        Self {
            sessions: DashMap::new(),
            bomb_detector,
            target_dict_size: target_dict_size.min(MAX_DICT_SIZE),
        }
    }

    /// Register a pre-trained dictionary for `session_id`.
    ///
    /// The bomb detector validates the dictionary's byte count against the
    /// configured `max_compressed_size` budget — the same gate used for
    /// compressed frame payloads. This reuse is intentional: the check is a
    /// "size budget" guard, not a semantic decompression check.
    ///
    /// **First-write-wins:** if a dictionary is already registered (or training
    /// already completed via [`DictionaryStore::train_if_ready`]), the call
    /// silently returns `Ok(())`. This avoids a TOCTOU race while keeping the
    /// API simple. Operators calling `register` twice will not learn that the
    /// second write was a no-op.
    ///
    /// # Errors
    ///
    /// Returns [`Error::CompressionError`] if the bomb detector rejects `dict.len()`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # #[cfg(all(feature = "compression", not(target_arch = "wasm32")))]
    /// # {
    /// use std::sync::Arc;
    /// use pjson_rs::infrastructure::repositories::{InMemoryDictionaryStore};
    /// use pjson_rs::compression::zstd::{ZstdDictCompressor, MAX_DICT_SIZE, N_TRAIN};
    /// use pjson_rs::security::CompressionBombDetector;
    /// use pjson_rs_domain::value_objects::SessionId;
    ///
    /// # tokio_test::block_on(async {
    /// let store = InMemoryDictionaryStore::new(
    ///     Arc::new(CompressionBombDetector::default()),
    ///     MAX_DICT_SIZE,
    /// );
    /// let samples: Vec<Vec<u8>> = (0..N_TRAIN)
    ///     .map(|i| format!("{{\"n\":{i}}}").into_bytes())
    ///     .collect();
    /// let dict = ZstdDictCompressor::train(&samples, MAX_DICT_SIZE).unwrap();
    /// let sid = SessionId::new();
    /// store.register(sid, dict).unwrap();
    /// # });
    /// # }
    /// ```
    pub fn register(&self, session_id: SessionId, dict: ZstdDictionary) -> Result<()> {
        // Reuses `validate_pre_decompression` as a size-budget gate. The function
        // name refers to its primary call site (pre-decompression checks), but the
        // underlying logic — "reject if byte count exceeds the configured cap" — is
        // equally applicable to dictionary blobs.
        self.bomb_detector
            .validate_pre_decompression(dict.len())
            .map_err(|e| {
                Error::CompressionError(format!("dictionary rejected by bomb detector: {e}"))
            })?;

        let state = self
            .sessions
            .entry(session_id)
            .or_insert_with(|| {
                Arc::new(SessionDictState {
                    corpus: Mutex::new(Vec::new()),
                    dict: OnceCell::new(),
                })
            })
            .clone();

        // First-write-wins: silently ignore if already set.
        let _ = state.dict.set(Arc::new(dict));
        Ok(())
    }

    /// Return or initialise the per-session state entry.
    fn session_state(&self, session_id: SessionId) -> Arc<SessionDictState> {
        self.sessions
            .entry(session_id)
            .or_insert_with(|| {
                Arc::new(SessionDictState {
                    corpus: Mutex::new(Vec::new()),
                    dict: OnceCell::new(),
                })
            })
            .clone()
    }
}

impl DictionaryStore for InMemoryDictionaryStore {
    fn get_dictionary<'a>(
        &'a self,
        session_id: SessionId,
    ) -> DictionaryFuture<'a, Option<Arc<ZstdDictionary>>> {
        Box::pin(async move {
            Ok(self
                .sessions
                .get(&session_id)
                .and_then(|s| s.dict.get().cloned()))
        })
    }

    fn train_if_ready<'a>(
        &'a self,
        session_id: SessionId,
        sample: Vec<u8>,
    ) -> DictionaryFuture<'a, ()> {
        Box::pin(async move {
            // TODO(#144 follow-up): per-sample size cap to bound corpus RSS.
            let state = self.session_state(session_id);

            // Fast path: dictionary already trained (no lock acquisition needed).
            if state.dict.initialized() {
                return Ok(());
            }

            // Append sample and snapshot when threshold reached.
            // The mutex is released before any async/blocking work.
            let snapshot = {
                let mut guard = state.corpus.lock().await;
                if guard.len() < N_TRAIN {
                    guard.push(sample);
                }
                if guard.len() < N_TRAIN {
                    return Ok(());
                }
                guard.clone()
            };

            let target = self.target_dict_size;
            let bomb_detector = self.bomb_detector.clone();

            // `get_or_try_init` runs the closure at most once even when many tasks
            // cross the threshold concurrently. On failure (transient libzstd error)
            // the cell is NOT poisoned — the next crossing will retry.
            let _ = state
                .dict
                .get_or_try_init(|| async move {
                    let dict = tokio::task::spawn_blocking(move || {
                        ZstdDictCompressor::train(&snapshot, target)
                    })
                    .await
                    .map_err(|e| {
                        Error::CompressionError(format!("zstd: train join error: {e}"))
                    })??;

                    // Symmetric bomb-detector check after training, mirroring `register()`.
                    // Catches the case where CompressionBombConfig is tuned tighter than
                    // MAX_DICT_SIZE — the trained dict could legitimately exceed the
                    // deployment's configured budget even though the type invariant holds.
                    bomb_detector
                        .validate_pre_decompression(dict.len())
                        .map_err(|e| {
                            Error::CompressionError(format!(
                                "trained dict rejected by bomb detector: {e}"
                            ))
                        })?;

                    Ok::<_, Error>(Arc::new(dict))
                })
                .await?;

            Ok(())
        })
    }
}

// TODO(#144 follow-up): persistent backend (sled/sqlite) when in-memory store proves insufficient.

#[cfg(test)]
mod tests {
    use super::*;
    use pjson_rs_domain::value_objects::SessionId;

    fn make_store() -> InMemoryDictionaryStore {
        InMemoryDictionaryStore::new(Arc::new(CompressionBombDetector::default()), 64 * 1024)
    }

    fn make_samples(count: usize) -> Vec<Vec<u8>> {
        (0..count)
            .map(|i| format!(r#"{{"id":{i},"name":"item","value":{}}}"#, i * 10).into_bytes())
            .collect()
    }

    #[tokio::test]
    async fn test_get_dictionary_returns_none_before_training() {
        let store = make_store();
        let sid = SessionId::new();
        let result = store.get_dictionary(sid).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_train_if_ready_below_threshold_stays_none() {
        let store = make_store();
        let sid = SessionId::new();

        for i in 0..(N_TRAIN - 1) {
            let sample = format!(r#"{{"i":{i}}}"#).into_bytes();
            store.train_if_ready(sid, sample).await.unwrap();
        }

        let result = store.get_dictionary(sid).await.unwrap();
        assert!(
            result.is_none(),
            "should still be None before N_TRAIN samples"
        );
    }

    #[tokio::test]
    async fn test_train_if_ready_fires_after_threshold() {
        let store = make_store();
        let sid = SessionId::new();
        let samples = make_samples(N_TRAIN);

        for sample in samples {
            store.train_if_ready(sid, sample).await.unwrap();
        }

        let result = store.get_dictionary(sid).await.unwrap();
        assert!(
            result.is_some(),
            "dictionary should be Some after N_TRAIN samples"
        );
    }

    #[tokio::test]
    async fn test_register_then_get_returns_dict() {
        let store = make_store();
        let sid = SessionId::new();
        let samples = make_samples(N_TRAIN);
        let dict = ZstdDictCompressor::train(&samples, MAX_DICT_SIZE).unwrap();

        store.register(sid, dict).unwrap();

        let result = store.get_dictionary(sid).await.unwrap();
        assert!(result.is_some());
    }

    #[tokio::test]
    async fn test_concurrent_train_if_ready_produces_exactly_one_dict() {
        use futures::future::try_join_all;

        let store = Arc::new(make_store());
        let sid = SessionId::new();
        let samples = make_samples(N_TRAIN * 2); // more than enough

        let futs: Vec<_> = samples
            .into_iter()
            .map(|sample| {
                let store = store.clone();
                tokio::spawn(async move { store.train_if_ready(sid, sample).await })
            })
            .collect();

        // All tasks must complete without panicking or erroring.
        let results = try_join_all(futs).await.unwrap();
        for r in results {
            r.unwrap();
        }

        let result = store.get_dictionary(sid).await.unwrap();
        assert!(result.is_some(), "exactly one dictionary should be trained");
    }

    #[tokio::test]
    async fn test_train_if_ready_bomb_detector_rejects_trained_dict() {
        use crate::security::CompressionBombConfig;

        // A budget so tight that any real trained dictionary will exceed it.
        let config = CompressionBombConfig {
            max_compressed_size: 100,
            ..Default::default()
        };
        let store = InMemoryDictionaryStore::new(
            Arc::new(CompressionBombDetector::new(config)),
            MAX_DICT_SIZE,
        );
        let sid = SessionId::new();
        let samples = make_samples(N_TRAIN);

        // Feed all samples. The call that crosses the N_TRAIN threshold triggers
        // training and then runs the bomb-detector check; that check fails, so
        // get_or_try_init propagates the error back through train_if_ready via `?`.
        // All preceding calls (below the threshold) return Ok(()).
        let mut training_error_seen = false;
        for sample in samples {
            let result = store.train_if_ready(sid, sample).await;
            if result.is_err() {
                training_error_seen = true;
                // Only the threshold-crossing call should fail.
                break;
            }
        }
        assert!(
            training_error_seen,
            "expected bomb detector to reject the trained dict"
        );

        // The dictionary must not be accessible because the bomb detector rejected it.
        let result = store.get_dictionary(sid).await.unwrap();
        assert!(
            result.is_none(),
            "bomb detector should have prevented dict from being stored"
        );
    }

    #[test]
    fn test_register_rejects_oversized_dict_via_bomb_detector() {
        use crate::security::CompressionBombConfig;

        let config = CompressionBombConfig {
            max_compressed_size: 10, // tinier than any real dict
            ..Default::default()
        };
        let store = InMemoryDictionaryStore::new(
            Arc::new(CompressionBombDetector::new(config)),
            MAX_DICT_SIZE,
        );
        let sid = SessionId::new();
        let samples = make_samples(N_TRAIN);
        let dict = ZstdDictCompressor::train(&samples, MAX_DICT_SIZE).unwrap();

        let result = store.register(sid, dict);
        assert!(result.is_err(), "bomb detector must reject oversized dict");
    }
}
