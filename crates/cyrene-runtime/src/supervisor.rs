//! The supervisor: crash restart + checkpoint restore within budget (R1.5).
//!
//! The supervisor wraps the agent run loop in a restart guard. When the loop
//! exits unexpectedly, the supervisor:
//!
//! 1. Loads the most recent persisted checkpoint for the session from the
//!    State_Tree (restore = "load latest checkpoint", since state is persisted
//!    on every step).
//! 2. Restarts the loop from that restored state.
//! 3. Enforces a wall-clock restore budget (default 5s, R1.5): if restore takes
//!    longer, it reports a budget breach so the caller can alert.
//!
//! The restartable unit of work is expressed as a closure returning a
//! [`RunOutcome`], so the supervisor is testable without a real loop and the
//! restart policy is decoupled from what is being run.

use std::time::{Duration, Instant};

use cyrene_core::SessionId;
use cyrene_state::{Checkpoint, StateStore};

/// The default restore budget mandated by R1.5.
pub const DEFAULT_RESTORE_BUDGET: Duration = Duration::from_secs(5);

/// Why a supervised run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunOutcome {
    /// The run finished its work normally; do not restart.
    Completed,
    /// The run crashed/terminated unexpectedly; the supervisor should restore
    /// and restart.
    Crashed,
}

/// The result of a restore attempt after a crash.
#[derive(Debug, Clone)]
pub struct RestoreReport {
    /// The checkpoint restored from, if any existed for the session.
    pub restored_checkpoint: Option<Checkpoint>,
    /// How long the restore took.
    pub elapsed: Duration,
    /// Whether the restore completed within the budget (R1.5).
    pub within_budget: bool,
}

/// Supervises a restartable run, restoring the latest checkpoint on crash.
pub struct Supervisor<'a> {
    state: &'a StateStore,
    restore_budget: Duration,
    max_restarts: u32,
}

impl<'a> Supervisor<'a> {
    /// Creates a supervisor over a state store with the default 5s budget.
    #[must_use]
    pub fn new(state: &'a StateStore) -> Self {
        Self {
            state,
            restore_budget: DEFAULT_RESTORE_BUDGET,
            max_restarts: 16,
        }
    }

    /// Sets the restore budget (R1.5).
    #[must_use]
    pub fn with_restore_budget(mut self, budget: Duration) -> Self {
        self.restore_budget = budget;
        self
    }

    /// Caps the number of automatic restarts to avoid crash loops.
    #[must_use]
    pub fn with_max_restarts(mut self, max: u32) -> Self {
        self.max_restarts = max;
        self
    }

    /// Restores the most recent persisted checkpoint for `session`, timing the
    /// operation against the restore budget (R1.5).
    ///
    /// Returns a [`RestoreReport`] whose `restored_checkpoint` is `None` when
    /// the session has no checkpoints yet (a fresh crash before any step).
    ///
    /// # Errors
    /// Returns a [`cyrene_state::StateError`] on a storage failure.
    pub fn restore_latest(
        &self,
        session: SessionId,
    ) -> Result<RestoreReport, cyrene_state::StateError> {
        let start = Instant::now();
        let history = self.state.history(session)?;
        let restored = match history.last() {
            Some(summary) => Some(self.state.checkout(summary.id)?),
            None => None,
        };
        let elapsed = start.elapsed();
        Ok(RestoreReport {
            restored_checkpoint: restored,
            within_budget: elapsed <= self.restore_budget,
            elapsed,
        })
    }

