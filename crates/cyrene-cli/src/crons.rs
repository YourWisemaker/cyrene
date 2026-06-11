//! Real cron scheduling for the CLI.
//!
//! Wires the SQLite-backed [`cyrene_events::CronScheduler`] (named jobs,
//! at-most-once execution per minute) to two concrete actions Cyrene needs for
//! the "report me X every day" workflow:
//!
//! - **run** a saved Python script (`~/.cyrene/scripts/<name>.py`), and
//! - **deliver** its output to a channel — Telegram, Discord, or the CLI.
//!
//! A job's `task` field holds the script name (or path); its `channel` is one
//! of `cli`, `telegram`/`telegram:<chat_id>`, or `discord`. The daemon (`cyrene cron run`)
//! ticks every 60s and fires due jobs; `run-once` fires a single job now (handy
//! for testing a freshly written scraper).

use std::path::PathBuf;
use std::time::Duration;

use chrono::{Local, Utc};
use cyrene_events::CronScheduler;

use crate::pyexec;

/// How long a scheduled script may run before it's killed.
const JOB_TIMEOUT: Duration = Duration::from_secs(300);

/// Path to the cron job database (`~/.cyrene/cron.db`).
fn db_path() -> PathBuf {
    let base = cyrene_config::cyrene_home_dir().unwrap_or_default();
    let _ = std::fs::create_dir_all(&base);
    base.join("cron.db")
}

/// Opens (creating if needed) the persistent scheduler.
fn open() -> Result<CronScheduler, String> {
    CronScheduler::open(db_path()).map_err(|e| e.to_string())
}

/// Loads `~/.cyrene/.env` so scheduled scripts and Telegram delivery see the
/// same secrets the interactive chat does.
fn load_env() {
    let base = cyrene_config::cyrene_home_dir().unwrap_or_default();
    let _ = cyrene_config::SecretResolver::with_dotenv_path(base.join(".env"));
}

/// Converts a friendly schedule into a validated 5-field cron string.
///
/// Accepts: `@hourly`/`hourly`, `@daily`/`daily`/`midnight`, a local `HH:MM`
/// (converted to UTC, since matching runs in UTC), or a raw 5-field cron
/// expression (passed through after validation).
pub fn normalize_schedule(raw: &str) -> Result<String, String> {
    let t = raw.trim();
    match t.to_lowercase().as_str() {
        "@hourly" | "hourly" => return Ok("0 * * * *".to_owned()),
        "@daily" | "daily" | "midnight" => return Ok("0 0 * * *".to_owned()),
        _ => {}
    }

    // `HH:MM` local time → a daily cron in UTC.
    if let Some((h, m)) = t.split_once(':') {
        if let (Ok(h), Ok(m)) = (h.trim().parse::<u32>(), m.trim().parse::<u32>()) {
            if h < 24 && m < 60 {
                let offset = Local::now().offset().local_minus_utc();
                return Ok(hhmm_local_to_cron(h, m, offset));
            }
        }
    }

    // Otherwise require a valid 5-field expression.
    cyrene_events::CronExpr::parse(t).map_err(|e| format!("invalid schedule `{t}`: {e}"))?;
    Ok(t.to_owned())
}

/// Pure local-`HH:MM` → UTC daily-cron conversion. `offset_secs` is
/// local-minus-UTC (e.g. +25200 for UTC+7). Factored out for deterministic
/// tests independent of the host timezone.
fn hhmm_local_to_cron(h: u32, m: u32, offset_secs: i32) -> String {
    let local_min = (h * 60 + m) as i32;
    let utc_min = (local_min - offset_secs / 60).rem_euclid(1440);
    format!("{} {} * * *", utc_min % 60, utc_min / 60)
}

