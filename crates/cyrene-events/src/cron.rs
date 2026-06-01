//! The Cron_Scheduler: SQLite-persisted named cron jobs with at-most-once
//! execution (R29).
//!
//! Each [`CronJob`] has a name, a cron schedule, a task prompt, and a target
//! channel. The scheduler persists jobs in SQLite, ticks every 60s, and starts
//! a session for each due job. At-most-once execution is enforced by recording
//! the last-run timestamp per job in the DB and only firing when the current
//! minute exceeds the last-run minute (R29.5).

use std::path::Path;

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension};

use crate::heartbeat::CronExpr;

/// A named cron job persisted in the scheduler's SQLite store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronJob {
    /// The unique name of the job.
    pub name: String,
    /// The cron schedule expression (5-field).
    pub schedule: CronExpr,
    /// The raw schedule string (for display/listing).
    pub schedule_raw: String,
    /// The task prompt to run when due.
    pub task: String,
    /// The channel to deliver output on.
    pub channel: String,
    /// The last time this job was executed (if ever).
    pub last_run: Option<DateTime<Utc>>,
}

/// Errors from the cron scheduler.
#[derive(Debug, thiserror::Error)]
pub enum CronError {
    /// A database operation failed.
    #[error("cron database error: {0}")]
    Database(String),
    /// The schedule expression is invalid.
    #[error("invalid schedule: {0}")]
    InvalidSchedule(String),
    /// A job with the given name was not found.
    #[error("cron job not found: {0}")]
    NotFound(String),
}

impl From<rusqlite::Error> for CronError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Database(e.to_string())
    }
}

const SCHEMA: &str = "\
CREATE TABLE IF NOT EXISTS cron_jobs (
    name     TEXT PRIMARY KEY,
    schedule TEXT NOT NULL,
    task     TEXT NOT NULL,
    channel  TEXT NOT NULL,
    last_run TEXT
);";

/// The SQLite-backed cron scheduler.
#[derive(Debug)]
pub struct CronScheduler {
    conn: Connection,
}

impl CronScheduler {
    /// Opens (or creates) a scheduler at `db_path`.
    ///
    /// # Errors
    /// Returns [`CronError::Database`] if the DB cannot be opened.
    pub fn open(db_path: impl AsRef<Path>) -> Result<Self, CronError> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    /// Opens an in-memory scheduler (for tests).
    ///
    /// # Errors
    /// Returns [`CronError::Database`] if the schema cannot be initialized.
    pub fn in_memory() -> Result<Self, CronError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    /// Persists a new cron job (or updates an existing one with the same name).
    ///
    /// # Errors
    /// Returns [`CronError::InvalidSchedule`] if the schedule is malformed, or
    /// [`CronError::Database`] on a storage failure.
    pub fn upsert(
        &self,
        name: &str,
        schedule: &str,
        task: &str,
        channel: &str,
    ) -> Result<(), CronError> {
        // Validate the schedule expression.
        CronExpr::parse(schedule).map_err(CronError::InvalidSchedule)?;
        self.conn.execute(
            "INSERT INTO cron_jobs (name, schedule, task, channel)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(name) DO UPDATE SET schedule=?2, task=?3, channel=?4",
            rusqlite::params![name, schedule, task, channel],
        )?;
        Ok(())
    }

    /// Removes a cron job by name.
    ///
    /// # Errors
    /// Returns [`CronError::NotFound`] if no job with that name exists.
    pub fn remove(&self, name: &str) -> Result<(), CronError> {
        let affected = self
            .conn
            .execute("DELETE FROM cron_jobs WHERE name = ?1", [name])?;
        if affected == 0 {
            return Err(CronError::NotFound(name.to_owned()));
        }
        Ok(())
    }

    /// Lists all cron jobs with their schedules and last-run times (R29.4).
    ///
    /// # Errors
    /// Returns [`CronError::Database`] on a storage failure.
    pub fn list(&self) -> Result<Vec<CronJob>, CronError> {
        let mut stmt = self.conn.prepare(
            "SELECT name, schedule, task, channel, last_run FROM cron_jobs ORDER BY name",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })?;

