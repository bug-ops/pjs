//! Cooperative shutdown for [`serve_with_limits`](super::serve_with_limits)
//! and the WebSocket transport that shares its listener.
//!
//! A [`ShutdownSignal`] is triggered once by the embedding application. After
//! the trigger the server stops accepting, lets in-flight work finish for at
//! most a [`DrainTimeout`], then force-drops whatever is still running and
//! reports a [`ShutdownOutcome`].

use std::{future::Future, num::NonZeroUsize, sync::Arc, time::Duration};

use tokio::task::JoinHandle;
use tokio_util::{
    sync::CancellationToken,
    task::{TaskTracker, task_tracker::TaskTrackerToken},
};

use crate::config::ConfigError;

const DEFAULT_DRAIN_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_DRAIN_TIMEOUT: Duration = Duration::from_secs(600);
const FORCE_GRACE: Duration = Duration::from_millis(100);

/// Upper bound on how long the shutdown drain waits for in-flight work
/// before force-dropping it.
///
/// Valid values are `0 < d <= 600 s`; the default is 30 s.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use pjson_rs::infrastructure::http::DrainTimeout;
///
/// assert!(DrainTimeout::try_from(Duration::from_secs(5)).is_ok());
/// assert!(DrainTimeout::try_from(Duration::ZERO).is_err());
/// assert!(DrainTimeout::try_from(Duration::from_secs(601)).is_err());
/// assert_eq!(Duration::from(DrainTimeout::default()), Duration::from_secs(30));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DrainTimeout(Duration);

impl Default for DrainTimeout {
    fn default() -> Self {
        Self(DEFAULT_DRAIN_TIMEOUT)
    }
}

impl TryFrom<Duration> for DrainTimeout {
    type Error = ConfigError;

    /// # Errors
    ///
    /// [`ConfigError::MustBePositive`] for a zero duration and
    /// [`ConfigError::InconsistentBounds`] above 600 s.
    fn try_from(timeout: Duration) -> Result<Self, ConfigError> {
        if timeout.is_zero() {
            return Err(ConfigError::MustBePositive {
                section: "http.shutdown",
                field: "drain_timeout",
            });
        }
        if timeout > MAX_DRAIN_TIMEOUT {
            return Err(ConfigError::InconsistentBounds {
                section: "http.shutdown",
                message: "drain_timeout must not exceed 600 seconds",
            });
        }
        Ok(Self(timeout))
    }
}

impl From<DrainTimeout> for Duration {
    fn from(timeout: DrainTimeout) -> Self {
        timeout.0
    }
}

/// How a drain ended.
///
/// # Examples
///
/// ```
/// use std::num::NonZeroUsize;
/// use pjson_rs::infrastructure::http::ShutdownOutcome;
///
/// let outcome = ShutdownOutcome::Forced { aborted: NonZeroUsize::MIN };
/// assert_ne!(outcome, ShutdownOutcome::Drained);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownOutcome {
    /// All tracked work finished within the drain timeout.
    Drained,
    /// The drain timeout elapsed and the remaining work was dropped.
    Forced {
        /// Tracked tasks (connections, WebSocket loops, session tasks) still
        /// alive when the drain was forced.
        aborted: NonZeroUsize,
    },
}

#[derive(Debug)]
struct Inner {
    shutdown: CancellationToken,
    force: CancellationToken,
    tracker: TaskTracker,
}

/// A shareable, idempotent shutdown trigger with a bounded drain.
///
/// Clones share one state: triggering any clone is observed by all, and
/// dropping every clone never triggers. Pass clones of **one** signal to both
/// [`serve_with_limits`](super::serve_with_limits) and
/// `AxumWebSocketTransport::with_shutdown`; a different instance means the
/// drain does not wait for WebSocket tasks.
///
/// # Examples
///
/// ```
/// use pjson_rs::infrastructure::http::ShutdownSignal;
///
/// let signal = ShutdownSignal::new();
/// let observer = signal.clone();
/// assert!(!observer.is_triggered());
/// signal.trigger();
/// signal.trigger();
/// assert!(observer.is_triggered());
/// ```
#[derive(Debug, Clone)]
pub struct ShutdownSignal {
    inner: Arc<Inner>,
}

impl Default for ShutdownSignal {
    fn default() -> Self {
        Self::new()
    }
}