/// Adds (or updates) a job. `task` is the script name/path; `channel` is `cli`,
/// `telegram`, or `telegram:<chat_id>`.
pub fn add(name: &str, schedule_raw: &str, task: &str, channel: &str) -> Result<(), String> {
    let schedule = normalize_schedule(schedule_raw)?;
    // Warn early if the script can't be found yet (still allowed — it may be
    // created later — but most "nothing happened" reports trace back to this).
    if pyexec::resolve_script(task).is_none() {
        eprintln!("  note: no saved script `{task}` yet — save one with /script {task} in chat.");
    }
    let s = open()?;
    s.upsert(name, &schedule, task, channel)
        .map_err(|e| e.to_string())?;
    println!("✓ Scheduled `{name}`: {schedule} (UTC) → run `{task}` → {channel}");
    Ok(())
}

/// Prints all jobs.
pub fn list() {
    let Ok(s) = open() else {
        println!("Cron Jobs:\n  (could not open the cron store)");
        return;
    };
    match s.list() {
        Ok(jobs) if jobs.is_empty() => {
            println!("Cron Jobs:\n  (none yet — `cyrene cron add` or /cron in chat)");
        }
        Ok(jobs) => {
            println!("Cron Jobs:\n");
            for j in jobs {
                let last = j
                    .last_run
                    .map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string())
                    .unwrap_or_else(|| "never".to_owned());
                println!(
                    "  {} — {} (UTC) → {} → {}   [last run: {}]",
                    j.name, j.schedule_raw, j.task, j.channel, last
                );
            }
        }
        Err(e) => println!("Cron Jobs:\n  (error: {e})"),
    }
}

/// Removes a job by name.
pub fn remove(name: &str) {
    match open().and_then(|s| s.remove(name).map_err(|e| e.to_string())) {
        Ok(()) => println!("✓ Removed cron job `{name}`."),
        Err(e) => println!("  ✗ {e}"),
    }
}

/// Runs a single job now and delivers its output. Used by `cron run-once`.
pub fn run_once(name: &str) -> Result<(), String> {
    load_env();
    let s = open()?;
    let job = s.get(name).map_err(|e| e.to_string())?;
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    fire(&rt, &job.task, &job.channel);
    s.mark_run(name, Utc::now()).map_err(|e| e.to_string())?;
    Ok(())
}

/// Runs the scheduler loop: every 60s, fire due jobs and record their run.
/// Blocks until interrupted. Used by the explicit `cyrene cron run` command.
pub fn run_daemon() {
    load_env();
    let s = match open() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("  ✗ Could not open cron store: {e}");
            return;
        }
    };
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("  ✗ Could not start async runtime: {e}");
            return;
        }
    };
    println!("✓ Cyrene cron is running. Checking every minute… (Ctrl-C to stop)");
    loop {
        tick(&rt, &s, true);
        std::thread::sleep(Duration::from_secs(60));
    }
}

/// Whether the in-process background scheduler has already been started, so
/// repeated entry points (or a re-entered REPL) don't spawn duplicates.
static BACKGROUND_STARTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Starts the cron scheduler in a background thread for the lifetime of this
/// process, unless one is already running. Idempotent.
///
/// This is what lets "report me X every morning" work straight from a chat,
/// Telegram, or WhatsApp session — the long-running process fires due jobs
/// itself, with no separate `cyrene cron run`. The thread is quiet: it logs
/// fires to stderr but prints nothing on the happy idle path.
pub fn spawn_background() {
    use std::sync::atomic::Ordering;
    if BACKGROUND_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("cyrene-cron".to_owned())
        .spawn(background_loop);
}

/// The background scheduler body: open the store, then tick every 60s. Any
/// setup failure ends the thread quietly — scheduling simply stays manual.
fn background_loop() {
    load_env();
    let Ok(s) = open() else {
        return;
    };
    let Ok(rt) = tokio::runtime::Runtime::new() else {
        return;
    };
    loop {
        tick(&rt, &s, false);
        std::thread::sleep(Duration::from_secs(60));
    }
}

