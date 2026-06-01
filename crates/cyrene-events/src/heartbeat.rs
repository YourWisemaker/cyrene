//! The Heartbeat_Engine: HEARTBEAT.md-driven scheduled tasks (R10).
//!
//! The engine reads a `HEARTBEAT.md` file that defines named tasks with cron-
//! like schedule expressions. At each tick it checks which tasks are due and
//! returns them so the runtime can start sessions. Between ticks the engine is
//! idle (no CPU spin, R10.4).
//!
//! ## HEARTBEAT.md format
//!
//! ```markdown
//! # Heartbeat
//!
//! ## morning-brief
//! schedule: 0 7 * * *
//! task: Prepare a morning brief of overnight activity.
//! channel: telegram
//!
//! ## nightly-backup
//! schedule: 0 2 * * *
//! task: Run the database backup procedure.
//! channel: cli
//! ```
//!
//! Each `## <name>` section defines one heartbeat task with `schedule` (cron
//! expression), `task` (the prompt), and `channel` (where to deliver output).

use chrono::{DateTime, Datelike, NaiveTime, Timelike, Utc};

/// A parsed heartbeat task from `HEARTBEAT.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeartbeatTask {
    /// The task name (from the `## name` heading).
    pub name: String,
    /// The cron-like schedule expression.
    pub schedule: CronExpr,
    /// The task prompt to run.
    pub task: String,
    /// The channel to deliver output on.
    pub channel: String,
}

/// A simplified cron expression: `minute hour day_of_month month day_of_week`.
/// Each field is either `*` (any) or a specific number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronExpr {
    pub minute: Option<u32>,
    pub hour: Option<u32>,
    pub day: Option<u32>,
    pub month: Option<u32>,
    pub weekday: Option<u32>,
}

impl CronExpr {
    /// Parses a 5-field cron expression. `*` means any.
    ///
    /// # Errors
    /// Returns a parse error string if the expression is malformed.
    pub fn parse(expr: &str) -> Result<Self, String> {
        let fields: Vec<&str> = expr.split_whitespace().collect();
        if fields.len() != 5 {
            return Err(format!("expected 5 fields, got {}", fields.len()));
        }
        Ok(Self {
            minute: parse_field(fields[0])?,
            hour: parse_field(fields[1])?,
            day: parse_field(fields[2])?,
            month: parse_field(fields[3])?,
            weekday: parse_field(fields[4])?,
        })
    }

    /// Returns `true` if `time` matches this cron expression.
    #[must_use]
    pub fn matches(&self, time: DateTime<Utc>) -> bool {
        matches_field(self.minute, time.minute())
            && matches_field(self.hour, time.hour())
            && matches_field(self.day, time.day())
            && matches_field(self.month, time.month())
            && matches_field(self.weekday, time.weekday().num_days_from_sunday())
    }
}

fn parse_field(s: &str) -> Result<Option<u32>, String> {
    if s == "*" {
        Ok(None)
    } else {
        s.parse::<u32>()
            .map(Some)
            .map_err(|_| format!("invalid cron field: {s}"))
    }
}

fn matches_field(spec: Option<u32>, actual: u32) -> bool {
    spec.is_none_or(|v| v == actual)
}

/// Parses a `HEARTBEAT.md` file into heartbeat tasks.
///
/// # Errors
/// Returns a parse error string if the file is malformed.
pub fn parse_heartbeat_md(content: &str) -> Result<Vec<HeartbeatTask>, String> {
    let mut tasks = Vec::new();
    let mut current_name: Option<String> = None;
    let mut schedule: Option<String> = None;
    let mut task: Option<String> = None;
    let mut channel: Option<String> = None;

    for line in content.lines() {
        let line = line.trim();
        if let Some(heading) = line.strip_prefix("## ") {
            // Flush the previous task if complete.
            if let Some(name) = current_name.take() {
                if let (Some(sched), Some(t), Some(ch)) =
                    (schedule.take(), task.take(), channel.take())
                {
                    tasks.push(HeartbeatTask {
                        name,
                        schedule: CronExpr::parse(&sched)?,
                        task: t,
                        channel: ch,
                    });
                }
            }
            current_name = Some(heading.trim().to_owned());
            schedule = None;
            task = None;
            channel = None;
        } else if let Some((key, value)) = line.split_once(':') {
            let key = key.trim().to_lowercase();
            let value = value.trim().to_owned();
            match key.as_str() {
                "schedule" => schedule = Some(value),
                "task" => task = Some(value),
                "channel" => channel = Some(value),
                _ => {}
            }
        }
    }

    // Flush the last task.
    if let Some(name) = current_name {
        if let (Some(sched), Some(t), Some(ch)) = (schedule, task, channel) {
            tasks.push(HeartbeatTask {
                name,
                schedule: CronExpr::parse(&sched)?,
                task: t,
                channel: ch,
            });
        }
    }

    Ok(tasks)
}

