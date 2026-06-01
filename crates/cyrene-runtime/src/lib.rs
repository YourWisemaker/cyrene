//! `cyrene-runtime`: the Agent_Loop integration, daemon, and supervisor.
//!
//! This crate wires Cyrene's subsystems into the running agent. Task 13 builds
//! the [`AgentLoop`] — the spine that composes the safety pipeline (injection
//! scanning, planning, shadow execution, the approval gate, real execution,
//! State_Tree checkpoints, and signed Receipt_Ledger entries) into one
//! unskippable request lifecycle. Later tasks add the Tokio daemon, service
//! registration, and the crash-restore supervisor.

mod agent_loop;
mod error;

pub use agent_loop::{
    AgentLoop, ApprovalResponder, Executor, Planner, StepDisposition, StepOutput, TurnOutcome,
};
pub use error::LoopError;

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
