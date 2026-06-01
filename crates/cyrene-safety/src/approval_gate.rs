//! Approval Gate (R6): human-in-the-loop control for irreversible actions.
//!
//! When the Agent_Loop reaches an [`Irreversible_Action`], it halts execution
//! and emits an [`ApprovalRequest`] with the projected effect. The gate offers
//! **Approve / Rewrite / Abort** and persists pending state so a restart does
//! not lose it (R6.6). A configurable timeout cancels the action and logs the
//! timeout (R6.7).
//!
//! The gate does NOT execute actions — it only manages the approval lifecycle.
//! The Agent_Loop (task 13) wires it into the execution pipeline.

use std::collections::HashMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ─── Error ───────────────────────────────────────────────────────────────────

/// Errors that can occur during approval gate operations.
#[derive(Debug, thiserror::Error)]
pub enum ApprovalError {
    /// The referenced approval request was not found.
    #[error("approval request `{0}` not found")]
    NotFound(ApprovalId),

    /// The approval request has already been resolved.
    #[error("approval request `{0}` has already been resolved")]
    AlreadyResolved(ApprovalId),

    /// Serialization/deserialization failure during persist/restore.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

// ─── ApprovalId ──────────────────────────────────────────────────────────────

/// A unique identifier for an approval request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ApprovalId(Uuid);

impl ApprovalId {
    /// Generate a new random approval id.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ApprovalId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ApprovalId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ─── ApprovalRequest ─────────────────────────────────────────────────────────

/// A request for user approval before executing an irreversible action (R6.1).
///
/// Contains the pending action's description, the projected effect (from
/// [`ProjectedOutcomeSummary`] or a summary string), the step sequence number,
/// and a unique request id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    /// Unique identifier for this approval request.
    pub id: ApprovalId,
    /// The step sequence number that triggered the approval.
    pub step_seq: u64,
    /// A human-readable description of the pending action.
    pub action_description: String,
    /// The projected effect of the action (from shadow execution or a summary).
    pub projected_effect: String,
}

impl ApprovalRequest {
    /// Create a new approval request.
    #[must_use]
    pub fn new(step_seq: u64, action_description: String, projected_effect: String) -> Self {
        Self {
            id: ApprovalId::new(),
            step_seq,
            action_description,
            projected_effect,
        }
    }
}

// ─── ApprovalResponse ────────────────────────────────────────────────────────

/// The user's response to an approval request (R6.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalResponse {
    /// Allow the Agent_Loop to execute the pending action (R6.3).
    Approve,
    /// Return corrective instructions; withhold the original action (R6.5).
    Rewrite { instructions: String },
    /// Cancel the pending action and stop the plan (R6.4).
    Abort,
}

// ─── ApprovalStatus ──────────────────────────────────────────────────────────

/// The lifecycle status of a pending approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalStatus {
    /// Awaiting user response (R6.6).
    Pending,
    /// Resolved with a user response.
    Resolved(ApprovalResponse),
    /// Cancelled due to timeout (R6.7).
    TimedOut,
}

// ─── PendingApproval ─────────────────────────────────────────────────────────

/// A pending approval entry managed by the gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingApproval {
    /// The unique id of this approval.
    pub id: ApprovalId,
    /// The original request.
    pub request: ApprovalRequest,
    /// Current status.
    pub status: ApprovalStatus,
    /// When the approval was created.
    pub created_at: DateTime<Utc>,
    /// How long to wait before timing out.
    pub timeout: Duration,
}

impl PendingApproval {
    /// Returns `true` if this approval is still pending.
    #[must_use]
    pub fn is_pending(&self) -> bool {
        self.status == ApprovalStatus::Pending
    }

    /// Returns `true` if this approval has been resolved (not timed out).
    #[must_use]
    pub fn is_resolved(&self) -> bool {
        matches!(self.status, ApprovalStatus::Resolved(_))
    }

    /// Returns `true` if this approval was cancelled due to timeout.
    #[must_use]
    pub fn is_timed_out(&self) -> bool {
        self.status == ApprovalStatus::TimedOut
    }
}

