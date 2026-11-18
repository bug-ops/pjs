//! Entry points that let the `fuzz/` crate reach crate-private websocket
//! machinery without widening the real API.
//!
//! The module is compiled only under `--cfg fuzzing` (set by `cargo fuzz`) or
//! `--cfg pjs_bench`, so it is not part of the published surface.

#[cfg(feature = "websocket-client")]
pub use client_harness::ClientPatchHarness;
#[cfg(feature = "websocket-server")]
pub use planner_entry::plan_messages;

#[cfg(feature = "websocket-client")]
mod client_harness {
    use super::super::{Framing, WirePatch, client::ClientStreamSession};
    use crate::{Result as PjsResult, domain::SessionId};
    use serde_json::Value;

    /// Client-side patch reconstruction state with its byte accounting.
    ///
    /// Mirrors a live client session: [`apply_patches`](Self::apply_patches)
    /// has the production cap semantics, so a fuzz oracle can check that
    /// [`tracked_bytes`](Self::tracked_bytes) never undercounts
    /// [`reconstructed`](Self::reconstructed).
    #[derive(Debug)]
    pub struct ClientPatchHarness(ClientStreamSession);

    impl Default for ClientPatchHarness {
        fn default() -> Self {
            Self::new()
        }
    }

    impl ClientPatchHarness {
        /// Starts from the empty-object state a stream begins with.
        pub fn new() -> Self {
            Self(ClientStreamSession::new(SessionId::new(), Framing::Patch))
        }

        /// Applies `patches` in order, refusing any patch that would take the
        /// accounted size above `max_bytes`.
        ///
        /// # Errors
        ///
        /// Returns the production error: a limit error past the cap, a patch
        /// error for a path that does not resolve. The state may be partly
        /// modified after an error.
        pub fn apply_patches(
            &mut self,
            patches: Vec<WirePatch>,
            max_bytes: usize,
        ) -> PjsResult<()> {
            self.0.apply_patches(patches, max_bytes)
        }

        /// Merges a merge-framing `payload` with the production cap semantics.
        ///
        /// # Errors
        ///
        /// Returns a limit error when the merged size would pass `max_bytes`;
        /// nothing is modified then.
        pub fn apply_frame(&mut self, payload: Value, max_bytes: usize) -> PjsResult<()> {
            self.0.apply_frame(payload, max_bytes)
        }

        /// The document reconstructed so far.
        pub fn reconstructed(&self) -> &Value {
            &self.0.reconstructed_data
        }

        /// The size the session accounts against its cap.
        pub fn tracked_bytes(&self) -> usize {
            self.0.reconstructed_bytes
        }
    }
}

#[cfg(feature = "websocket-server")]
mod planner_entry {
    use super::super::{
        StreamOptions,
        controller::CreateSessionError,
        planner::{PlanOptions, plan_stream},
    };
    use crate::domain::SessionId;
    use serde_json::Value;

    /// Plans the messages that deliver `data` under `options`.
    ///
    /// # Errors
    ///
    /// [`CreateSessionError::PlanRejected`] for a document the planner
    /// refuses; no other variant occurs.
    pub fn plan_messages(
        data: Value,
        options: &StreamOptions,
    ) -> Result<Vec<String>, CreateSessionError> {
        let plan_options = PlanOptions::from(options);
        plan_stream(SessionId::new(), data, &plan_options)
            .map(|plan| plan.messages)
            .map_err(CreateSessionError::PlanRejected)
    }
}