/// Fires every job due at the current minute and records its run. `verbose`
/// prints a line per fire (the foreground daemon); the background scheduler
/// keeps quiet except on errors.
fn tick(rt: &tokio::runtime::Runtime, s: &CronScheduler, verbose: bool) {
    let now = Utc::now();
    match s.due_jobs(now) {
        Ok(jobs) => {
            for job in jobs {
                if verbose {
                    println!("  cron: firing `{}` → {}", job.name, job.task);
                }
                fire(rt, &job.task, &job.channel);
                let _ = s.mark_run(&job.name, now);
            }
        }
        Err(e) => eprintln!("  cron: scheduler error: {e}"),
    }
}

/// Runs a job's script and delivers the captured output to its channel.
fn fire(rt: &tokio::runtime::Runtime, script: &str, channel: &str) {
    let report = match run_script(script) {
        Ok(text) => text,
        Err(e) => format!("⚠️ Cyrene couldn't run `{script}`: {e}"),
    };
    deliver(rt, channel, &report);
}

/// Resolves and runs a saved script, returning its combined output as the
/// report body.
fn run_script(script: &str) -> Result<String, String> {
    let Some(path) = pyexec::resolve_script(script) else {
        return Err(format!("no script named `{script}`"));
    };
    let py = pyexec::interpreter().ok_or("no Python interpreter found")?;
    let outcome = pyexec::run_file(py, &path, JOB_TIMEOUT)?;
    Ok(outcome.summary())
}

/// Delivers `text` to a channel:
/// - `cli` — prints to stdout.
/// - `telegram[:chat_id]` — Bot API (chat id from suffix or `TELEGRAM_CHAT_ID`).
/// - `discord` — posts to the `DISCORD_WEBHOOK_URL` webhook (no bot needed).
fn deliver(rt: &tokio::runtime::Runtime, channel: &str, text: &str) {
    let (kind, target) = channel.split_once(':').unwrap_or((channel, ""));
    match kind {
        "telegram" => {
            let chat_id = if target.is_empty() {
                std::env::var("TELEGRAM_CHAT_ID").unwrap_or_default()
            } else {
                target.to_owned()
            };
            let token = std::env::var("TELEGRAM_BOT_TOKEN").unwrap_or_default();
            if token.is_empty() || chat_id.is_empty() {
                eprintln!(
                    "  cron: telegram delivery needs TELEGRAM_BOT_TOKEN and a chat id \
                     (channel `telegram:<chat_id>` or TELEGRAM_CHAT_ID). Printing instead:\n{text}"
                );
                return;
            }
            rt.block_on(send_telegram(&token, &chat_id, text));
        }
        "discord" => {
            let url = std::env::var("DISCORD_WEBHOOK_URL").unwrap_or_default();
            if url.is_empty() {
                eprintln!(
                    "  cron: discord delivery needs DISCORD_WEBHOOK_URL in ~/.cyrene/.env \
                     (Server Settings → Integrations → Webhooks). Printing instead:\n{text}"
                );
                return;
            }
            rt.block_on(send_discord(&url, text));
        }
        "whatsapp" => {
            let token = std::env::var("WHATSAPP_TOKEN").unwrap_or_default();
            let phone_id = std::env::var("WHATSAPP_PHONE_NUMBER_ID").unwrap_or_default();
            if token.is_empty() || phone_id.is_empty() || target.is_empty() {
                eprintln!(
                    "  cron: whatsapp delivery needs WHATSAPP_TOKEN, WHATSAPP_PHONE_NUMBER_ID, \
                     and a recipient (channel `whatsapp:<number>`). Printing instead:\n{text}"
                );
                return;
            }
            rt.block_on(send_whatsapp(&token, &phone_id, target, text));
        }
        _ => println!("\n[cron report]\n{text}\n"),
    }
}

