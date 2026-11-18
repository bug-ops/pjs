//! Injectable wall clock.
//!
//! Entities that stamp their own transitions (`Stream`) take the time from a
//! [`TimeProvider`], so a session and the streams it owns can share one clock
//! and tests can drive time deterministically.

use jiff::Timestamp;

/// Source of the current time.
///
/// Implementors return a monotonically meaningful wall-clock instant; callers
/// may assume `now` is cheap and never blocks.
///
/// # Examples
///
/// ```
/// use jiff::Timestamp;
/// use pjson_rs_domain::clock::TimeProvider;
///
/// struct Fixed(Timestamp);
///
/// impl TimeProvider for Fixed {
///     fn now(&self) -> Timestamp {
///         self.0
///     }
/// }
///
/// assert_eq!(Fixed(Timestamp::UNIX_EPOCH).now_millis(), 0);
/// ```
pub trait TimeProvider: Send + Sync {
    /// Current timestamp.
    fn now(&self) -> Timestamp;

    /// Current Unix timestamp in milliseconds.
    fn now_millis(&self) -> u64 {
        self.now().as_millisecond() as u64
    }
}

/// [`TimeProvider`] backed by the system clock.
///
/// # Examples
///
/// ```
/// use pjson_rs_domain::clock::{SystemTimeProvider, TimeProvider};
///
/// assert!(SystemTimeProvider.now_millis() > 0);
/// ```
#[derive(Debug, Clone)]
pub struct SystemTimeProvider;

impl TimeProvider for SystemTimeProvider {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}
