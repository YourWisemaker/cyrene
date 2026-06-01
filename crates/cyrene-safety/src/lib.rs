//! `cyrene-safety`: the safety pipeline (Sandbox, Shadow_Executor, Approval_Gate,
//! Injection_Scanner, and autonomy policy) for Cyrene.
//!
//! ## Sandbox (R3.2, R22.3)
//!
//! The [`Sandbox`] is an isolated, copy-on-write workspace fork with OS-level
//! confinement. It:
//!
//! - Forks the workspace into a temporary directory (copy-on-write overlay).
//! - Confines filesystem access to the sandbox boundary using OS-level
//!   mechanisms (Landlock on Linux, Seatbelt on macOS) with a Docker fallback.
//! - Denies and reports any write attempt targeting paths outside the boundary.
//!
//! ## Shadow Executor (R3.1, R3.3–R3.6)
//!
//! The [`ShadowExecutor`] runs a full plan in the sandbox, intercepts
//! irreversible/external calls (records, never performs), and produces a
//! [`ProjectedOutcomeSummary`] listing file changes and would-be external
//! actions. On a failed step, it reports the failure and withholds real
//! execution.

pub mod approval_gate;
pub mod autonomy;
pub mod injection_scanner;
pub mod sandbox;
pub mod shadow_executor;

pub use sandbox::confinement::{platform_confinement, Confinement, NoopConfinement};
pub use sandbox::error::SandboxError;
pub use sandbox::{Sandbox, SandboxBackend};
pub use shadow_executor::{
    FailedStep, FileChange, FileChangeKind, InterceptedAction, ProjectedOutcomeSummary,
    ShadowExecutionConfig, ShadowExecutor,
};

pub use autonomy::{AutonomyDecision, AutonomyPolicy, RiskClassifier};

pub use approval_gate::{
    ApprovalError, ApprovalGate, ApprovalId, ApprovalRequest, ApprovalResponse, ApprovalStatus,
    PendingApproval,
};

pub use injection_scanner::{ContentSource, Detection, InjectionScanner, ScanResult};

#[cfg(target_os = "macos")]
pub use sandbox::confinement::SeatbeltConfinement;

#[cfg(target_os = "linux")]
pub use sandbox::confinement::LandlockConfinement;

/// Returns the stable identifier of this subsystem crate.
#[must_use]
pub fn subsystem() -> &'static str {
    "cyrene-safety"
}

#[cfg(test)]
mod tests {
    use super::subsystem;

    #[test]
    fn subsystem_id_is_nonempty() {
        assert!(!subsystem().is_empty());
    }
}
