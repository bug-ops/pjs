//! Repository implementations for data persistence
//!
//! Contains implementations for storing and retrieving PJS data.

#[cfg(all(feature = "compression", not(target_arch = "wasm32")))]
pub mod dictionary_store;

#[cfg(all(feature = "compression", not(target_arch = "wasm32")))]
pub use dictionary_store::InMemoryDictionaryStore;
