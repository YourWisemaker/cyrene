//! `cyrene-acp`: the ACP_Adapter exposing the Agent_Loop to editors (R27).
//!
//! The Agent Client Protocol lets an editor/IDE drive Cyrene over JSON-RPC.
//! This crate provides:
//!
//! - [`jsonrpc`] — JSON-RPC 2.0 request/response/error types and a validating
//!   parser, with the standard scoped error codes (R27.5).
//! - [`AcpAdapter`] — dispatches `initialize`/`prompt`/`resolve_edit`, runs
//!   prompts through the Agent_Loop, streams response + tool activity (R27.2),
//!   and binds editor edit-approval to the Approval_Gate so proposed edits are
//!   never applied without an explicit approve (R27.3, R27.4).
//!
//! The Agent_Loop is abstracted behind [`AcpBackend`] so the adapter is testable
//! without the full runtime; the CLI layer wires the real loop + transport.

pub mod jsonrpc;

mod adapter;

pub use adapter::{AcpAdapter, AcpBackend, AcpEvent, EditDecision};
pub use jsonrpc::{parse_request, Request, Response, RpcError};

/// Returns the stable identifier of this subsystem crate.
#[must_use]
pub fn subsystem() -> &'static str {
    "cyrene-acp"
}

#[cfg(test)]
mod tests {
    use super::subsystem;

    #[test]
    fn subsystem_id_is_nonempty() {
        assert!(!subsystem().is_empty());
    }
}