/// The Heartbeat_Engine: checks which tasks are due at a given time.
#[derive(Debug, Clone)]
pub struct HeartbeatEngine {
    tasks: Vec<HeartbeatTask>,
}

impl HeartbeatEngine {
    /// Creates an engine from parsed heartbeat tasks.
    #[must_use]
    pub fn new(tasks: Vec<HeartbeatTask>) -> Self {
        Self { tasks }
    }

    /// Loads an engine from a `HEARTBEAT.md` file content.
    ///
    /// # Errors
    /// Returns a parse error if the content is malformed.
    pub fn from_md(content: &str) -> Result<Self, String> {
        Ok(Self::new(parse_heartbeat_md(content)?))
    }

    /// Returns the tasks that are due at `now`.
    #[must_use]
    pub fn due_tasks(&self, now: DateTime<Utc>) -> Vec<&HeartbeatTask> {
        self.tasks
            .iter()
            .filter(|t| t.schedule.matches(now))
            .collect()
    }

    /// All configured tasks.
    #[must_use]
    pub fn tasks(&self) -> &[HeartbeatTask] {
        &self.tasks
    }

    /// Computes the next time a specific task will be due, searching forward
    /// minute-by-minute from `after` up to 48 hours. Returns `None` if no
    /// match is found within that window.
    #[must_use]
    pub fn next_run(&self, task_name: &str, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let task = self.tasks.iter().find(|t| t.name == task_name)?;
        let mut candidate = after
            .with_time(NaiveTime::from_hms_opt(after.hour(), after.minute(), 0)?)
            .unwrap();
        // Advance one minute past `after` to avoid matching the current minute.
        candidate += chrono::Duration::minutes(1);
        let limit = after + chrono::Duration::hours(48);
        while candidate <= limit {
            if task.schedule.matches(candidate) {
                return Some(candidate);
            }
            candidate += chrono::Duration::minutes(1);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    const SAMPLE_MD: &str = "\
# Heartbeat

## morning-brief
schedule: 0 7 * * *
task: Prepare a morning brief.
channel: telegram

## nightly-backup
schedule: 30 2 * * *
task: Run backup.
channel: cli
";

    #[test]
    fn parses_heartbeat_md() {
        let tasks = parse_heartbeat_md(SAMPLE_MD).unwrap();
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].name, "morning-brief");
        assert_eq!(tasks[0].task, "Prepare a morning brief.");
        assert_eq!(tasks[0].channel, "telegram");
        assert_eq!(tasks[0].schedule.minute, Some(0));
        assert_eq!(tasks[0].schedule.hour, Some(7));
        assert_eq!(tasks[1].name, "nightly-backup");
    }

    #[test]
    fn cron_expr_matches_correctly() {
        let expr = CronExpr::parse("0 7 * * *").unwrap();
        // 2024-01-15 07:00 UTC (Monday)
        let t = Utc.with_ymd_and_hms(2024, 1, 15, 7, 0, 0).unwrap();
        assert!(expr.matches(t));
        // 2024-01-15 08:00 UTC — wrong hour.
        let t2 = Utc.with_ymd_and_hms(2024, 1, 15, 8, 0, 0).unwrap();
        assert!(!expr.matches(t2));
    }

    #[test]
    fn due_tasks_returns_matching_tasks() {
        let engine = HeartbeatEngine::from_md(SAMPLE_MD).unwrap();
        let at_7am = Utc.with_ymd_and_hms(2024, 6, 1, 7, 0, 0).unwrap();
        let due = engine.due_tasks(at_7am);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].name, "morning-brief");
    }

    #[test]
    fn next_run_finds_upcoming_time() {
        let engine = HeartbeatEngine::from_md(SAMPLE_MD).unwrap();
        let now = Utc.with_ymd_and_hms(2024, 6, 1, 6, 30, 0).unwrap();
        let next = engine.next_run("morning-brief", now).unwrap();
        assert_eq!(next.hour(), 7);
        assert_eq!(next.minute(), 0);
    }

    #[test]
    fn invalid_cron_expr_is_rejected() {
        assert!(CronExpr::parse("0 7 *").is_err());
        assert!(CronExpr::parse("abc 7 * * *").is_err());
    }
}