// ─── ApprovalGate ────────────────────────────────────────────────────────────

/// Default approval timeout: 5 minutes.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// The Approval Gate: manages the lifecycle of approval requests for
/// irreversible actions (R6).
///
/// Stores pending approvals in memory with a serialize/deserialize interface
/// so the runtime can persist state across restarts. The gate does NOT execute
/// actions — it only tracks whether an action is approved, rewritten, or
/// aborted.
#[derive(Debug)]
pub struct ApprovalGate {
    /// In-memory store of pending approvals keyed by id.
    store: HashMap<ApprovalId, PendingApproval>,
    /// Configurable timeout duration for new approvals.
    timeout: Duration,
}

impl ApprovalGate {
    /// Create a new approval gate with the default timeout (5 minutes).
    #[must_use]
    pub fn new() -> Self {
        Self {
            store: HashMap::new(),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Create a new approval gate with a custom timeout duration.
    #[must_use]
    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            store: HashMap::new(),
            timeout,
        }
    }

    /// Submit an approval request and return the pending approval handle.
    ///
    /// Creates a pending approval, persists it in the store, and returns a
    /// reference to the pending entry. The caller (Agent_Loop) should present
    /// the request to the user and await a response or timeout.
    pub fn request_approval(&mut self, request: ApprovalRequest) -> &PendingApproval {
        let id = request.id;
        let pending = PendingApproval {
            id,
            request,
            status: ApprovalStatus::Pending,
            created_at: Utc::now(),
            timeout: self.timeout,
        };
        self.store.insert(id, pending);
        self.store.get(&id).expect("just inserted")
    }

    /// Resolve a pending approval with the user's response (R6.3, R6.4, R6.5).
    ///
    /// # Errors
    ///
    /// Returns [`ApprovalError::NotFound`] if the id doesn't exist, or
    /// [`ApprovalError::AlreadyResolved`] if the approval was already resolved
    /// or timed out.
    pub fn resolve(
        &mut self,
        id: ApprovalId,
        response: ApprovalResponse,
    ) -> Result<ApprovalResponse, ApprovalError> {
        let entry = self.store.get_mut(&id).ok_or(ApprovalError::NotFound(id))?;

        if !entry.is_pending() {
            return Err(ApprovalError::AlreadyResolved(id));
        }

        entry.status = ApprovalStatus::Resolved(response.clone());
        Ok(response)
    }

    /// Check whether the approval with the given id has timed out.
    ///
    /// Returns `true` if the approval exists, is still pending, and the
    /// elapsed time since creation exceeds the configured timeout.
    /// Returns `false` if the approval doesn't exist, is already resolved,
    /// or hasn't timed out yet.
    #[must_use]
    pub fn check_timeout(&self, id: ApprovalId) -> bool {
        self.check_timeout_at(id, Utc::now())
    }

    /// Check timeout against a specific point in time (for testing).
    #[must_use]
    pub fn check_timeout_at(&self, id: ApprovalId, now: DateTime<Utc>) -> bool {
        match self.store.get(&id) {
            Some(entry) if entry.is_pending() => {
                let elapsed = now
                    .signed_duration_since(entry.created_at)
                    .to_std()
                    .unwrap_or(Duration::ZERO);
                elapsed >= entry.timeout
            }
            _ => false,
        }
    }

    /// Cancel a pending approval due to timeout (R6.7).
    ///
    /// Marks the approval as timed out. The caller should also log the timeout
    /// to the Receipt_Ledger.
    ///
    /// # Errors
    ///
    /// Returns [`ApprovalError::NotFound`] if the id doesn't exist, or
    /// [`ApprovalError::AlreadyResolved`] if already resolved/timed out.
    pub fn cancel_on_timeout(&mut self, id: ApprovalId) -> Result<(), ApprovalError> {
        let entry = self.store.get_mut(&id).ok_or(ApprovalError::NotFound(id))?;

        if !entry.is_pending() {
            return Err(ApprovalError::AlreadyResolved(id));
        }

        entry.status = ApprovalStatus::TimedOut;
        Ok(())
    }

