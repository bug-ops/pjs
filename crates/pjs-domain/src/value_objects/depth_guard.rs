//! Generic per-thread deserialization recursion depth guard.
//!
//! Recursive types deserialized directly from untrusted input (e.g.
//! [`crate::value_objects::Schema`] here, and `SchemaDefinitionDto` in
//! `pjson-rs`) need to bound their own nesting depth independently of any
//! format-specific recursion limit a particular [`serde::Deserializer`] may
//! or may not enforce (`serde_json` happens to cap structural nesting
//! around 128 levels, but that is not a contract other formats such as
//! MessagePack or CBOR make). This module factors the thread-local counter,
//! RAII drop-guard, and bounds-checked entry pattern shared by every such
//! guard into one primitive, so each recursive type only needs to declare
//! its own counter and call [`enter_deserialize_depth`].

use std::cell::Cell;
use std::thread::LocalKey;

use crate::value_objects::MAX_DESERIALIZE_DEPTH;

/// RAII guard returned by [`enter_deserialize_depth`].
///
/// Decrements the held thread-local counter on drop — including when the
/// guarded deserialization call returns an error or unwinds — so the
/// counter always reflects the caller's actual current nesting depth.
#[must_use = "the depth guard must be held for the duration of deserialization, \
              or the depth check is silently disabled"]
pub struct DepthGuard {
    counter: &'static LocalKey<Cell<usize>>,
}

impl Drop for DepthGuard {
    fn drop(&mut self) {
        self.counter.with(|depth| depth.set(depth.get() - 1));
    }
}

/// Enters one nesting level of recursive deserialization tracked by `counter`.
///
/// Returns a guard that must be held for the duration of the nested
/// deserialization call and released (dropped) afterward. Rejects with a
/// deserialization error naming `type_name`, rather than recursing further,
/// once [`MAX_DESERIALIZE_DEPTH`] is reached.
///
/// Each recursive type declares its own
/// `thread_local! { static COUNTER: Cell<usize> = const { Cell::new(0) }; }`
/// — nesting depth is tracked independently per type — and passes a
/// `&'static` reference to it here together with a `type_name` used in the
/// rejection message, so failures from different recursive types stay
/// distinguishable from one another.
///
/// # Examples
///
/// ```
/// use pjson_rs_domain::value_objects::enter_deserialize_depth;
/// use std::cell::Cell;
///
/// thread_local! {
///     static DEPTH: Cell<usize> = const { Cell::new(0) };
/// }
///
/// fn enter() -> Result<(), serde_json::Error> {
///     let _guard = enter_deserialize_depth::<serde_json::Error>(&DEPTH, "Example")?;
///     Ok(())
/// }
///
/// assert!(enter().is_ok());
///
/// // Once `MAX_DESERIALIZE_DEPTH` guards are held at the same time, the next
/// // entry is rejected instead of recursing further.
/// let guards: Vec<_> = (0..pjson_rs_domain::MAX_DESERIALIZE_DEPTH)
///     .map(|_| enter_deserialize_depth::<serde_json::Error>(&DEPTH, "Example").unwrap())
///     .collect();
/// assert!(enter_deserialize_depth::<serde_json::Error>(&DEPTH, "Example").is_err());
/// drop(guards);
/// ```
pub fn enter_deserialize_depth<E>(
    counter: &'static LocalKey<Cell<usize>>,
    type_name: &str,
) -> Result<DepthGuard, E>
where
    E: serde::de::Error,
{
    counter.with(|depth| {
        let current = depth.get();
        if current >= MAX_DESERIALIZE_DEPTH {
            return Err(E::custom(format_args!(
                "{type_name} nesting depth exceeds maximum of {MAX_DESERIALIZE_DEPTH}"
            )));
        }
        depth.set(current + 1);
        Ok(DepthGuard { counter })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    thread_local! {
        static TEST_DEPTH: Cell<usize> = const { Cell::new(0) };
    }

    #[test]
    fn test_enter_up_to_max_depth_succeeds_then_next_entry_rejected() {
        let mut guards = Vec::with_capacity(MAX_DESERIALIZE_DEPTH);
        for _ in 0..MAX_DESERIALIZE_DEPTH {
            guards.push(enter_deserialize_depth::<serde_json::Error>(&TEST_DEPTH, "Test").unwrap());
        }

        match enter_deserialize_depth::<serde_json::Error>(&TEST_DEPTH, "Test") {
            Ok(_) => panic!("entry beyond MAX_DESERIALIZE_DEPTH should be rejected"),
            Err(err) => assert!(
                err.to_string().contains(&format!(
                    "Test nesting depth exceeds maximum of {MAX_DESERIALIZE_DEPTH}"
                )),
                "{err}"
            ),
        }

        drop(guards);
    }

    #[test]
    fn test_guard_drop_decrements_so_next_entry_starts_fresh() {
        {
            let _guard = enter_deserialize_depth::<serde_json::Error>(&TEST_DEPTH, "Test").unwrap();
        }
        assert_eq!(TEST_DEPTH.with(Cell::get), 0);

        let mut guards = Vec::with_capacity(MAX_DESERIALIZE_DEPTH);
        for _ in 0..MAX_DESERIALIZE_DEPTH {
            guards.push(enter_deserialize_depth::<serde_json::Error>(&TEST_DEPTH, "Test").unwrap());
        }
        assert!(enter_deserialize_depth::<serde_json::Error>(&TEST_DEPTH, "Test").is_err());

        drop(guards);
        assert_eq!(TEST_DEPTH.with(Cell::get), 0);
    }
}
