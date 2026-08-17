//! Byte-bounded mpsc channel.
//!
//! [`tokio::sync::mpsc::channel`] bounds queue depth by message *count*,
//! not by queued bytes. When individual message sizes vary widely (e.g. a
//! WebSocket frame governed only by a coarse frame-count ceiling), a
//! count-bounded channel can still queue an unbounded amount of memory in
//! the worst case: `capacity * largest_possible_message`.
//!
//! [`byte_bounded_channel`] layers an additional byte budget on top of a
//! normal bounded channel using a [`Semaphore`]: the sender acquires
//! `payload_len` permits before pushing, and the permit travels with the
//! item in an [`Envelope`] so it is released back to the budget as soon as
//! the item is received (or otherwise dropped) — bounding worst-case
//! queued memory to `max_queued_bytes` regardless of individual message
//! size or how many items that adds up to.

use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc};

/// An item received from a channel created by [`byte_bounded_channel`],
/// carrying the byte-budget permit it reserved.
///
/// Access the payload via [`Envelope::into_inner`] or through [`Deref`].
/// The permit is released back to the channel's byte budget when this
/// value is dropped, so receiving (and discarding, or finishing with) an
/// item is what frees its bytes for new sends — not simply the item
/// leaving the channel's internal queue. If the consumer does further
/// work with the payload after taking it out (e.g. writing it to a
/// socket) and wants the budget charged for that duration too, use
/// [`Envelope::split`] instead of `into_inner` and drop the returned
/// [`BudgetPermit`] once that work finishes.
///
/// [`Deref`]: std::ops::Deref
pub struct Envelope<T> {
    value: T,
    permit: OwnedSemaphorePermit,
}

impl<T> Envelope<T> {
    /// Unwraps the payload, dropping the byte-budget permit alongside it.
    pub fn into_inner(self) -> T {
        self.value
    }

    /// Splits into the owned payload and a [`BudgetPermit`] guarding its
    /// bytes, without releasing them yet.
    ///
    /// Use this instead of [`Self::into_inner`] when the payload's memory
    /// stays live past the point of dequeuing it — e.g. handed to a
    /// socket write that hasn't completed — so the byte budget reflects
    /// the payload's actual lifetime instead of being released the moment
    /// it leaves the channel while a copy of it is still held elsewhere.
    pub fn split(self) -> (T, BudgetPermit) {
        (
            self.value,
            BudgetPermit {
                _permit: self.permit,
            },
        )
    }
}

impl<T> std::ops::Deref for Envelope<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.value
    }
}

// No `Clone` impl: `OwnedSemaphorePermit` isn't `Clone`, and there's no
// correct way to fabricate one for a clone — sharing the original permit
// would let the clone's bytes escape accounting when only the original is
// dropped, while acquiring a fresh same-sized permit would make `Clone`
// fallible (the budget might not have room), which the trait can't
// express. Use `Envelope::split` to detach the payload from its permit
// when a caller needs to hold or duplicate the value independently.

impl<T: std::fmt::Debug> std::fmt::Debug for Envelope<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Envelope").field(&self.value).finish()
    }
}

impl<T: PartialEq> PartialEq for Envelope<T> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl<T: Eq> Eq for Envelope<T> {}

/// A byte-budget permit detached from its payload by [`Envelope::split`].
///
/// Releases its bytes back to the originating channel's budget when
/// dropped.
pub struct BudgetPermit {
    _permit: OwnedSemaphorePermit,
}

/// Error returned by [`ByteBoundedSender::try_send`].
#[derive(Debug)]
pub enum TrySendError<T> {
    /// Enqueuing `payload_len` more bytes would exceed the channel's
    /// remaining byte budget.
    BudgetExceeded(T),
    /// The underlying bounded channel rejected the send (full or closed).
    Channel(mpsc::error::TrySendError<T>),
}

/// Error returned by [`ByteBoundedSender::send`].
#[derive(Debug)]
pub struct SendError<T>(pub T);

/// Sending half of a channel created by [`byte_bounded_channel`].
pub struct ByteBoundedSender<T> {
    inner: mpsc::Sender<Envelope<T>>,
    budget: Arc<Semaphore>,
    /// The budget's total permit count, captured at creation time.
    ///
    /// `Semaphore` has no "total permits" query, and a request for more
    /// than this can *never* be satisfied — [`Semaphore::acquire_many`]
    /// would wait forever (draining `available_permits` to 0 and starving
    /// every other waiter in the process) rather than erroring, since the
    /// semaphore has no way to distinguish "not available yet" from "not
    /// available ever". `try_send`/`send` check against this upfront so
    /// an over-budget payload is rejected immediately instead of queuing
    /// a waiter that can never be woken.
    max_queued_bytes: usize,
}

impl<T> Clone for ByteBoundedSender<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            budget: Arc::clone(&self.budget),
            max_queued_bytes: self.max_queued_bytes,
        }
    }
}

/// Converts `payload_len` into a permit count, clamping to `u32::MAX` and
/// flooring at 1 so a zero-length payload still consumes (and later
/// releases) a slot rather than bypassing the budget entirely.
fn permits_for(payload_len: usize) -> u32 {
    u32::try_from(payload_len.max(1)).unwrap_or(u32::MAX)
}

