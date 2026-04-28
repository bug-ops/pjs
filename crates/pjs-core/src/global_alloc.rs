//! Process-wide global allocator registration.
//!
//! When the `mimalloc` feature is enabled on a non-WASM target, registers
//! `mimalloc::MiMalloc` as the `#[global_allocator]`. This routes all
//! `Box`/`Vec`/`String` allocations through mimalloc.
//!
//! On WASM (`target_arch = "wasm32"`) the system allocator is always used,
//! since mimalloc relies on TLS and `mmap` primitives that wasm32 lacks.
//!
//! > **Note for library consumers:** the `mimalloc` feature registers MiMalloc
//! > as the binary's `#[global_allocator]`. If the downstream binary already
//! > declares a `#[global_allocator]`, enabling this feature will cause a
//! > link-time conflict. Opt in only if your binary does not set its own
//! > global allocator.

#[cfg(all(feature = "mimalloc", not(target_arch = "wasm32")))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// Returns the name of the active process-wide allocator.
///
/// Useful for diagnostics, logging, and benchmark reporting.
///
/// # Examples
///
/// ```
/// let name = pjson_rs::global_allocator_name();
/// assert!(name == "mimalloc" || name == "system");
/// ```
pub fn global_allocator_name() -> &'static str {
    #[cfg(all(feature = "mimalloc", not(target_arch = "wasm32")))]
    {
        "mimalloc"
    }
    #[cfg(not(all(feature = "mimalloc", not(target_arch = "wasm32"))))]
    {
        "system"
    }
}
