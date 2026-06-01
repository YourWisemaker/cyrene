//! `cyrene-runtime`: the Agent_Loop integration, daemon, and supervisor.
//!
//! This crate wires Cyrene's subsystems into the running agent:
//!
//! - [`AgentLoop`] (task 13) — the spine that composes the safety pipeline
//!   (injection scanning, planning, shadow execution, the approval gate, real
//!   execution, State_Tree checkpoints, and signed Receipt_Ledger entries) into
//!   one unskippable request lifecycle.
//! - [`Daemon`] (task 14) — the Tokio background process with an event-driven
//!   idle path and O(1) inbound dispatch.
//! - [`Supervisor`] (task 14) — the crash-restart guard that restores the
//!   latest checkpoint within the restore budget (R1.5).
//! - [`ServiceSpec`] (task 14) — systemd/launchd/Windows service definitions
//!   for run-at-startup registration.

mod agent_loop;
mod daemon;
mod error;
mod service;
mod supervisor;

pub use agent_loop::{
    AgentLoop, ApprovalResponder, Executor, Planner, StepDisposition, StepOutput, TurnOutcome,
};
pub use daemon::{Daemon, DaemonHandle, DispatchError, InboundRequest, RequestHandler};
pub use error::LoopError;
pub use service::{ServicePlatform, ServiceSpec};
pub use supervisor::{RestoreReport, RunOutcome, Supervisor, DEFAULT_RESTORE_BUDGET};

/// Returns the stable identifier of this subsystem crate.
#[must_use]
pub fn subsystem() -> &'static str {
    "cyrene-runtime"
}

#[cfg(test)]
mod tests {
    use super::subsystem;

    #[test]
    fn subsystem_id_is_nonempty() {
        assert!(!subsystem().is_empty());
    }
}