impl ShutdownSignal {
    /// Creates an untriggered signal.
    ///
    /// # Examples
    ///
    /// ```
    /// use pjson_rs::infrastructure::http::ShutdownSignal;
    ///
    /// assert!(!ShutdownSignal::new().is_triggered());
    /// ```
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                shutdown: CancellationToken::new(),
                force: CancellationToken::new(),
                tracker: TaskTracker::new(),
            }),
        }
    }

    /// Requests shutdown. Idempotent; observed by every clone.
    ///
    /// # Examples
    ///
    /// ```
    /// use pjson_rs::infrastructure::http::ShutdownSignal;
    ///
    /// let signal = ShutdownSignal::new();
    /// signal.trigger();
    /// assert!(signal.is_triggered());
    /// ```
    pub fn trigger(&self) {
        self.inner.shutdown.cancel();
    }

    /// Whether shutdown has been requested.
    ///
    /// # Examples
    ///
    /// ```
    /// use pjson_rs::infrastructure::http::ShutdownSignal;
    ///
    /// let signal = ShutdownSignal::new();
    /// assert!(!signal.is_triggered());
    /// ```
    pub fn is_triggered(&self) -> bool {
        self.inner.shutdown.is_cancelled()
    }

    /// Completes once shutdown has been requested; completes immediately if it
    /// already was.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[tokio::main(flavor = "current_thread")]
    /// # async fn main() {
    /// use pjson_rs::infrastructure::http::ShutdownSignal;
    ///
    /// let signal = ShutdownSignal::new();
    /// signal.trigger();
    /// signal.triggered().await;
    /// # }
    /// ```
    pub async fn triggered(&self) {
        self.inner.shutdown.cancelled().await;
    }

    /// Spawns `fut` on the tracker; it is dropped at its next poll once the
    /// drain is forced.
    pub(crate) fn spawn_tracked<F>(&self, fut: F) -> JoinHandle<()>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let force = self.inner.force.clone();
        self.inner.tracker.spawn(async move {
            tokio::select! {
                biased;
                () = force.cancelled() => {}
                () = fut => {}
            }
        })
    }

    /// A token that keeps [`drain`](Self::drain) waiting while it is held.
    #[cfg_attr(
        not(any(test, feature = "websocket-server")),
        expect(dead_code, reason = "only the WebSocket upgrade hand-off calls this")
    )]
    pub(crate) fn token(&self) -> TaskTrackerToken {
        self.inner.tracker.token()
    }

    /// Whether a drain has been forced.
    #[cfg_attr(
        not(any(test, feature = "websocket-server")),
        expect(dead_code, reason = "only the WebSocket upgrade hand-off calls this")
    )]
    pub(crate) fn is_forced(&self) -> bool {
        self.inner.force.is_cancelled()
    }

    /// Waits up to `timeout` for every tracked task and token to finish, then
    /// force-drops what remains.
    ///
    /// Returns within `timeout` plus 100 ms unless a tracked future never
    /// yields, which cannot be dropped and voids that bound.
    pub(crate) async fn drain(&self, timeout: DrainTimeout) -> ShutdownOutcome {
        let tracker = &self.inner.tracker;
        tracker.close();
        tokio::select! {
            () = tracker.wait() => return ShutdownOutcome::Drained,
            () = tokio::time::sleep(timeout.into()) => {}
        }
        self.inner.force.cancel();
        let remaining = tracker.len();
        tokio::select! {
            () = tracker.wait() => {}
            () = tokio::time::sleep(FORCE_GRACE) => {}
        }
        NonZeroUsize::new(remaining).map_or(ShutdownOutcome::Drained, |aborted| {
            ShutdownOutcome::Forced { aborted }
        })
    }
}

/// Shutdown parameters for [`serve_with_limits`](super::serve_with_limits).
///
/// The default is a fresh, never-triggered signal with the default
/// [`DrainTimeout`], i.e. the server runs until the process ends.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use pjson_rs::infrastructure::http::{DrainTimeout, ShutdownConfig, ShutdownSignal};
///
/// let signal = ShutdownSignal::new();
/// let config = ShutdownConfig {
///     signal: signal.clone(),
///     drain_timeout: DrainTimeout::try_from(Duration::from_secs(10))?,
/// };
/// signal.trigger();
/// assert!(config.signal.is_triggered());
/// # Ok::<(), pjson_rs::config::ConfigError>(())
/// ```
#[derive(Debug, Clone, Default)]
pub struct ShutdownConfig {
    /// The signal that stops accepting and starts the drain.
    pub signal: ShutdownSignal,
    /// How long in-flight work may take after the signal fires.
    pub drain_timeout: DrainTimeout,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timeout(millis: u64) -> DrainTimeout {
        DrainTimeout::try_from(Duration::from_millis(millis)).expect("valid timeout")
    }

    #[tokio::test(start_paused = true)]
    async fn drain_of_idle_signal_is_drained() {
        let signal = ShutdownSignal::new();
        assert_eq!(signal.drain(timeout(50)).await, ShutdownOutcome::Drained);
    }

    #[tokio::test(start_paused = true)]
    async fn drain_waits_for_a_task_that_finishes_in_time() {
        let signal = ShutdownSignal::new();
        signal.spawn_tracked(tokio::time::sleep(Duration::from_millis(20)));
        assert_eq!(signal.drain(timeout(50)).await, ShutdownOutcome::Drained);
        assert!(!signal.is_forced());
    }

    #[tokio::test(start_paused = true)]
    async fn drain_forces_a_task_that_outlives_the_timeout() {
        let signal = ShutdownSignal::new();
        let handle = signal.spawn_tracked(std::future::pending());
        assert_eq!(
            signal.drain(timeout(50)).await,
            ShutdownOutcome::Forced {
                aborted: NonZeroUsize::MIN
            }
        );
        assert!(signal.is_forced());
        assert!(handle.await.is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn held_token_counts_as_unfinished_work() {
        let signal = ShutdownSignal::new();
        let token = signal.token();
        assert!(matches!(
            signal.drain(timeout(50)).await,
            ShutdownOutcome::Forced { .. }
        ));
        drop(token);
    }
}
