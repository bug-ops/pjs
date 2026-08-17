//! Route handler groups for the Axum HTTP adapter, split by domain concept.
//!
//! Shared state ([`PjsAppState`](super::axum_adapter::PjsAppState)), router
//! assembly, and error mapping live in [`super::axum_adapter`]; this module
//! only holds the individual `async fn` handlers grouped by the resource they
//! serve.

#[cfg(all(feature = "compression", not(target_arch = "wasm32")))]
pub mod dictionary;
pub mod health;
pub mod sessions;
pub mod streams;