        let mut jobs = Vec::new();
        for row in rows {
            let (name, schedule_raw, task, channel, last_run_str) = row?;
            let schedule = CronExpr::parse(&schedule_raw).map_err(CronError::InvalidSchedule)?;
            let last_run = last_run_str
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc));
            jobs.push(CronJob {
                name,
                schedule,
                schedule_raw,
                task,
                channel,
                last_run,
            });
        }
        Ok(jobs)
    }

    /// Returns the jobs that are due at `now` and have not already run this
    /// minute (at-most-once, R29.5).
    ///
    /// # Errors
    /// Returns [`CronError::Database`] on a storage failure.
    pub fn due_jobs(&self, now: DateTime<Utc>) -> Result<Vec<CronJob>, CronError> {
        let all = self.list()?;
        let now_minute = now.format("%Y-%m-%dT%H:%M").to_string();
        Ok(all
            .into_iter()
            .filter(|job| {
                if !job.schedule.matches(now) {
                    return false;
                }
                // At-most-once: skip if last_run is in the same minute.
                match &job.last_run {
                    Some(lr) => lr.format("%Y-%m-%dT%H:%M").to_string() != now_minute,
                    None => true,
                }
            })
            .collect())
    }

    /// Marks a job as having run at `now` (records last_run, R29.5).
    ///
    /// # Errors
    /// Returns [`CronError::Database`] on a storage failure.
    pub fn mark_run(&self, name: &str, now: DateTime<Utc>) -> Result<(), CronError> {
        let ts = now.to_rfc3339();
        self.conn.execute(
            "UPDATE cron_jobs SET last_run = ?1 WHERE name = ?2",
            rusqlite::params![ts, name],
        )?;
        Ok(())
    }

    /// The number of persisted jobs.
    ///
    /// # Errors
    /// Returns [`CronError::Database`] on a storage failure.
    pub fn count(&self) -> Result<usize, CronError> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM cron_jobs", [], |row| row.get(0))?;
        Ok(n as usize)
    }

    /// Looks up a single job by name.
    ///
    /// # Errors
    /// Returns [`CronError::NotFound`] if no job with that name exists.
    pub fn get(&self, name: &str) -> Result<CronJob, CronError> {
        let row = self
            .conn
            .query_row(
                "SELECT name, schedule, task, channel, last_run FROM cron_jobs WHERE name = ?1",
                [name],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| CronError::NotFound(name.to_owned()))?;

        let (name, schedule_raw, task, channel, last_run_str) = row;
        let schedule = CronExpr::parse(&schedule_raw).map_err(CronError::InvalidSchedule)?;
        let last_run = last_run_str
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.with_timezone(&Utc));
        Ok(CronJob {
            name,
            schedule,
            schedule_raw,
            task,
            channel,
            last_run,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn sched() -> CronScheduler {
        CronScheduler::in_memory().unwrap()
    }

    #[test]
    fn upsert_and_list_round_trips() {
        let s = sched();
        s.upsert("daily-report", "0 9 * * *", "Generate report", "telegram")
            .unwrap();
        let jobs = s.list().unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].name, "daily-report");
        assert_eq!(jobs[0].task, "Generate report");
        assert_eq!(jobs[0].channel, "telegram");
        assert!(jobs[0].last_run.is_none());
    }

    #[test]
    fn upsert_same_name_updates_in_place() {
        let s = sched();
        s.upsert("j", "0 9 * * *", "v1", "cli").unwrap();
        s.upsert("j", "30 10 * * *", "v2", "slack").unwrap();
        assert_eq!(s.count().unwrap(), 1);
        let job = s.get("j").unwrap();
        assert_eq!(job.task, "v2");
        assert_eq!(job.schedule_raw, "30 10 * * *");
    }

    #[test]
    fn invalid_schedule_is_rejected() {
        let s = sched();
        let err = s.upsert("bad", "not a cron", "x", "cli").unwrap_err();
        assert!(matches!(err, CronError::InvalidSchedule(_)));
    }

    #[test]
    fn remove_deletes_job() {
        let s = sched();
        s.upsert("x", "0 0 * * *", "t", "c").unwrap();
        s.remove("x").unwrap();
        assert_eq!(s.count().unwrap(), 0);
    }

    #[test]
    fn remove_missing_returns_not_found() {
        let s = sched();
        let err = s.remove("ghost").unwrap_err();
        assert!(matches!(err, CronError::NotFound(_)));
    }

    #[test]
    fn due_jobs_returns_matching_jobs() {
        let s = sched();
        s.upsert("morning", "0 7 * * *", "brief", "tg").unwrap();
        s.upsert("evening", "0 18 * * *", "summary", "tg").unwrap();

        let at_7am = Utc.with_ymd_and_hms(2024, 6, 1, 7, 0, 0).unwrap();
        let due = s.due_jobs(at_7am).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].name, "morning");
    }

    #[test]
    fn at_most_once_prevents_double_execution() {
        let s = sched();
        s.upsert("j", "0 7 * * *", "t", "c").unwrap();
        let now = Utc.with_ymd_and_hms(2024, 6, 1, 7, 0, 0).unwrap();

        // First tick: job is due.
        assert_eq!(s.due_jobs(now).unwrap().len(), 1);
        s.mark_run("j", now).unwrap();

        // Same minute: job is NOT due again (at-most-once, R29.5).
        assert_eq!(s.due_jobs(now).unwrap().len(), 0);

        // Next minute: job is not due (schedule is minute 0 only).
        let next_min = now + chrono::Duration::minutes(1);
        assert_eq!(s.due_jobs(next_min).unwrap().len(), 0);

        // Next day at 7:00: job is due again.
        let tomorrow = now + chrono::Duration::days(1);
        assert_eq!(s.due_jobs(tomorrow).unwrap().len(), 1);
    }

    #[test]
    fn mark_run_persists_last_run() {
        let s = sched();
        s.upsert("j", "0 7 * * *", "t", "c").unwrap();
        let now = Utc.with_ymd_and_hms(2024, 6, 1, 7, 0, 0).unwrap();
        s.mark_run("j", now).unwrap();
        let job = s.get("j").unwrap();
        assert!(job.last_run.is_some());
    }
}