    /// Runs `run` under supervision: on [`RunOutcome::Crashed`] it restores the
    /// latest checkpoint and restarts, up to `max_restarts` times.
    ///
    /// `run` receives the [`RestoreReport`] from the preceding crash (or `None`
    /// on the first start) so it can resume from restored state. The loop ends
    /// when `run` returns [`RunOutcome::Completed`] or the restart cap is hit.
    ///
    /// Returns the number of restarts performed.
    ///
    /// # Errors
    /// Returns a [`cyrene_state::StateError`] if a restore fails.
    pub fn supervise<F>(
        &self,
        session: SessionId,
        mut run: F,
    ) -> Result<u32, cyrene_state::StateError>
    where
        F: FnMut(Option<&RestoreReport>) -> RunOutcome,
    {
        let mut restarts = 0u32;
        let mut last_restore: Option<RestoreReport> = None;

        loop {
            match run(last_restore.as_ref()) {
                RunOutcome::Completed => return Ok(restarts),
                RunOutcome::Crashed => {
                    if restarts >= self.max_restarts {
                        return Ok(restarts);
                    }
                    last_restore = Some(self.restore_latest(session)?);
                    restarts += 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cyrene_core::BranchId;
    use std::cell::Cell;

    fn store() -> StateStore {
        StateStore::open_in_memory().unwrap()
    }

    #[test]
    fn restore_latest_returns_none_when_no_checkpoints() {
        let store = store();
        let sup = Supervisor::new(&store);
        let report = sup.restore_latest(SessionId::new()).unwrap();
        assert!(report.restored_checkpoint.is_none());
        assert!(report.within_budget);
    }

    #[test]
    fn restore_latest_picks_the_most_recent_checkpoint() {
        let store = store();
        let session = SessionId::new();
        let branch = BranchId::new();
        store
            .checkpoint(session, 0, None, branch, "s0", b"m0", b"v0", &[])
            .unwrap();
        let id1 = store
            .checkpoint(session, 1, None, branch, "s1", b"m1", b"v1", &[])
            .unwrap();

        let sup = Supervisor::new(&store);
        let report = sup.restore_latest(session).unwrap();
        let restored = report.restored_checkpoint.unwrap();
        assert_eq!(restored.id, id1);
        assert_eq!(restored.step_seq, 1);
    }

    #[test]
    fn restore_within_default_budget() {
        let store = store();
        let session = SessionId::new();
        let branch = BranchId::new();
        store
            .checkpoint(session, 0, None, branch, "s0", b"m", b"v", &[])
            .unwrap();

        let sup = Supervisor::new(&store);
        let report = sup.restore_latest(session).unwrap();
        // An in-memory restore is far under 5s.
        assert!(report.within_budget);
        assert!(report.elapsed < DEFAULT_RESTORE_BUDGET);
    }

    #[test]
    fn supervise_restarts_on_crash_then_completes() {
        let store = store();
        let session = SessionId::new();
        let branch = BranchId::new();
        store
            .checkpoint(session, 0, None, branch, "s0", b"m", b"v", &[])
            .unwrap();

        let attempts = Cell::new(0u32);
        let sup = Supervisor::new(&store);
        let restarts = sup
            .supervise(session, |restore| {
                let n = attempts.get();
                attempts.set(n + 1);
                if n == 0 {
                    // First start: no prior restore.
                    assert!(restore.is_none());
                    RunOutcome::Crashed
                } else {
                    // After a crash we get the restored checkpoint.
                    assert!(restore.is_some());
                    assert!(restore.unwrap().restored_checkpoint.is_some());
                    RunOutcome::Completed
                }
            })
            .unwrap();

        assert_eq!(restarts, 1);
        assert_eq!(attempts.get(), 2);
    }

    #[test]
    fn supervise_honors_restart_cap() {
        let store = store();
        let session = SessionId::new();
        let sup = Supervisor::new(&store).with_max_restarts(3);
        // Always crashes: the cap stops the loop.
        let restarts = sup.supervise(session, |_| RunOutcome::Crashed).unwrap();
        assert_eq!(restarts, 3);
    }

    #[test]
    fn tight_budget_is_reported_as_breach() {
        let store = store();
        let session = SessionId::new();
        let branch = BranchId::new();
        store
            .checkpoint(session, 0, None, branch, "s0", b"m", b"v", &[])
            .unwrap();

        // A zero budget cannot be met, so within_budget must be false.
        let sup = Supervisor::new(&store).with_restore_budget(Duration::ZERO);
        let report = sup.restore_latest(session).unwrap();
        assert!(!report.within_budget);
    }
}
