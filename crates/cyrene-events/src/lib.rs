//! `cyrene-events`: the Event_Listener, Heartbeat_Engine, and Cron_Scheduler
//! for Cyrene (R9, R10, R29).
//!
//! This crate provides the proactivity subsystems:
//!
//! - [`EventListener`] — webhook receiver with HMAC-SHA256 signature
//!   verification for GitHub/Stripe/Linear; rejects invalid signatures and
//!   starts sessions on matching triggers (R9).
//! - [`HeartbeatEngine`] — reads a `HEARTBEAT.md` schedule file and reports
//!   which tasks are due at a given time (R10).
//! - [`CronScheduler`] — SQLite-persisted named cron jobs with at-most-once
//!   execution per scheduled minute (R29).

pub mod cron;
pub mod heartbeat;
pub mod webhook;

pub use cron::{CronError, CronJob, CronScheduler};
pub use heartbeat::{CronExpr, HeartbeatEngine, HeartbeatTask};
pub use webhook::{EventListener, WebhookSource, WebhookVerdict};

/// Returns the stable identifier of this subsystem crate.
#[must_use]
pub fn subsystem() -> &'static str {
    "cyrene-events"
}

#[cfg(test)]
mod tests {
    use super::subsystem;

    #[test]
    fn subsystem_id_is_nonempty() {
        assert!(!subsystem().is_empty());
    }
}