    /// Get a reference to a pending approval by id.
    #[must_use]
    pub fn get(&self, id: ApprovalId) -> Option<&PendingApproval> {
        self.store.get(&id)
    }

    /// Returns all currently pending approvals.
    #[must_use]
    pub fn pending_approvals(&self) -> Vec<&PendingApproval> {
        self.store
            .values()
            .filter(|entry| entry.is_pending())
            .collect()
    }

    // ── Persistence interface ────────────────────────────────────────────────

    /// Serialize the entire gate state to JSON for persistence across restarts.
    ///
    /// The runtime calls this to persist pending approvals to disk so they
    /// survive a restart (R6.6).
    ///
    /// # Errors
    ///
    /// Returns a serialization error if the state cannot be serialized.
    pub fn persist(&self) -> Result<String, ApprovalError> {
        let entries: Vec<&PendingApproval> = self.store.values().collect();
        let json = serde_json::to_string_pretty(&entries)?;
        Ok(json)
    }

    /// Restore gate state from a previously persisted JSON string.
    ///
    /// The runtime calls this on startup to restore pending approvals that
    /// were persisted before a restart (R6.6).
    ///
    /// # Errors
    ///
    /// Returns a deserialization error if the JSON is malformed.
    pub fn restore(json: &str) -> Result<Self, ApprovalError> {
        let entries: Vec<PendingApproval> = serde_json::from_str(json)?;
        let timeout = entries
            .first()
            .map(|e| e.timeout)
            .unwrap_or(DEFAULT_TIMEOUT);
        let store: HashMap<ApprovalId, PendingApproval> =
            entries.into_iter().map(|e| (e.id, e)).collect();
        Ok(Self { store, timeout })
    }
}

impl Default for ApprovalGate {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_request() -> ApprovalRequest {
        ApprovalRequest::new(
            42,
            "deploy to production".to_string(),
            "Will push v2.1.0 to prod cluster".to_string(),
        )
    }

    #[test]
    fn request_approval_creates_pending_entry() {
        let mut gate = ApprovalGate::new();
        let request = sample_request();
        let id = request.id;

        let pending = gate.request_approval(request.clone());
        assert_eq!(pending.id, id);
        assert_eq!(pending.status, ApprovalStatus::Pending);
        assert_eq!(pending.request, request);
        assert!(pending.is_pending());
    }

    #[test]
    fn resolve_with_approve_returns_approve() {
        let mut gate = ApprovalGate::new();
        let request = sample_request();
        let id = request.id;
        gate.request_approval(request);

        let result = gate.resolve(id, ApprovalResponse::Approve);
        assert_eq!(result.unwrap(), ApprovalResponse::Approve);

        let entry = gate.get(id).unwrap();
        assert!(entry.is_resolved());
    }

    #[test]
    fn resolve_with_abort_returns_abort() {
        let mut gate = ApprovalGate::new();
        let request = sample_request();
        let id = request.id;
        gate.request_approval(request);

        let result = gate.resolve(id, ApprovalResponse::Abort);
        assert_eq!(result.unwrap(), ApprovalResponse::Abort);
    }

    #[test]
    fn resolve_with_rewrite_returns_instructions() {
        let mut gate = ApprovalGate::new();
        let request = sample_request();
        let id = request.id;
        gate.request_approval(request);

        let response = ApprovalResponse::Rewrite {
            instructions: "use staging instead".to_string(),
        };
        let result = gate.resolve(id, response.clone());
        assert_eq!(result.unwrap(), response);
    }

    #[test]
    fn check_timeout_returns_true_after_timeout() {
        let mut gate = ApprovalGate::with_timeout(Duration::from_secs(60));
        let request = sample_request();
        let id = request.id;
        gate.request_approval(request);

        // Not timed out yet (now == created_at).
        assert!(!gate.check_timeout_at(id, Utc::now()));

        // Simulate time passing beyond the timeout.
        let future = Utc::now() + chrono::Duration::seconds(61);
        assert!(gate.check_timeout_at(id, future));
    }

