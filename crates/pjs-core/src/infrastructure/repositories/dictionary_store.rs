//! In-memory [`DictionaryStore`] implementation with race-free per-session training.
//!
//! Available only when `feature = "compression"` is enabled and the target is
//! not `wasm32`.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use pjson_rs_domain::value_objects::SessionId;
use tokio::sync::{Mutex, OnceCell};

use crate::{
    Error, Result,
    compression::zstd::{MAX_DICT_SIZE, N_TRAIN, ZstdDictCompressor, ZstdDictionary},
    domain::ports::dictionary_store::{DictionaryFuture, DictionaryStore},
    security::CompressionBombDetector,
};

/// Maximum size of a single training sample fed into a session's corpus.
///
/// Independent of `JsonLimits::max_input_size` (100 MiB default), which
/// bounds a whole request body — this bounds one dictionary-training sample
/// (a single frame payload). Training samples are meant to be representative
/// small JSON snippets; capping them at 1 MiB bounds the worst-case
/// per-session corpus at `N_TRAIN * MAX_TRAINING_SAMPLE_SIZE` (32 MiB) instead
/// of `N_TRAIN * max_input_size` (3.2 GiB). Oversized samples are skipped
/// rather than rejecting the whole request — training remains best-effort.
const MAX_TRAINING_SAMPLE_SIZE: usize = 1024 * 1024;

/// Process-wide cap on the total bytes reserved for per-session corpora that
/// have not yet finished training — either still accumulating below
/// `N_TRAIN`, or already snapshotted and inside `spawn_blocking` training —
/// tracked by [`InMemoryDictionaryStore::corpus_bytes_in_flight`].
///
/// [`MAX_TRAINING_SAMPLE_SIZE`] alone bounds a single session's corpus to
/// `N_TRAIN * MAX_TRAINING_SAMPLE_SIZE` (32 MiB) — but a client can pin close
/// to that entire amount indefinitely (until [`SESSION_TTL`] evicts it)
/// simply by sending `N_TRAIN - 1` samples and never crossing the training
/// threshold, and nothing bounds how many *distinct* sessions do this
/// concurrently. This budget bounds the sum across all sessions, so no
/// number of concurrently open sessions can exceed it: once the budget is
/// exhausted, new samples across every session are skipped (not admitted)
/// until a session's reservation is released — which happens only once its
/// training has actually *finished* (success or failure), not merely been
/// snapshotted, so a session mid-`spawn_blocking` still counts against the
/// budget for the entire duration of that call — or TTL eviction frees it
/// early. 128 MiB allows a handful of sessions' worth of full pending or
/// in-flight corpora at once while keeping the worst case a small,
/// predictable fraction of typical container memory.
const TOTAL_CORPUS_BYTE_BUDGET: usize = 128 * 1024 * 1024;

/// How long a session's dictionary-training state may sit idle before the
/// background task in [`InMemoryDictionaryStore::new`] evicts it.
const SESSION_TTL: Duration = Duration::from_secs(30 * 60);

/// Interval between periodic prunes of expired session state, mirroring the
/// cadence `WebSocketRateLimiter`'s background cleanup uses.
const SESSION_CLEANUP_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// Per-session state for corpus accumulation and one-time training.
struct SessionDictState {
    /// Training corpus. Capped at `N_TRAIN` entries; the mutex is held only
    /// during the push and snapshot — it is never held across `spawn_blocking`.
    corpus: Mutex<Vec<Vec<u8>>>,
    /// Training result. `OnceCell::get_or_try_init` guarantees that the closure
    /// runs at most once even when many tasks cross the threshold concurrently.
    dict: OnceCell<Arc<ZstdDictionary>>,
    /// Monotonic timestamp (milliseconds since process start, see
    /// [`now_millis`]) of the last access — bumped by both writes
    /// (`session_state`) and reads (`get_dictionary`) so an
    /// actively-served-but-not-retrained session is not evicted out from
    /// under its readers. Used by the TTL eviction task.
    last_access: AtomicU64,
    /// Bytes currently reserved against [`InMemoryDictionaryStore::corpus_bytes_in_flight`]
    /// for this session's not-yet-trained corpus. Released back to the
    /// global counter once the corpus is snapshotted for training (success
    /// or failure) or the session is evicted while samples are still pending.
    pending_corpus_bytes: AtomicUsize,
}

