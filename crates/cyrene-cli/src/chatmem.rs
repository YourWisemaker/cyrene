//! Lightweight persistent memory for the chat REPL.
//!
//! Cyrene "remembers every input and improves itself" by appending a durable,
//! human-readable log to `~/.cyrene/memory/chat.jsonl` — one JSON object per
//! line. Two kinds of entries matter:
//!
//! - `input`  — every message the user sends, so past sessions are recoverable
//!   and Cyrene can learn from how it's actually used.
//! - `fact`   — things the user explicitly asked Cyrene to remember (`/remember`)
//!   or preferences it inferred. Facts are injected into the system prompt of
//!   every new session, so guidance carries across restarts.
//!
//! JSONL (append-only, one record per line) is deliberately boring: it survives
//! partial writes, is trivial to inspect, and never needs a migration.
//!
//! The public helpers operate on the real `~/.cyrene` log; the `*_at` variants
//! take an explicit path and carry the logic, which keeps the unit tests free
//! of any process-global state (no `CYRENE_HOME` juggling).

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One remembered entry.
#[derive(Serialize, Deserialize, Clone)]
pub struct Entry {
    /// Unix seconds when recorded.
    pub ts: u64,
    /// `"input"`, `"fact"`, or `"reply"`.
    pub kind: String,
    pub text: String,
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Path to the append-only memory log (`~/.cyrene/memory/chat.jsonl`).
#[must_use]
pub fn log_path() -> PathBuf {
    let base = cyrene_config::cyrene_home_dir().unwrap_or_default();
    base.join("memory").join("chat.jsonl")
}

/// Appends one entry to `path`. Best-effort: a read-only or missing directory
/// is silently tolerated so the chat loop never breaks on a logging failure.
fn record_at(path: &Path, kind: &str, text: &str) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let entry = Entry {
        ts: now(),
        kind: kind.to_owned(),
        text: text.to_owned(),
    };
    if let Ok(line) = serde_json::to_string(&entry) {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(f, "{line}");
        }
    }
}

/// Reads every entry from `path` (skipping malformed lines).
fn load_at(path: &Path) -> Vec<Entry> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    raw.lines()
        .filter_map(|l| serde_json::from_str::<Entry>(l).ok())
        .collect()
}

/// Rewrites `path` keeping only entries the predicate returns `true` for.
/// Returns how many entries were removed.
fn retain_at(path: &Path, keep: impl Fn(&Entry) -> bool) -> usize {
    let all = load_at(path);
    let before = all.len();
    let kept: Vec<Entry> = all.into_iter().filter(|e| keep(e)).collect();
    let removed = before - kept.len();
    if removed == 0 {
        return 0;
    }
    let body: String = kept
        .iter()
        .filter_map(|e| serde_json::to_string(e).ok())
        .map(|l| l + "\n")
        .collect();
    let _ = std::fs::write(path, body);
    removed
}

/// The saved facts in `path`, oldest first.
fn facts_at(path: &Path) -> Vec<String> {
    load_at(path)
        .into_iter()
        .filter(|e| e.kind == "fact")
        .map(|e| e.text)
        .collect()
}

/// Builds a bounded system-prompt addendum from the facts in `path`.
fn context_block_at(path: &Path) -> Option<String> {
    let facts = facts_at(path);
    if facts.is_empty() {
        return None;
    }
    let mut block = String::from(
        "Here is what you remember about this user from past sessions. \
         Use it to personalize your help:\n",
    );
    for f in facts.iter().rev().take(40).rev() {
        block.push_str("- ");
        block.push_str(f);
        block.push('\n');
    }
    Some(block)
}

// --- Public API over the real ~/.cyrene log ---------------------------------

/// Records every user message (`input` kind).
pub fn record_input(text: &str) {
    record_at(&log_path(), "input", text);
}

/// Saves a durable fact (`fact` kind) recalled into future sessions.
pub fn record_fact(text: &str) {
    record_at(&log_path(), "fact", text);
}

/// The remembered facts, oldest first.
#[must_use]
pub fn facts() -> Vec<String> {
    facts_at(&log_path())
}

/// A system-prompt addendum built from saved facts, or `None` if there are none.
#[must_use]
pub fn context_block() -> Option<String> {
    context_block_at(&log_path())
}

/// Forgets all saved *facts* (keeps the raw input log). Returns count removed.
pub fn forget_facts() -> usize {
    retain_at(&log_path(), |e| e.kind != "fact")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facts_round_trip_and_build_context() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chat.jsonl");
        assert!(context_block_at(&path).is_none());
        record_at(&path, "input", "just a message");
        record_at(&path, "fact", "the user lives in Yogyakarta");
        record_at(&path, "fact", "prefers cheap flights via KUL");
        assert_eq!(facts_at(&path).len(), 2);
        let ctx = context_block_at(&path).unwrap();
        assert!(ctx.contains("Yogyakarta"));
        assert!(ctx.contains("KUL"));
    }

    #[test]
    fn forget_facts_keeps_inputs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chat.jsonl");
        record_at(&path, "input", "hello");
        record_at(&path, "fact", "remember me");
        assert_eq!(retain_at(&path, |e| e.kind != "fact"), 1);
        assert!(facts_at(&path).is_empty());
        assert_eq!(
            load_at(&path).iter().filter(|e| e.kind == "input").count(),
            1
        );
    }

    #[test]
    fn blank_input_is_not_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chat.jsonl");
        record_at(&path, "input", "   ");
        assert!(load_at(&path).is_empty());
    }
}
