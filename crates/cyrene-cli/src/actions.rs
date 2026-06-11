//! The self-learning action protocol.
//!
//! This is what makes Cyrene act like a learning agent rather than a chat box:
//! she can take actions *in her replies*, and the REPL carries them out without
//! the user typing a single slash command. Two surfaces:
//!
//! 1. **Named skills** — a fenced ` ```python name=<skill> ` block means "save
//!    this as a reusable integration." The REPL persists it under
//!    `~/.cyrene/scripts/<skill>.py` and remembers that it exists.
//!
//! 2. **A `cyrene` action block** — a fenced ` ```cyrene ` block of `verb: args`
//!    lines that Cyrene uses to curate her own memory and schedule work:
//!
//!    ```text
//!    ```cyrene
//!    remember: the user's home airport is JOG
//!    schedule: flights flights 08:00 telegram:123456789
//!    ```
//!    ```
//!
//! Parsing is intentionally forgiving — unknown verbs and malformed lines are
//! skipped, never fatal — so a model that improvises slightly still does
//! something sensible. Execution (and any consent gating) lives in the REPL;
//! this module only turns text into a list of intentions.

use crate::pyexec;

/// One thing Cyrene decided to do, parsed from her reply.
#[derive(Debug, PartialEq)]
pub enum Action {
    /// Persist a durable memory fact (agent-curated memory).
    Remember(String),
    /// Persist a durable observation about who the user is (user model).
    RememberUser(String),
    /// Save a named Python block as a reusable skill/integration.
    LearnSkill { name: String, code: String },
    /// Schedule a saved script to run and report on a timer.
    Schedule {
        name: String,
        script: String,
        schedule: String,
        channel: String,
    },
    /// Schedule a recurring *agent task*: a natural-language prompt run through
    /// a full agent turn on a timer (thinks, runs tools, learns), not a static
    /// script. The delivery channel defaults to where it was set up.
    ScheduleAgent {
        name: String,
        schedule: String,
        prompt: String,
        channel: String,
    },
}

/// Parses every action out of a model reply, in document order.
#[must_use]
pub fn parse(reply: &str) -> Vec<Action> {
    let mut actions = Vec::new();

    // 1. Named Python blocks become skills.
    for block in pyexec::extract_python_block_meta(reply) {
        if let Some(name) = block.name {
            actions.push(Action::LearnSkill {
                name,
                code: block.code,
            });
        }
    }

    // 2. `cyrene` action blocks carry memory + scheduling directives.
    for body in extract_blocks(reply, "cyrene") {
        for line in body.lines() {
            if let Some(action) = parse_directive(line) {
                actions.push(action);
            }
        }
    }

    actions
}

/// Parses a single `verb: args` directive line into an [`Action`].
fn parse_directive(line: &str) -> Option<Action> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let (verb, rest) = line.split_once(':')?;
    let rest = rest.trim();
    match verb.trim().to_lowercase().as_str() {
        "remember" | "note" | "learn" if !rest.is_empty() => {
            Some(Action::Remember(rest.to_owned()))
        }
        "remember_user" | "remember-user" | "user" | "profile" if !rest.is_empty() => {
            Some(Action::RememberUser(rest.to_owned()))
        }
        "schedule" | "cron" => {
            // `<name> <script> <when> [channel]`. The `when` keyword forms are
            // single tokens (daily/hourly/HH:MM), which keeps this splittable.
            let mut p = rest.splitn(4, char::is_whitespace);
            let name = p.next().unwrap_or("").trim();
            let script = p.next().unwrap_or("").trim();
            let when = p.next().unwrap_or("").trim();
            let channel = p.next().unwrap_or("").trim();
            if name.is_empty() || script.is_empty() || when.is_empty() {
                return None;
            }
            Some(Action::Schedule {
                name: name.to_owned(),
                script: script.to_owned(),
                schedule: when.to_owned(),
                channel: if channel.is_empty() {
                    "cli".to_owned()
                } else {
                    channel.to_owned()
                },
            })
        }
        "automate" | "agenda" => {
            // Pipe-delimited so the prompt can be free text:
            // `<name> | <when> | <prompt> [| <channel>]`. The channel is
            // optional and defaults to wherever the task was set up.
            let mut p = rest.splitn(4, '|');
            let name = p.next().unwrap_or("").trim();
            let when = p.next().unwrap_or("").trim();
            let prompt = p.next().unwrap_or("").trim();
            let channel = p.next().unwrap_or("").trim();
            if name.is_empty() || when.is_empty() || prompt.is_empty() {
                return None;
            }
            Some(Action::ScheduleAgent {
                name: name.to_owned(),
                schedule: when.to_owned(),
                prompt: prompt.to_owned(),
                channel: channel.to_owned(),
            })
        }
        _ => None,
    }
}