impl SessionDictState {
    fn new() -> Self {
        Self {
            corpus: Mutex::new(Vec::new()),
            dict: OnceCell::new(),
            last_access: AtomicU64::new(now_millis()),
            pending_corpus_bytes: AtomicUsize::new(0),
        }
    }
}

/// Release `amount` bytes from a global corpus-budget counter, saturating at
/// zero rather than wrapping.
///
/// A plain `fetch_sub` underflows to ~`usize::MAX` if `amount` ever exceeds
/// the counter's current value — reachable because the counter can be
/// concurrently resynchronized to a smaller value by [`evict_expired_sessions`]
/// (a session's `pending_corpus_bytes` can outlive the global counter briefly
/// during that resync). An underflowed counter makes every subsequent
/// admission check in `InMemoryDictionaryStore::train_if_ready` see the
/// budget as exhausted, silently rejecting all training samples
/// process-wide until the next sweep happens to correct it.
fn release_corpus_bytes(corpus_bytes_in_flight: &AtomicUsize, amount: usize) {
    if amount == 0 {
        return;
    }
    // A manual CAS loop rather than `AtomicUsize::update`/`try_update`
    // (stable since Rust 1.95): the workspace MSRV is 1.89.
    let mut current = corpus_bytes_in_flight.load(Ordering::Relaxed);
    loop {
        let new = current.saturating_sub(amount);
        match corpus_bytes_in_flight.compare_exchange_weak(
            current,
            new,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return,
            Err(observed) => current = observed,
        }
    }
}

/// RAII guard releasing a fixed, pre-captured byte amount back to the
/// global `corpus_bytes_in_flight` counter on drop.
///
/// Exists so the release runs even if the future holding it is dropped
/// before training completes — a client disconnecting mid-request, or a
/// timeout/`select!` cancelling the call, would otherwise skip a release
/// placed only after the training `.await`. Without this, a client can
/// repeat that cancellation to drive one session's reservation up toward
/// the entire global budget on its own.
///
/// Holds a plain `usize` captured under the same `corpus` mutex hold as the
/// snapshot, **not** `state.pending_corpus_bytes` read again at drop time.
/// The corpus mutex is released before this guard is constructed, so a
/// concurrent `train_if_ready` call on the same session — still possible
/// during the (potentially seconds-long) training window, since `dict` is
/// not yet initialized — can push fresh samples into the now-empty corpus
/// and grow `pending_corpus_bytes` again in the meantime. A guard that
/// re-read and `swap(0)`'d that counter at drop time would sweep up those
/// unrelated, newer reservations too: freeing budget for corpus bytes this
/// guard never accounted for, while those samples themselves are never
/// consumed (the `dict.initialized()` fast path returns without touching
/// the corpus again) or bounded — an unbounded, budget-invisible leak per
/// session. Capturing the exact reserved amount up front and releasing
/// exactly that fixes both: no more, no less than what this guard reserved.
struct CorpusBudgetReservation {
    reserved: usize,
    corpus_bytes_in_flight: Arc<AtomicUsize>,
}

impl Drop for CorpusBudgetReservation {
    fn drop(&mut self) {
        release_corpus_bytes(&self.corpus_bytes_in_flight, self.reserved);
    }
}

/// Monotonic milliseconds since first call, used for `last_access` bookkeeping.
///
/// Deliberately not wall-clock (`SystemTime`): an NTP step forward would
/// otherwise evict live sessions early, and a step backward would pin them
/// forever. Millisecond (not second) granularity avoids a footgun where a
/// sub-second `ttl` passed to [`InMemoryDictionaryStore::cleanup_expired`]
/// would silently truncate to "evict everything."
fn now_millis() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    let start = *START.get_or_init(Instant::now);
    start.elapsed().as_millis() as u64
}