/// Sends a Telegram message (chunked to stay under the API's ~4096-char limit).
async fn send_telegram(token: &str, chat_id: &str, text: &str) {
    let client = reqwest::Client::new();
    let url = format!("https://api.telegram.org/bot{token}/sendMessage");
    for chunk in chunk_text(text, 3500) {
        let body = serde_json::json!({ "chat_id": chat_id, "text": chunk });
        if let Err(e) = client.post(&url).json(&body).send().await {
            eprintln!("  cron: telegram send error: {e}");
        }
    }
}

/// Posts a message to a Discord webhook (chunked to Discord's 2000-char limit).
async fn send_discord(webhook_url: &str, text: &str) {
    let client = reqwest::Client::new();
    for chunk in chunk_text(text, 1900) {
        let body = serde_json::json!({ "content": chunk });
        if let Err(e) = client.post(webhook_url).json(&body).send().await {
            eprintln!("  cron: discord send error: {e}");
        }
    }
}

/// Sends a WhatsApp message via the Graph API (chunked under the body limit).
/// Mirrors the send path in `whatsapp.rs` so scheduled reports reach the same
/// recipient who asked for them.
async fn send_whatsapp(token: &str, phone_number_id: &str, to: &str, text: &str) {
    let client = reqwest::Client::new();
    let url = format!("https://graph.facebook.com/v20.0/{phone_number_id}/messages");
    for chunk in chunk_text(text, 3500) {
        let body = serde_json::json!({
            "messaging_product": "whatsapp",
            "to": to,
            "type": "text",
            "text": { "body": chunk },
        });
        if let Err(e) = client
            .post(&url)
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
        {
            eprintln!("  cron: whatsapp send error: {e}");
        }
    }
}

/// Splits text into <= `max`-byte chunks on line boundaries where possible.
fn chunk_text(text: &str, max: usize) -> Vec<String> {
    if text.len() <= max {
        return vec![text.to_owned()];
    }
    let mut chunks = Vec::new();
    let mut cur = String::new();
    for line in text.lines() {
        if cur.len() + line.len() + 1 > max && !cur.is_empty() {
            chunks.push(std::mem::take(&mut cur));
        }
        cur.push_str(line);
        cur.push('\n');
    }
    if !cur.is_empty() {
        chunks.push(cur);
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyword_schedules_normalize() {
        assert_eq!(normalize_schedule("@daily").unwrap(), "0 0 * * *");
        assert_eq!(normalize_schedule("hourly").unwrap(), "0 * * * *");
    }

    #[test]
    fn raw_cron_passes_through_and_validates() {
        assert_eq!(normalize_schedule("30 6 * * 1").unwrap(), "30 6 * * 1");
        // Wrong field count is rejected (the underlying parser is range-lenient).
        assert!(normalize_schedule("not a cron").is_err());
        assert!(normalize_schedule("0 0 * *").is_err());
    }

    #[test]
    fn hhmm_converts_local_to_utc() {
        // 08:00 at UTC+7 (Jakarta) → 01:00 UTC.
        assert_eq!(hhmm_local_to_cron(8, 0, 7 * 3600), "0 1 * * *");
        // 06:30 at UTC+0 → 06:30 UTC.
        assert_eq!(hhmm_local_to_cron(6, 30, 0), "30 6 * * *");
        // 02:00 at UTC+7 wraps to 19:00 UTC the previous day.
        assert_eq!(hhmm_local_to_cron(2, 0, 7 * 3600), "0 19 * * *");
        // 23:00 at UTC-5 → 04:00 UTC (next day).
        assert_eq!(hhmm_local_to_cron(23, 0, -5 * 3600), "0 4 * * *");
    }

    #[test]
    fn chunking_splits_long_text() {
        let text = "line\n".repeat(2000); // ~10k chars
        let chunks = chunk_text(&text, 3500);
        assert!(chunks.len() > 1);
        assert!(chunks.iter().all(|c| c.len() <= 3500 + 5));
    }
}