/// Extracts the bodies of fenced blocks whose info string starts with `lang`
/// (e.g. ` ```cyrene `). Generic sibling of the Python extractor.
fn extract_blocks(text: &str, lang: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let fence = line.trim_start();
        let info = fence
            .strip_prefix("```")
            .or_else(|| fence.strip_prefix("~~~"));
        let Some(info) = info else { continue };
        if info.split_whitespace().next() != Some(lang) {
            continue;
        }
        let mut body = String::new();
        for inner in lines.by_ref() {
            let t = inner.trim_start();
            if t.starts_with("```") || t.starts_with("~~~") {
                break;
            }
            body.push_str(inner);
            body.push('\n');
        }
        blocks.push(body);
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_named_skill() {
        let reply = "Here's the integration:\n```python name=weather\nprint('sunny')\n```";
        let actions = parse(reply);
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            Action::LearnSkill { name, code } => {
                assert_eq!(name, "weather");
                assert!(code.contains("sunny"));
            }
            other => panic!("expected LearnSkill, got {other:?}"),
        }
    }

    #[test]
    fn parses_remember_and_schedule_from_cyrene_block() {
        let reply = "All set!\n\n```cyrene\n# self-notes\nremember: user prefers cheap flights via KUL\nschedule: flights flights 08:00 telegram:123\n```\n";
        let actions = parse(reply);
        assert_eq!(
            actions,
            vec![
                Action::Remember("user prefers cheap flights via KUL".to_owned()),
                Action::Schedule {
                    name: "flights".to_owned(),
                    script: "flights".to_owned(),
                    schedule: "08:00".to_owned(),
                    channel: "telegram:123".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn schedule_defaults_channel_to_cli_and_skips_incomplete() {
        let reply = "```cyrene\nschedule: brief brief daily\nschedule: broken\n```";
        let actions = parse(reply);
        assert_eq!(
            actions,
            vec![Action::Schedule {
                name: "brief".to_owned(),
                script: "brief".to_owned(),
                schedule: "daily".to_owned(),
                channel: "cli".to_owned(),
            }]
        );
    }

    #[test]
    fn parses_remember_user_into_profile_action() {
        let reply =
            "```cyrene\nremember_user: the user is a Rust engineer who prefers terse replies\n```";
        let actions = parse(reply);
        assert_eq!(
            actions,
            vec![Action::RememberUser(
                "the user is a Rust engineer who prefers terse replies".to_owned()
            )]
        );
    }

    #[test]
    fn parses_automate_into_scheduled_agent_task() {
        let reply = "```cyrene\nautomate: crypto | 17:00 | Summarize new posts in r/crypto and post an update\n```";
        let actions = parse(reply);
        assert_eq!(
            actions,
            vec![Action::ScheduleAgent {
                name: "crypto".to_owned(),
                schedule: "17:00".to_owned(),
                prompt: "Summarize new posts in r/crypto and post an update".to_owned(),
                channel: String::new(),
            }]
        );
    }

    #[test]
    fn unnamed_python_and_unknown_verbs_are_ignored() {
        let reply = "```python\nprint(1)\n```\n```cyrene\nfly: to the moon\n```";
        assert!(parse(reply).is_empty());
    }

    #[test]
    fn combines_skill_then_schedule() {
        let reply = "```python name=flights\nprint('report')\n```\n```cyrene\nschedule: flights flights 08:00 cli\n```";
        let actions = parse(reply);
        assert_eq!(actions.len(), 2);
        assert!(matches!(actions[0], Action::LearnSkill { .. }));
        assert!(matches!(actions[1], Action::Schedule { .. }));
    }
}