    #[test]
    fn check_timeout_returns_false_before_timeout() {
        let mut gate = ApprovalGate::with_timeout(Duration::from_secs(300));
        let request = sample_request();
        let id = request.id;
        gate.request_approval(request);

        // 1 second later — well within the 5 minute timeout.
        let soon = Utc::now() + chrono::Duration::seconds(1);
        assert!(!gate.check_timeout_at(id, soon));
    }

    #[test]
    fn cancel_on_timeout_marks_timed_out() {
        let mut gate = ApprovalGate::new();
        let request = sample_request();
        let id = request.id;
        gate.request_approval(request);

        gate.cancel_on_timeout(id).unwrap();

        let entry = gate.get(id).unwrap();
        assert!(entry.is_timed_out());
        assert!(!entry.is_pending());
    }

    #[test]
    fn double_resolve_returns_error() {
        let mut gate = ApprovalGate::new();
        let request = sample_request();
        let id = request.id;
        gate.request_approval(request);

        // First resolve succeeds.
        gate.resolve(id, ApprovalResponse::Approve).unwrap();

        // Second resolve fails.
        let result = gate.resolve(id, ApprovalResponse::Abort);
        assert!(matches!(result, Err(ApprovalError::AlreadyResolved(_))));
    }

    #[test]
    fn resolve_after_timeout_returns_error() {
        let mut gate = ApprovalGate::new();
        let request = sample_request();
        let id = request.id;
        gate.request_approval(request);

        gate.cancel_on_timeout(id).unwrap();

        let result = gate.resolve(id, ApprovalResponse::Approve);
        assert!(matches!(result, Err(ApprovalError::AlreadyResolved(_))));
    }

    #[test]
    fn resolve_unknown_id_returns_not_found() {
        let mut gate = ApprovalGate::new();
        let unknown_id = ApprovalId::new();

        let result = gate.resolve(unknown_id, ApprovalResponse::Approve);
        assert!(matches!(result, Err(ApprovalError::NotFound(_))));
    }

    #[test]
    fn cancel_unknown_id_returns_not_found() {
        let mut gate = ApprovalGate::new();
        let unknown_id = ApprovalId::new();

        let result = gate.cancel_on_timeout(unknown_id);
        assert!(matches!(result, Err(ApprovalError::NotFound(_))));
    }

    #[test]
    fn persist_and_restore_round_trip() {
        let mut gate = ApprovalGate::with_timeout(Duration::from_secs(120));
        let request1 = sample_request();
        let id1 = request1.id;
        gate.request_approval(request1);

        let request2 = ApprovalRequest::new(
            7,
            "send email".to_string(),
            "Will send to all@company.com".to_string(),
        );
        let id2 = request2.id;
        gate.request_approval(request2);

        // Persist.
        let json = gate.persist().unwrap();

        // Restore into a new gate.
        let restored = ApprovalGate::restore(&json).unwrap();

        // Both entries should be present.
        assert!(restored.get(id1).is_some());
        assert!(restored.get(id2).is_some());
        assert!(restored.get(id1).unwrap().is_pending());
        assert!(restored.get(id2).unwrap().is_pending());
    }

    #[test]
    fn pending_approvals_returns_only_pending() {
        let mut gate = ApprovalGate::new();

        let req1 = sample_request();
        let id1 = req1.id;
        gate.request_approval(req1);

        let req2 = ApprovalRequest::new(
            10,
            "delete database".to_string(),
            "Will drop table users".to_string(),
        );
        let id2 = req2.id;
        gate.request_approval(req2);

        // Resolve one.
        gate.resolve(id1, ApprovalResponse::Approve).unwrap();

        let pending = gate.pending_approvals();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, id2);
    }

    #[test]
    fn default_timeout_is_five_minutes() {
        let gate = ApprovalGate::new();
        assert_eq!(gate.timeout, Duration::from_secs(300));
    }
}