/// Evict entries whose `last_access` is older than `ttl`, then resynchronize
/// `corpus_bytes_in_flight` to the live map's actual total.
///
/// A pure incremental release (subtract each evicted entry's bytes) is
/// vulnerable to a narrow TOCTOU race: a caller can hold an `Arc<SessionDictState>`
/// obtained from `session_state()` whose map entry this same sweep evicts
/// *before* that caller reserves and pushes its sample — the reservation then
/// lands in a `pending_corpus_bytes` counter no future sweep will ever visit
/// (the state is no longer reachable via `sessions`), permanently leaking
/// that reservation from the global budget. Recomputing the authoritative
/// total from what is actually still in the map after eviction self-heals
/// any such drift every sweep, bounding its lifetime to at most one
/// `SESSION_CLEANUP_INTERVAL` instead of forever. This is a best-effort
/// resync, not a hard invariant, and it is only ever *permissive*: a
/// session currently mid-training has already had its `pending_corpus_bytes`
/// zeroed out (moved into a live `CorpusBudgetReservation` guard — see
/// `train_if_ready`), so summing the map excludes that session's in-flight
/// reservation entirely, undercounting the true in-flight total by up to
/// one training snapshot's worth (`N_TRAIN * MAX_TRAINING_SAMPLE_SIZE`, 32
/// MiB) per session concurrently training when a sweep runs. This can never
/// cause a false "budget exhausted" rejection — only make the budget briefly
/// look more available than it truly is until the reservation's own guard
/// releases it on training completion.
fn evict_expired_sessions(
    sessions: &DashMap<SessionId, Arc<SessionDictState>>,
    ttl: Duration,
    corpus_bytes_in_flight: &AtomicUsize,
) {
    let now = now_millis();
    let ttl_millis = ttl.as_millis() as u64;
    sessions.retain(|_, state| {
        now.saturating_sub(state.last_access.load(Ordering::Relaxed)) < ttl_millis
    });

    let live_total: usize = sessions
        .iter()
        .map(|entry| entry.value().pending_corpus_bytes.load(Ordering::Relaxed))
        .sum();
    corpus_bytes_in_flight.store(live_total, Ordering::Relaxed);
}

/// Spawn a background task that periodically prunes idle session state.
///
/// Without this, every distinct session accumulates a permanent entry for
/// the process lifetime (see issue #329). Requires a Tokio runtime; if none
/// is available, logs a warning and returns without spawning rather than
/// panicking, since bare construction of the store must remain usable from
/// non-async contexts. The task holds only `Weak` references and exits on
/// its own once every strong reference to the store's state is dropped.
fn spawn_session_cleanup_task(
    sessions: &Arc<DashMap<SessionId, Arc<SessionDictState>>>,
    corpus_bytes_in_flight: &Arc<AtomicUsize>,
) {
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        tracing::warn!(
            "InMemoryDictionaryStore: no Tokio runtime available; periodic session eviction \
             not started"
        );
        return;
    };

    let sessions_weak = Arc::downgrade(sessions);
    let bytes_weak = Arc::downgrade(corpus_bytes_in_flight);
    handle.spawn(async move {
        let mut interval = tokio::time::interval(SESSION_CLEANUP_INTERVAL);
        loop {
            interval.tick().await;
            let (Some(sessions), Some(bytes)) = (sessions_weak.upgrade(), bytes_weak.upgrade())
            else {
                break;
            };
            evict_expired_sessions(&sessions, SESSION_TTL, &bytes);
            tracing::debug!("InMemoryDictionaryStore: session cleanup pass completed");
        }
    });
}

