//! Error model for the Agent_Loop and runtime.

use cyrene_ledger::LedgerError;
use cyrene_safety::{ApprovalError, SandboxError};
use cyrene_state::StateError;

/// Errors the Agent_Loop can return while driving a turn.
#[derive(Debug, thiserror::Error)]
pub enum LoopError {
    /// The planner (Model_Router boundary) failed to produce a plan.
    #[error("planning failed: {0}")]
    Planner(String),

    /// A step executor failed while performing a real action.
    #[error("execution failed at step {seq}: {message}")]
    Executor {
        /// The step sequence number that failed.
        seq: u64,
        /// The underlying error message.
        message: String,
    },

    /// The Receipt_Ledger failed to record a receipt.
    #[error("ledger error: {0}")]
    Ledger(#[from] LedgerError),

    /// The State_Tree failed to record a checkpoint.
    #[error("state error: {0}")]
    State(#[from] StateError),

    /// The sandbox failed during shadow execution.
    #[error("sandbox error: {0}")]
    Sandbox(#[from] SandboxError),

    /// The Approval_Gate failed to manage a pending approval.
    #[error("approval error: {0}")]
    Approval(#[from] ApprovalError),
}
