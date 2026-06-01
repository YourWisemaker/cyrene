//! Dashboard view-models (R26.2, R26.4, R26.5, R26.6).
//!
//! These are serializable snapshots the dashboard server sends to the SPA. The
//! actual data sources (State_Tree, Receipt_Ledger, Approval_Gate, config) are
//! injected by the runtime; this module defines the shapes and the actions the
//! dashboard exposes.

use serde::{Deserialize, Serialize};

/// A summary row in the sessions list (R26.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRow {
    /// The session id (string form).
    pub id: String,
    /// The user the session belongs to.
    pub user: String,
    /// The channel the session originated on.
    pub channel: String,
    /// Whether the session is currently active.
    pub active: bool,
}

/// A node in the State_Tree timeline (R26.2, R26.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointRow {
    /// The checkpoint id (string form).
    pub id: String,
    /// The step sequence number.
    pub step_seq: u64,
    /// A human-readable summary.
    pub summary: String,
    /// The branch this checkpoint is on.
    pub branch: String,
}

/// The verification status of the Receipt_Ledger (R26.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LedgerStatus {
    /// Every receipt verified cleanly.
    Valid,
    /// Verification diverged at a receipt seq.
    Diverged { seq: u64, reason: String },
}

/// A receipt row shown in the ledger view (R26.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiptRow {
    /// The monotonic sequence number.
    pub seq: u64,
    /// The action recorded.
    pub action: String,
    /// The deciding model.
    pub deciding_model: String,
}

/// A pending approval shown for resolution (R26.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingApprovalRow {
    /// The approval request id.
    pub id: String,
    /// The step that triggered it.
    pub step_seq: u64,
    /// A description of the pending action.
    pub action: String,
    /// The projected effect.
    pub projected_effect: String,
}

/// The status of a single subsystem/component (R26.6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentStatus {
    /// The component name (e.g. `"openai.coding"`).
    pub name: String,
    /// The component kind (model/channel/memory/subsystem).
    pub kind: String,
    /// Whether the component is healthy/available.
    pub healthy: bool,
}

/// An action the dashboard can request of the runtime (R26.3, R26.4, R26.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum DashboardAction {
    /// Submit a chat message into the loop (R26.3).
    SendMessage { session: String, text: String },
    /// Check out a checkpoint, restoring the session (R26.4).
    Checkout { checkpoint: String },
    /// Resolve a pending approval (R26.5).
    ResolveApproval {
        id: String,
        /// One of "approve" | "abort" | "rewrite".
        decision: String,
        /// Corrective instructions when decision is "rewrite".
        #[serde(default, skip_serializing_if = "Option::is_none")]
        instructions: Option<String>,
    },
    /// Update the configuration (R26.6).
    UpdateConfig { toml: String },
}

/// The complete dashboard snapshot sent to the SPA.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DashboardState {
    /// Active and past sessions (R26.2).
    pub sessions: Vec<SessionRow>,
    /// The State_Tree for the selected session (R26.2).
    pub checkpoints: Vec<CheckpointRow>,
    /// The receipt ledger rows (R26.2).
    pub receipts: Vec<ReceiptRow>,
    /// The ledger verification status (R26.2).
    pub ledger_status: LedgerStatus,
    /// Pending approvals awaiting resolution (R26.5).
    pub pending_approvals: Vec<PendingApprovalRow>,
    /// Subsystem/component statuses (R26.6).
    pub components: Vec<ComponentStatus>,
}

impl DashboardState {
    /// Creates an empty dashboard state with a valid (empty) ledger.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            sessions: Vec::new(),
            checkpoints: Vec::new(),
            receipts: Vec::new(),
            ledger_status: LedgerStatus::Valid,
            pending_approvals: Vec::new(),
            components: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashboard_state_round_trips() {
        let state = DashboardState {
            sessions: vec![SessionRow {
                id: "s1".to_owned(),
                user: "alice".to_owned(),
                channel: "cli".to_owned(),
                active: true,
            }],
            checkpoints: vec![CheckpointRow {
                id: "c1".to_owned(),
                step_seq: 0,
                summary: "init".to_owned(),
                branch: "main".to_owned(),
            }],
            receipts: vec![ReceiptRow {
                seq: 0,
                action: "plan".to_owned(),
                deciding_model: "local".to_owned(),
            }],
            ledger_status: LedgerStatus::Valid,
            pending_approvals: vec![],
            components: vec![ComponentStatus {
                name: "openai.coding".to_owned(),
                kind: "model".to_owned(),
                healthy: true,
            }],
        };
        let json = serde_json::to_string(&state).unwrap();
        let back: DashboardState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, back);
    }

    #[test]
    fn send_message_action_round_trips() {
        let action = DashboardAction::SendMessage {
            session: "s1".to_owned(),
            text: "hello".to_owned(),
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("\"action\":\"send_message\""));
        let back: DashboardAction = serde_json::from_str(&json).unwrap();
        assert_eq!(action, back);
    }

    #[test]
    fn resolve_approval_with_rewrite_round_trips() {
        let action = DashboardAction::ResolveApproval {
            id: "a1".to_owned(),
            decision: "rewrite".to_owned(),
            instructions: Some("use staging".to_owned()),
        };
        let json = serde_json::to_string(&action).unwrap();
        let back: DashboardAction = serde_json::from_str(&json).unwrap();
        assert_eq!(action, back);
    }

    #[test]
    fn checkout_action_round_trips() {
        let action = DashboardAction::Checkout {
            checkpoint: "c5".to_owned(),
        };
        let json = serde_json::to_string(&action).unwrap();
        let back: DashboardAction = serde_json::from_str(&json).unwrap();
        assert_eq!(action, back);
    }

    #[test]
    fn diverged_ledger_status_round_trips() {
        let status = LedgerStatus::Diverged {
            seq: 7,
            reason: "bad signature".to_owned(),
        };
        let json = serde_json::to_string(&status).unwrap();
        let back: LedgerStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(status, back);
    }

    #[test]
    fn empty_state_has_valid_ledger() {
        let state = DashboardState::empty();
        assert_eq!(state.ledger_status, LedgerStatus::Valid);
        assert!(state.sessions.is_empty());
    }
}
