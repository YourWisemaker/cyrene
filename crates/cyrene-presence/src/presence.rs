//! The Presence_Engine: real-time thinking signals (R17).
//!
//! While the Agent_Loop works on a task that runs longer than 5 seconds, the
//! engine emits periodic status updates describing the current activity at
//! intervals no longer than 30 seconds (R17.1, R17.2). When the task completes,
//! a completion summary is emitted (R17.3).
//!
//! The engine is a state machine driven by `tick` calls from the runtime. It
//! does not own a timer — the runtime calls `tick` at its own cadence and the
//! engine decides whether an update is due.

use std::time::{Duration, Instant};

/// The minimum task duration before presence updates begin (R17.1).
const PRESENCE_THRESHOLD: Duration = Duration::from_secs(5);

/// The maximum interval between status updates (R17.2).
const MAX_UPDATE_INTERVAL: Duration = Duration::from_secs(30);

/// A status update emitted by the presence engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusUpdate {
    /// A human-readable description of the current activity (R17.4).
    pub activity: String,
    /// Whether this is the final completion summary (R17.3).
    pub is_completion: bool,
}

impl StatusUpdate {
    /// Creates a progress update.
    #[must_use]
    pub fn progress(activity: impl Into<String>) -> Self {
        Self {
            activity: activity.into(),
            is_completion: false,
        }
    }

    /// Creates a completion summary.
    #[must_use]
    pub fn completed(summary: impl Into<String>) -> Self {
        Self {
            activity: summary.into(),
            is_completion: true,
        }
    }
}

/// The Presence_Engine state machine.
#[derive(Debug)]
pub struct PresenceEngine {
    /// When the current task started (set on `start_task`).
    task_start: Option<Instant>,
    /// When the last update was emitted.
    last_update: Option<Instant>,
    /// The current activity description.
    current_activity: String,
    /// Whether the threshold has been crossed (updates are active).
    active: bool,
}

impl Default for PresenceEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl PresenceEngine {
    /// Creates an idle presence engine.
    #[must_use]
    pub fn new() -> Self {
        Self {
            task_start: None,
            last_update: None,
            current_activity: String::new(),
            active: false,
        }
    }

    /// Signals that a new task has started.
    pub fn start_task(&mut self, activity: impl Into<String>) {
        self.task_start = Some(Instant::now());
        self.last_update = None;
        self.current_activity = activity.into();
        self.active = false;
    }

    /// Updates the current activity description (e.g. "running tests").
    pub fn set_activity(&mut self, activity: impl Into<String>) {
        self.current_activity = activity.into();
    }

    /// Called periodically by the runtime. Returns a [`StatusUpdate`] if one is
    /// due (task running >5s and ≥30s since last update), or `None` otherwise.
    #[must_use]
    pub fn tick(&mut self) -> Option<StatusUpdate> {
        let start = self.task_start?;
        let elapsed = start.elapsed();

        // Don't emit updates until the task has been running >5s (R17.1).
        if elapsed < PRESENCE_THRESHOLD {
            return None;
        }

        // Activate on first crossing of the threshold.
        if !self.active {
            self.active = true;
            self.last_update = Some(Instant::now());
            return Some(StatusUpdate::progress(self.current_activity.clone()));
        }

        // Emit at most every 30s (R17.2).
        let since_last = self
            .last_update
            .map(|t| t.elapsed())
            .unwrap_or(Duration::MAX);
        if since_last >= MAX_UPDATE_INTERVAL {
            self.last_update = Some(Instant::now());
            return Some(StatusUpdate::progress(self.current_activity.clone()));
        }

        None
    }

    /// Signals that the current task has completed. Returns the completion
    /// summary update (R17.3).
    pub fn complete_task(&mut self, summary: impl Into<String>) -> StatusUpdate {
        self.task_start = None;
        self.active = false;
        StatusUpdate::completed(summary)
    }

    /// Returns `true` if a task is currently being tracked.
    #[must_use]
    pub fn is_tracking(&self) -> bool {
        self.task_start.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_update_before_threshold() {
        let mut engine = PresenceEngine::new();
        engine.start_task("working");
        // Immediately after start: no update (task hasn't run 5s yet).
        assert!(engine.tick().is_none());
    }

    #[test]
    fn first_update_after_threshold() {
        let mut engine = PresenceEngine::new();
        engine.start_task("compiling");
        // Simulate time passing beyond the 5s threshold.
        engine.task_start = Some(Instant::now() - Duration::from_secs(6));
        let update = engine.tick().unwrap();
        assert_eq!(update.activity, "compiling");
        assert!(!update.is_completion);
    }

    #[test]
    fn no_rapid_fire_updates() {
        let mut engine = PresenceEngine::new();
        engine.start_task("testing");
        engine.task_start = Some(Instant::now() - Duration::from_secs(6));
        // First tick emits.
        assert!(engine.tick().is_some());
        // Immediate second tick does NOT emit (< 30s since last).
        assert!(engine.tick().is_none());
    }

    #[test]
    fn update_after_30s_interval() {
        let mut engine = PresenceEngine::new();
        engine.start_task("deploying");
        engine.task_start = Some(Instant::now() - Duration::from_secs(40));
        // First tick emits.
        engine.tick().unwrap();
        // Simulate 30s passing since last update.
        engine.last_update = Some(Instant::now() - Duration::from_secs(31));
        let update = engine.tick().unwrap();
        assert_eq!(update.activity, "deploying");
    }

    #[test]
    fn completion_summary_is_emitted() {
        let mut engine = PresenceEngine::new();
        engine.start_task("building");
        let summary = engine.complete_task("Build succeeded in 12s");
        assert!(summary.is_completion);
        assert_eq!(summary.activity, "Build succeeded in 12s");
        assert!(!engine.is_tracking());
    }

    #[test]
    fn set_activity_changes_next_update_text() {
        let mut engine = PresenceEngine::new();
        engine.start_task("step 1");
        engine.task_start = Some(Instant::now() - Duration::from_secs(6));
        engine.set_activity("step 2");
        let update = engine.tick().unwrap();
        assert_eq!(update.activity, "step 2");
    }

    #[test]
    fn idle_engine_emits_nothing() {
        let mut engine = PresenceEngine::new();
        assert!(engine.tick().is_none());
        assert!(!engine.is_tracking());
    }
}