/// In-memory [`DictionaryStore`] that accumulates training samples per session
/// and fires a one-time background training task when the corpus is full.
///
/// Use [`InMemoryDictionaryStore::new`] and supply it to
/// `PjsAppState::with_dictionary_store(...)` to enable the dictionary endpoint.
///
/// # Session lifecycle
///
/// If called from within a Tokio runtime, [`InMemoryDictionaryStore::new`]
/// spawns a background task that evicts session state idle for longer than
/// `SESSION_TTL` every `SESSION_CLEANUP_INTERVAL` — "idle" accounts for both
/// writes (training samples, registration) and reads (`get_dictionary`), so
/// an actively-served session is never evicted out from under its readers.
/// Outside a Tokio runtime, construction still succeeds but periodic
/// eviction is skipped (logged as a warning); [`InMemoryDictionaryStore::cleanup_expired`]
/// remains available for manual pruning either way. Once a session's
/// dictionary finishes training, its corpus buffer is emptied immediately
/// since it is no longer needed. Independent of the per-session TTL, a
/// process-wide byte budget (`TOTAL_CORPUS_BYTE_BUDGET`) bounds the sum of
/// every session's *not-yet-trained* corpus at once, so no number of
/// concurrently open sessions can pin unbounded memory before any of them
/// reaches the training threshold.
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
/// // Background eviction only starts if a Tokio runtime is active; this
/// // works either way, it just skips the background task otherwise.
/// let store = InMemoryDictionaryStore::new(
///     Arc::new(CompressionBombDetector::default()),
///     64 * 1024, // 64 KiB target dictionary size
/// );
/// # let _ = store;
/// # }
/// ```
pub struct InMemoryDictionaryStore {
    sessions: Arc<DashMap<SessionId, Arc<SessionDictState>>>,
    bomb_detector: Arc<CompressionBombDetector>,
    /// Target dictionary size clamped to `MAX_DICT_SIZE` at construction.
    target_dict_size: usize,
    /// Sum of `pending_corpus_bytes` across every tracked session; bounded by
    /// [`TOTAL_CORPUS_BYTE_BUDGET`].
    corpus_bytes_in_flight: Arc<AtomicUsize>,
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
    /// // Background eviction only starts if a Tokio runtime is active; this
    /// // works either way, it just skips the background task otherwise.
    /// let store = InMemoryDictionaryStore::new(
    ///     Arc::new(CompressionBombDetector::default()),
    ///     64 * 1024,
    /// );
    /// # let _ = store;
    /// # }
    /// ```
    pub fn new(bomb_detector: Arc<CompressionBombDetector>, target_dict_size: usize) -> Self {
        let sessions = Arc::new(DashMap::new());
        let corpus_bytes_in_flight = Arc::new(AtomicUsize::new(0));
        spawn_session_cleanup_task(&sessions, &corpus_bytes_in_flight);

        Self {
            sessions,
            bomb_detector,
            target_dict_size: target_dict_size.min(MAX_DICT_SIZE),
            corpus_bytes_in_flight,
        }
    }

    /// Manually prune session state idle for longer than `ttl`.
    ///
    /// Normally invoked automatically every `SESSION_CLEANUP_INTERVAL` by
    /// the background task spawned in [`InMemoryDictionaryStore::new`];
    /// exposed for tests and callers that want tighter control.
    pub fn cleanup_expired(&self, ttl: Duration) {
        evict_expired_sessions(&self.sessions, ttl, &self.corpus_bytes_in_flight);
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

        let state = self.session_state(session_id);

        // First-write-wins: silently ignore if already set.
        let _ = state.dict.set(Arc::new(dict));
        Ok(())
    }

    /// Return or initialise the per-session state entry, refreshing its
    /// `last_access` timestamp so the TTL eviction task keeps it alive.
    fn session_state(&self, session_id: SessionId) -> Arc<SessionDictState> {
        let state = self
            .sessions
            .entry(session_id)
            .or_insert_with(|| Arc::new(SessionDictState::new()))
            .clone();
        state.last_access.store(now_millis(), Ordering::Relaxed);
        state
    }
}