impl<T> ByteBoundedSender<T> {
    /// Attempts to enqueue `value` (whose serialized size is `payload_len`
    /// bytes) without blocking.
    ///
    /// Rejects with [`TrySendError::BudgetExceeded`] if `payload_len`
    /// would push the channel's cumulative queued bytes over its budget
    /// (including if `payload_len` alone exceeds the channel's total
    /// budget — such a payload could never fit), or with
    /// [`TrySendError::Channel`] if the underlying channel is full or its
    /// receiver has been dropped.
    pub fn try_send(&self, value: T, payload_len: usize) -> Result<(), TrySendError<T>> {
        if payload_len.max(1) > self.max_queued_bytes {
            return Err(TrySendError::BudgetExceeded(value));
        }
        let permit = match Arc::clone(&self.budget).try_acquire_many_owned(permits_for(payload_len))
        {
            Ok(permit) => permit,
            Err(_) => return Err(TrySendError::BudgetExceeded(value)),
        };
        self.inner
            .try_send(Envelope { value, permit })
            .map_err(|err| match err {
                mpsc::error::TrySendError::Full(envelope) => {
                    TrySendError::Channel(mpsc::error::TrySendError::Full(envelope.value))
                }
                mpsc::error::TrySendError::Closed(envelope) => {
                    TrySendError::Channel(mpsc::error::TrySendError::Closed(envelope.value))
                }
            })
    }

    /// Enqueues `value` (whose serialized size is `payload_len` bytes),
    /// waiting for both byte budget and channel capacity to become
    /// available.
    ///
    /// Unlike [`Self::try_send`], this applies backpressure to the caller
    /// instead of rejecting immediately — appropriate for callers outside
    /// a connection's own read/write loop, where waiting cannot deadlock
    /// the consumer.
    ///
    /// Returns [`SendError`] immediately (without waiting) if
    /// `payload_len` exceeds the channel's total byte budget: such a
    /// payload could never fit, so waiting for it to would hang forever —
    /// see [`ByteBoundedSender`]'s `max_queued_bytes` doc for why the
    /// underlying `Semaphore` can't reject this on its own.
    pub async fn send(&self, value: T, payload_len: usize) -> Result<(), SendError<T>> {
        if payload_len.max(1) > self.max_queued_bytes {
            return Err(SendError(value));
        }
        let permit = match Arc::clone(&self.budget)
            .acquire_many_owned(permits_for(payload_len))
            .await
        {
            Ok(permit) => permit,
            Err(_) => return Err(SendError(value)),
        };
        self.inner
            .send(Envelope { value, permit })
            .await
            .map_err(|err| SendError(err.0.value))
    }

    /// Forwards to the underlying channel's [`mpsc::Sender::capacity`]:
    /// the number of additional item slots currently available (ignoring
    /// the byte budget).
    pub fn capacity(&self) -> usize {
        self.inner.capacity()
    }

    /// Forwards to the underlying channel's [`mpsc::Sender::max_capacity`]:
    /// the item-count capacity the channel was created with.
    pub fn max_capacity(&self) -> usize {
        self.inner.max_capacity()
    }
}

