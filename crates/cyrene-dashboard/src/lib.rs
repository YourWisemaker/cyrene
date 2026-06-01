//! `cyrene-dashboard`: the local web Dashboard control plane (R26).
//!
//! The dashboard lets a user monitor and control Cyrene visually. This crate
//! provides the transport-agnostic core:
//!
//! - [`DashboardAuth`] — bearer-token authentication required before access
//!   (R26.7).
//! - [`DashboardChannel`] — a [`cyrene_core::Channel`] so dashboard chat
//!   messages enter the same Agent_Loop as any other channel (R26.3).
//! - [`DashboardState`] + [`DashboardAction`] — the serializable view-models
//!   (sessions, State_Tree, ledger, pending approvals, component status) and
//!   the actions the SPA can request (send message, checkout, resolve
//!   approval, update config) (R26.2, R26.4, R26.5, R26.6).
//!
//! The axum server + embedded SPA assets bind these at the CLI layer; keeping
//! the core here makes auth and the message round-trip testable in isolation.

mod auth;
mod channel;
mod views;

pub use auth::DashboardAuth;
pub use channel::DashboardChannel;
pub use views::{
    CheckpointRow, ComponentStatus, DashboardAction, DashboardState, LedgerStatus,
    PendingApprovalRow, ReceiptRow, SessionRow,
};

/// Returns the stable identifier of this subsystem crate.
#[must_use]
pub fn subsystem() -> &'static str {
    "cyrene-dashboard"
}

#[cfg(test)]
mod tests {
    use super::subsystem;

    #[test]
    fn subsystem_id_is_nonempty() {
        assert!(!subsystem().is_empty());
    }
}