impl DictionaryStore for InMemoryDictionaryStore {
    fn get_dictionary<'a>(
        &'a self,
        session_id: SessionId,
    ) -> DictionaryFuture<'a, Option<Arc<ZstdDictionary>>> {
        Box::pin(async move {
            Ok(self.sessions.get(&session_id).and_then(|s| {
                let dict = s.dict.get().cloned();
                // A session actively serving an already-trained dictionary
                // must not be evicted out from under its readers — bump on
                // read too, not just on write via `session_state`. Bumping
                // unconditionally (including for a still-training session)
                // would let a client keep an *untrained* corpus's reserved
                // budget alive indefinitely just by polling this endpoint,
                // so only a real `Some` (an actual trained dictionary being
                // served) refreshes the timestamp — a `None` poll leaves it
                // alone and the session ages out normally.
                if dict.is_some() {
                    s.last_access.store(now_millis(), Ordering::Relaxed);
                }
                dict
            }))
        })
    }

    fn train_if_ready<'a>(
        &'a self,
        session_id: SessionId,
        sample: Vec<u8>,
    ) -> DictionaryFuture<'a, ()> {
        Box::pin(async move {
            let state = self.session_state(session_id);

            // Fast path: dictionary already trained (no lock acquisition needed).
            if state.dict.initialized() {
                return Ok(());
            }

            // Append sample and snapshot when threshold reached.
            // The mutex is released before any async/blocking work.
            // Oversized samples are skipped rather than erroring the whole
            // request — see MAX_TRAINING_SAMPLE_SIZE. Samples that would push
            // the process-wide pending-corpus budget over TOTAL_CORPUS_BYTE_BUDGET
            // are skipped the same way — this is what actually bounds worst-case
            // memory, since MAX_TRAINING_SAMPLE_SIZE alone only bounds one
            // session and nothing otherwise bounds how many sessions a client
            // can keep open below the N_TRAIN threshold at once.
            let (snapshot, reserved_bytes) = {
                let mut guard = state.corpus.lock().await;
                if guard.len() < N_TRAIN && sample.len() <= MAX_TRAINING_SAMPLE_SIZE {
                    let sample_len = sample.len();
                    let global_reserved = self
                        .corpus_bytes_in_flight
                        .fetch_add(sample_len, Ordering::Relaxed)
                        + sample_len;
                    if global_reserved <= TOTAL_CORPUS_BYTE_BUDGET {
                        state
                            .pending_corpus_bytes
                            .fetch_add(sample_len, Ordering::Relaxed);
                        guard.push(sample);
                    } else {
                        // Global budget exhausted: undo the reservation and skip.
                        release_corpus_bytes(&self.corpus_bytes_in_flight, sample_len);
                        tracing::debug!(
                            sample_len,
                            budget = TOTAL_CORPUS_BYTE_BUDGET,
                            "InMemoryDictionaryStore: skipping training sample, \
                             corpus byte budget exhausted"
                        );
                    }
                }
                if guard.len() < N_TRAIN {
                    return Ok(());
                }
                // Capture (and reset) the exact amount reserved for *this*
                // snapshot while still holding the corpus mutex — not later,
                // and not by re-reading `pending_corpus_bytes` at guard-drop
                // time. Training can take seconds; a concurrent
                // `train_if_ready` call on this same session (still possible
                // — `dict` isn't initialized yet) would otherwise be free to
                // push fresh samples into the corpus this mutex hold is
                // about to empty and grow `pending_corpus_bytes` again before
                // this guard drops, and a drop-time re-read would sweep up
                // and release those unrelated, newer reservations too — see
                // `CorpusBudgetReservation`'s doc comment.
                let reserved_bytes = state.pending_corpus_bytes.swap(0, Ordering::Relaxed);
                // Take rather than clone: halves peak RSS at the training
                // threshold crossing, and resets the corpus to empty so a
                // failed training attempt (bomb detector rejects, transient
                // zstd error) starts fresh from N new samples next time
                // instead of retrying the same stale snapshot on every
                // subsequent call.
                (std::mem::take(&mut *guard), reserved_bytes)
            };

            // Release this session's reservation once training has actually
            // *finished* (success, failure, or this future being dropped
            // before either) — not merely been snapshotted. The snapshot
            // bytes are still live, moved into the spawn_blocking closure
            // below, for the entire duration of that call; releasing at
            // snapshot time would let the reservation counter undercount
            // real memory while spawn_blocking's (unbounded) queue holds
            // multiple sessions' worth of snapshots concurrently, defeating
            // the budget's purpose. An RAII guard rather than a manual
            // release after the `.await` below: a manual release would never
            // run if this future is dropped while suspended there (client
            // disconnect, a timeout/`select!` cancelling the call), letting
            // the reservation leak indefinitely — `Drop` runs in that case too.
            let _reservation_guard = CorpusBudgetReservation {
                reserved: reserved_bytes,
                corpus_bytes_in_flight: self.corpus_bytes_in_flight.clone(),
            };

            let target = self.target_dict_size;
            let bomb_detector = self.bomb_detector.clone();

            // `get_or_try_init` runs the closure at most once even when many tasks
            // cross the threshold concurrently. On failure (transient libzstd error)
            // the cell is NOT poisoned — the next crossing will retry.
            state
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

            // The corpus was already emptied by `mem::take` above when it was
            // snapshotted for training, so there is nothing left to clear here.
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

    #[tokio::test]
    async fn test_register_rejects_oversized_dict_via_bomb_detector() {
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

    #[tokio::test]
    async fn test_cleanup_expired_evicts_idle_sessions() {
        let store = make_store();
        let sid = SessionId::new();

        // Below N_TRAIN so the session state stays alive without training.
        store.train_if_ready(sid, b"sample".to_vec()).await.unwrap();
        assert_eq!(store.sessions.len(), 1);

        // A TTL of zero means "idle at all" is expired.
        store.cleanup_expired(Duration::from_secs(0));

        assert_eq!(
            store.sessions.len(),
            0,
            "session idle past the TTL should be evicted"
        );
    }

    #[tokio::test]
    async fn test_cleanup_expired_preserves_fresh_sessions() {
        let store = make_store();
        let sid = SessionId::new();

        store.train_if_ready(sid, b"sample".to_vec()).await.unwrap();

        store.cleanup_expired(Duration::from_secs(3600));

        assert_eq!(
            store.sessions.len(),
            1,
            "session accessed within the TTL window must survive cleanup"
        );
    }

    #[tokio::test]
    async fn test_oversized_sample_is_skipped_not_rejected() {
        let store = make_store();
        let sid = SessionId::new();

        let oversized = vec![0u8; MAX_TRAINING_SAMPLE_SIZE + 1];
        let result = store.train_if_ready(sid, oversized).await;
        assert!(
            result.is_ok(),
            "oversized sample must be skipped, not error the request"
        );

        // The sample was skipped entirely, so the corpus is still empty; feed
        // exactly N_TRAIN normal-sized samples now and confirm training still
        // completes (proving the oversized sample never occupied a slot).
        for sample in make_samples(N_TRAIN) {
            store.train_if_ready(sid, sample).await.unwrap();
        }

        let result = store.get_dictionary(sid).await.unwrap();
        assert!(
            result.is_some(),
            "training should succeed once N_TRAIN valid samples arrive"
        );
    }

    #[tokio::test]
    async fn test_corpus_cleared_after_training_completes() {
        let store = make_store();
        let sid = SessionId::new();

        for sample in make_samples(N_TRAIN) {
            store.train_if_ready(sid, sample).await.unwrap();
        }

        let state = store.sessions.get(&sid).unwrap().value().clone();
        assert!(state.dict.initialized());
        assert!(
            state.corpus.lock().await.is_empty(),
            "corpus must be cleared once the dictionary is trained"
        );
    }

    #[tokio::test]
    async fn test_sample_at_exact_cap_is_accepted() {
        let store = make_store();
        let sid = SessionId::new();

        let exact_cap_sample = vec![0u8; MAX_TRAINING_SAMPLE_SIZE];
        store.train_if_ready(sid, exact_cap_sample).await.unwrap();

        // If the boundary sample had been (incorrectly) skipped, only
        // N_TRAIN - 1 total samples would ever have been admitted after
        // this loop and training would never fire.
        for sample in make_samples(N_TRAIN - 1) {
            store.train_if_ready(sid, sample).await.unwrap();
        }

        let result = store.get_dictionary(sid).await.unwrap();
        assert!(
            result.is_some(),
            "a sample of exactly MAX_TRAINING_SAMPLE_SIZE must be accepted, not skipped"
        );
    }

    #[tokio::test]
    async fn test_global_corpus_budget_rejects_samples_once_exhausted() {
        let store = make_store();
        // Simulate a near-exhausted budget without allocating hundreds of MB.
        store
            .corpus_bytes_in_flight
            .store(TOTAL_CORPUS_BYTE_BUDGET, Ordering::Relaxed);

        let sid = SessionId::new();
        for _ in 0..N_TRAIN {
            store.train_if_ready(sid, b"sample".to_vec()).await.unwrap();
        }

        // Every sample should have been skipped (budget already exhausted at
        // the very first push), so training never fires despite N_TRAIN calls.
        let result = store.get_dictionary(sid).await.unwrap();
        assert!(
            result.is_none(),
            "no sample should have been admitted once the global budget was exhausted"
        );
        assert_eq!(
            store.corpus_bytes_in_flight.load(Ordering::Relaxed),
            TOTAL_CORPUS_BYTE_BUDGET,
            "budget accounting must not grow past what was already reserved"
        );
    }

    #[tokio::test]
    async fn test_ttl_eviction_releases_pending_corpus_bytes() {
        let store = make_store();
        let sid = SessionId::new();
        // Below N_TRAIN, so the session stays alive with pending corpus bytes.
        store.train_if_ready(sid, vec![0u8; 1024]).await.unwrap();

        assert!(store.corpus_bytes_in_flight.load(Ordering::Relaxed) > 0);

        store.cleanup_expired(Duration::from_secs(0));

        assert_eq!(
            store.corpus_bytes_in_flight.load(Ordering::Relaxed),
            0,
            "evicting a session with pending samples must release its budget reservation"
        );
    }

    #[tokio::test]
    async fn test_training_completion_releases_pending_corpus_bytes() {
        let store = make_store();
        let sid = SessionId::new();

        for sample in make_samples(N_TRAIN) {
            store.train_if_ready(sid, sample).await.unwrap();
        }

        assert_eq!(
            store.corpus_bytes_in_flight.load(Ordering::Relaxed),
            0,
            "reservation must be released once the corpus is snapshotted for training"
        );
    }

    #[tokio::test]
    async fn test_get_dictionary_read_bumps_last_access() {
        let store = make_store();
        let sid = SessionId::new();

        for sample in make_samples(N_TRAIN) {
            store.train_if_ready(sid, sample).await.unwrap();
        }

        let state = store.sessions.get(&sid).unwrap().value().clone();
        state.last_access.store(0, Ordering::Relaxed);

        store.get_dictionary(sid).await.unwrap();

        assert!(
            state.last_access.load(Ordering::Relaxed) > 0,
            "get_dictionary must refresh last_access so an actively-served \
             session is not evicted out from under its readers"
        );
    }

    #[tokio::test]
    async fn test_get_dictionary_poll_on_untrained_session_does_not_bump_last_access() {
        let store = make_store();
        let sid = SessionId::new();

        // Below N_TRAIN: session exists, holds a pending reservation, but
        // has no trained dictionary yet.
        store.train_if_ready(sid, vec![0u8; 1024]).await.unwrap();

        let state = store.sessions.get(&sid).unwrap().value().clone();
        state.last_access.store(0, Ordering::Relaxed);

        let result = store.get_dictionary(sid).await.unwrap();
        assert!(result.is_none());

        assert_eq!(
            state.last_access.load(Ordering::Relaxed),
            0,
            "polling a still-training session must not refresh last_access — \
             otherwise a client could keep its pending budget reservation \
             alive forever just by polling"
        );
    }

    #[tokio::test]
    async fn test_training_failure_still_releases_pending_corpus_bytes() {
        use crate::security::CompressionBombConfig;

        // A budget so tight that any real trained dictionary is rejected —
        // guarantees the training closure returns Err.
        let config = CompressionBombConfig {
            max_compressed_size: 100,
            ..Default::default()
        };
        let store = InMemoryDictionaryStore::new(
            Arc::new(CompressionBombDetector::new(config)),
            MAX_DICT_SIZE,
        );
        let sid = SessionId::new();

        for sample in make_samples(N_TRAIN) {
            let _ = store.train_if_ready(sid, sample).await;
        }

        assert_eq!(
            store.corpus_bytes_in_flight.load(Ordering::Relaxed),
            0,
            "reservation must be released even when training itself fails, \
             not only on the success path"
        );
    }

    #[test]
    fn test_release_corpus_bytes_saturates_instead_of_wrapping() {
        let counter = AtomicUsize::new(5);

        // Releasing more than is currently tracked (e.g. because a
        // concurrent resync already reduced the counter) must clamp at
        // zero, not wrap around to near `usize::MAX`.
        release_corpus_bytes(&counter, 100);

        assert_eq!(counter.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_corpus_budget_reservation_releases_on_drop() {
        // Whitebox test of the RAII guard itself: this is what makes the
        // release cancellation-safe in `train_if_ready` — dropping the
        // guard (which happens automatically if the enclosing future is
        // dropped before training completes, e.g. a client disconnect or a
        // timeout/`select!` cancelling the call) must release exactly the
        // amount it was constructed with, and nothing more.
        let corpus_bytes_in_flight = Arc::new(AtomicUsize::new(500));

        let guard = CorpusBudgetReservation {
            reserved: 200,
            corpus_bytes_in_flight: corpus_bytes_in_flight.clone(),
        };

        // No explicit release call — simulates the guard's scope ending via
        // cancellation rather than normal completion.
        drop(guard);

        assert_eq!(corpus_bytes_in_flight.load(Ordering::Relaxed), 300);
    }

    #[test]
    fn test_guard_release_amount_is_independent_of_concurrent_pending_bytes_changes() {
        // Directly exercises the R3 scenario: after a guard is constructed
        // with a captured `reserved` amount, some *other* concurrent
        // activity pushes more bytes into the session's `pending_corpus_bytes`
        // (simulating a second `train_if_ready` call landing during the
        // training window — still possible, since `dict` isn't initialized
        // until training finishes). The guard must release only what it was
        // constructed with, leaving the newer, unrelated reservation intact.
        let corpus_bytes_in_flight = Arc::new(AtomicUsize::new(1000));
        let state = Arc::new(SessionDictState::new());

        let guard = CorpusBudgetReservation {
            reserved: 300,
            corpus_bytes_in_flight: corpus_bytes_in_flight.clone(),
        };

        // Simulates a concurrent train_if_ready call reserving more bytes
        // for the same session after this guard was already constructed.
        state.pending_corpus_bytes.fetch_add(150, Ordering::Relaxed);
        corpus_bytes_in_flight.fetch_add(150, Ordering::Relaxed); // now 1150

        drop(guard);

        assert_eq!(
            corpus_bytes_in_flight.load(Ordering::Relaxed),
            850, // 1150 - 300 (this guard's own reservation), not 1150 - 450
            "the guard must release only its own captured reservation, not \
             whatever the session's live pending_corpus_bytes happens to \
             hold at drop time"
        );
        assert_eq!(
            state.pending_corpus_bytes.load(Ordering::Relaxed),
            150,
            "the concurrent reservation must survive this guard's drop untouched"
        );
    }
}