/// Creates a byte-bounded mpsc channel.
///
/// `item_capacity` bounds queue depth by message count, exactly like
/// [`mpsc::channel`]. `max_queued_bytes` additionally bounds the sum of
/// `payload_len` across all currently-queued items, so worst-case queued
/// memory stays a predictable constant regardless of individual message
/// size.
///
/// # Examples
///
/// ```
/// use pjson_rs::infrastructure::bounded_channel::byte_bounded_channel;
///
/// # #[tokio::main]
/// # async fn main() {
/// let (tx, mut rx) = byte_bounded_channel::<&str>(10, 1024);
/// tx.try_send("hello", "hello".len()).unwrap();
/// let received = rx.recv().await.unwrap();
/// assert_eq!(received.into_inner(), "hello");
/// # }
/// ```
pub fn byte_bounded_channel<T>(
    item_capacity: usize,
    max_queued_bytes: usize,
) -> (ByteBoundedSender<T>, mpsc::Receiver<Envelope<T>>) {
    let (inner, rx) = mpsc::channel(item_capacity);
    (
        ByteBoundedSender {
            inner,
            budget: Arc::new(Semaphore::new(max_queued_bytes)),
            max_queued_bytes,
        },
        rx,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rejects_send_once_item_capacity_is_full() {
        let (tx, mut rx) = byte_bounded_channel::<&str>(2, 1024);

        tx.try_send("a", 1).unwrap();
        tx.try_send("b", 1).unwrap();

        assert!(matches!(
            tx.try_send("c", 1),
            Err(TrySendError::Channel(mpsc::error::TrySendError::Full(_)))
        ));

        rx.recv().await.unwrap();
        tx.try_send("c", 1).expect("capacity freed after a receive");
    }

    #[tokio::test]
    async fn test_rejects_send_once_byte_budget_is_exceeded() {
        // Item-count capacity is generous (100 slots); the byte budget
        // (10 bytes) is what should reject this send.
        let (tx, _rx) = byte_bounded_channel::<&str>(100, 10);

        assert!(matches!(
            tx.try_send("this string is way over budget", 31),
            Err(TrySendError::BudgetExceeded(_))
        ));

        // A payload that fits within budget still succeeds.
        tx.try_send("ok", 2).expect("small payload fits in budget");
    }

    #[tokio::test]
    async fn test_byte_budget_is_released_when_envelope_is_received() {
        let (tx, mut rx) = byte_bounded_channel::<&str>(100, 10);

        tx.try_send("12345", 5).unwrap();
        // Budget nearly exhausted (5/10 bytes used): a second 6-byte
        // payload must not fit.
        assert!(matches!(
            tx.try_send("123456", 6),
            Err(TrySendError::BudgetExceeded(_))
        ));

        // Draining the first item releases its 5 bytes back to the budget.
        let received = rx.recv().await.unwrap();
        assert_eq!(received.into_inner(), "12345");

        tx.try_send("123456", 6)
            .expect("budget freed after the first item was received and dropped");
    }

    #[tokio::test]
    async fn test_try_send_rejects_payload_larger_than_total_budget() {
        let (tx, _rx) = byte_bounded_channel::<&str>(100, 10);
        assert!(matches!(
            tx.try_send("too big", 11),
            Err(TrySendError::BudgetExceeded(_))
        ));
    }

    #[tokio::test]
    async fn test_send_rejects_payload_larger_than_total_budget_instead_of_hanging() {
        // Regression test for a critical bug: `payload_len > max_queued_bytes`
        // can never be satisfied by the underlying `Semaphore`, which has no
        // way to distinguish "not available yet" from "not available ever" —
        // `acquire_many` would wait forever, draining `available_permits` to
        // 0 and starving every other waiter (including unrelated `try_send`
        // calls, which would then also report `BudgetExceeded` forever).
        // Must reject immediately instead of hanging.
        let (tx, _rx) = byte_bounded_channel::<&str>(100, 10);

        let result = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            tx.send("too big", 11),
        )
        .await
        .expect("send must reject an unsatisfiable payload immediately, not hang");
        assert!(result.is_err());

        // The channel must still be usable afterward — proves the
        // semaphore wasn't left drained/bricked by the rejected send.
        tx.try_send("ok", 5)
            .expect("channel must remain usable after rejecting an oversized send");
    }

    #[tokio::test]
    async fn test_split_defers_budget_release_until_permit_is_dropped() {
        let (tx, mut rx) = byte_bounded_channel::<&str>(100, 10);
        tx.try_send("12345", 5).unwrap();

        let envelope = rx.recv().await.unwrap();
        let (value, permit) = envelope.split();
        assert_eq!(value, "12345");

        // The budget is still charged after `split`: a payload that only
        // fits once the first 5 bytes are released must not fit yet.
        assert!(matches!(
            tx.try_send("123456", 6),
            Err(TrySendError::BudgetExceeded(_))
        ));

        drop(permit);
        tx.try_send("123456", 6)
            .expect("budget freed once the split-off permit is dropped");
    }

    #[tokio::test]
    async fn test_concurrent_try_send_never_admits_past_byte_budget() {
        // Regression guard for the Semaphore-based budget under real
        // contention: sequential push-until-full tests can't catch a
        // races-losing-permits bug, since they never have two callers
        // acquiring from the same `Semaphore` at once. Here, 20 tasks race
        // to acquire from a 100-byte budget at 10 bytes each — at most 10
        // may be admitted concurrently, and every admitted item's bytes
        // must still be accounted for exactly once when drained.
        let (tx, mut rx) = byte_bounded_channel::<usize>(1000, 100);
        let payload_len = 10;

        let mut handles = Vec::new();
        for i in 0..20 {
            let tx = tx.clone();
            handles.push(tokio::spawn(
                async move { tx.try_send(i, payload_len).is_ok() },
            ));
        }

        let mut admitted = 0;
        for handle in handles {
            if handle.await.expect("task panicked") {
                admitted += 1;
            }
        }

        assert!(
            admitted <= 10,
            "a 100-byte budget at 10 bytes/item must never admit more than 10 \
             concurrent items, got {admitted}"
        );

        let mut drained = 0;
        while rx.try_recv().is_ok() {
            drained += 1;
        }
        assert_eq!(
            drained, admitted,
            "every admitted item must be receivable exactly once"
        );
    }

    #[tokio::test]
    async fn test_send_waits_for_budget_then_succeeds() {
        let (tx, mut rx) = byte_bounded_channel::<&str>(100, 5);

        tx.try_send("abcde", 5).unwrap();

        let tx2 = tx.clone();
        let waiter = tokio::spawn(async move { tx2.send("fghij", 5).await });

        // The waiter cannot have made progress yet: no budget is free.
        tokio::task::yield_now().await;
        assert!(!waiter.is_finished());

        rx.recv().await.unwrap();
        waiter
            .await
            .expect("task panicked")
            .expect("send should succeed once budget frees up");
    }
}
