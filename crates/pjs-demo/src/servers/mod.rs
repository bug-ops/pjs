//! Demo server implementations, exposed as a library for integration testing.
//!
//! Only modules whose router/state are needed by `tests/` are declared here;
//! the other demo binaries under `src/servers/` remain bin-only entry points.

pub mod websocket_streaming;
