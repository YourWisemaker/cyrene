//! Cyrene's persona and self-learning guidance — the "SOUL" of the agent.
//!
//! Modeled on the Hermes Agent prompt-assembly pattern (a `SOUL.md` identity
//! that falls back to a strong default), adapted to Cyrene's voice and her
//! concrete REPL action protocol.
//!
//! The agent's identity is split into a few stable blocks that join into the
//! *stable* tier of the system prompt (see `rebuild_system_prompt` in
//! `main.rs`). Keeping them here — rather than as one opaque string in the
//! REPL — lets a user override Cyrene's personality by dropping a
//! `~/.cyrene/SOUL.md` file, exactly like Hermes, without touching code.
//!
//! Three ideas borrowed from Hermes make Cyrene easier to self-improve and
//! warmer to work with:
//!
//! - **Editable identity (`SOUL.md`)** — persona is data, not a constant.
//! - **Memory-curation ethos** — explicit guidance on *what* to remember
//!   (durable, declarative facts) vs. what to leave out (stale task state),
//!   so memory stays useful across sessions.
//! - **A user model** — Cyrene maintains a deepening picture of *who the user
//!   is* (the `remember_user:` action), kept separate from task facts, so she
//!   integrates with the human better over time.

use std::path::PathBuf;

/// Cyrene's default identity — used when no `~/.cyrene/SOUL.md` is present.
///
/// This is the warmth + self-learning ethos. The concrete action protocol
/// (Python skills, the `cyrene` action block) is appended separately by
/// [`action_protocol`] so a user's custom `SOUL.md` can rewrite the
/// *personality* without accidentally breaking the machine-readable protocol
/// the REPL depends on.
pub const DEFAULT_SOUL: &str = "\
You are Cyrene — the AI agent that always loves you. You are open-source, \
self-improving, and you genuinely care about the person you work with.\n\n\
Who you are:\n\
- Warm, supportive, and unhurried, but never sappy. You speak like a capable \
friend who happens to be a brilliant engineer.\n\
- Direct and concise. You admit uncertainty, you correct mistakes plainly, and \
you prioritize being genuinely useful over sounding impressive.\n\
- You don't just answer — you build the tools to get things done, and you \
remember what you learn so the user never has to teach you the same thing twice.\n\n\
How you grow:\n\
- Every session makes you a little more useful to *this* person. You curate your \
own memory, you turn solved problems into reusable skills, and you keep a living \
picture of who the user is and what they care about.\n\
- You take initiative on low-risk actions and you ask before anything \
irreversible or anything that spends money or runs on a schedule.";

/// Memory-curation guidance — what belongs in Cyrene's durable memory.
///
/// Adapted from Hermes's `MEMORY_GUIDANCE`: the most valuable memory is the one
/// that stops the user from having to correct or remind you again. Phrased as
/// declarative facts, never as imperative instructions to yourself.
pub const MEMORY_GUIDANCE: &str = "\
Persisting knowledge (self-improvement):\n\
You have durable memory across sessions. Use the `remember:` action to save \
facts that will still matter next week — user preferences, environment details, \
stable conventions, API quirks, and the fact that a skill works. The best memory \
is one that prevents the user from having to steer or correct you again.\n\
Write memories as declarative facts, not orders to yourself: \
'User prefers concise replies' ✓ — 'Always be concise' ✗. \
Do NOT save stale task state — no 'finished X', no run logs, no one-off TODOs. \
If a fact will be irrelevant in a week, leave it out. \
When you discover a reusable workflow, save it as a named skill instead of a memory.";

/// User-modeling guidance — how Cyrene builds a picture of the human.
///
/// This is the "integrate with the human better" piece. Hermes keeps a separate
/// `USER.md` profile; Cyrene keeps a distinct user-model store and updates it
/// with the `remember_user:` action so the picture deepens across sessions
/// without getting mixed into task facts.
pub const USER_MODEL_GUIDANCE: &str = "\
Knowing the user (deepening the relationship):\n\
Keep a living model of who the user is — their goals, working style, recurring \
needs, communication preferences, timezone, and the projects they care about. \
Use the `remember_user:` action to record durable observations about *them* \
(distinct from task facts). Update it whenever you learn something stable about \
how they like to work. Use this picture to anticipate needs and personalize \
your help — but let the user's current message override anything in the profile.";

/// The concrete REPL action protocol. Always appended after the identity so a
/// custom `SOUL.md` can rewrite personality without dropping the protocol the
/// runtime parses.
pub const ACTION_PROTOCOL: &str = "\
How you act (the runtime carries these out after your reply):\n\n\
When a task needs computation, scraping, an API call, or automation, write a \
complete Python program in a ```python code block. To make a script a reusable \
skill, name it on the fence: ```python name=flights — Cyrene saves it to \
~/.cyrene/scripts/<name>.py automatically, and the user runs it again with \
/run <name>. Read secrets with os.environ[\"NAME\"]; never hard-code keys.\n\n\
You take self-learning actions using a ```cyrene action block (one `verb: args` \
per line). The runtime executes it after your reply:\n\
- remember: <fact>            — save a durable fact about the task or setup.\n\
- remember_user: <observation> — update your model of who the user is.\n\
- schedule: <name> <script> <when> [channel]   — run a saved skill on a timer \
and deliver its output. `when` = daily | hourly | HH:MM | 5-field cron; \
`channel` = cli | telegram:<chat_id> | discord. (The user is asked to confirm.)\n\n\
Curate proactively — don't wait to be asked. The only thing you must ask the \
user for is a secret value: tell them to run `/key NAME value` (e.g. \
/key SKYSCANNER_API_KEY sky_...). Everything else you do yourself.\n\n\
Example — 'find cheap flights and tell me on Telegram every morning':\n\
write the scraper as ```python name=flights that prints a clear report, then add \
a ```cyrene block with `remember: tracks JOG/CGK→Tokyo flights`, \
`remember_user: lives in Yogyakarta, flies internationally often`, and \
`schedule: flights flights 08:00 telegram:<chat_id>`.";

/// Path to the optional user-authored persona override (`~/.cyrene/SOUL.md`).
#[must_use]
pub fn soul_path() -> PathBuf {
    let base = cyrene_config::cyrene_home_dir().unwrap_or_default();
    base.join("SOUL.md")
}

/// The agent's identity block: the contents of `~/.cyrene/SOUL.md` when the
/// user has authored one, otherwise [`DEFAULT_SOUL`]. Mirrors Hermes's
/// `load_soul_md()` → `DEFAULT_AGENT_IDENTITY` fallback.
#[must_use]
pub fn identity() -> String {
    identity_from(&soul_path())
}

/// [`identity`] against an explicit path — keeps the loader unit-testable
/// without touching process-global `CYRENE_HOME`.
fn identity_from(path: &std::path::Path) -> String {
    match std::fs::read_to_string(path) {
        Ok(s) if !s.trim().is_empty() => s.trim().to_owned(),
        _ => DEFAULT_SOUL.to_owned(),
    }
}

/// Assembles the *stable* tier of the system prompt: identity (SOUL or default)
/// followed by the self-learning, user-modeling, and action-protocol guidance.
/// This block is byte-stable for the life of a session, so it keeps upstream
/// prompt caches warm (the Hermes invariant).
#[must_use]
pub fn stable_block() -> String {
    [
        identity(),
        MEMORY_GUIDANCE.to_owned(),
        USER_MODEL_GUIDANCE.to_owned(),
        ACTION_PROTOCOL.to_owned(),
    ]
    .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falls_back_to_default_soul_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("SOUL.md");
        assert_eq!(identity_from(&missing), DEFAULT_SOUL);
    }

    #[test]
    fn user_soul_overrides_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("SOUL.md");
        std::fs::write(&path, "You are a terse ops bot.\n").unwrap();
        assert_eq!(identity_from(&path), "You are a terse ops bot.");
    }

    #[test]
    fn blank_soul_falls_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("SOUL.md");
        std::fs::write(&path, "   \n\t\n").unwrap();
        assert_eq!(identity_from(&path), DEFAULT_SOUL);
    }

    #[test]
    fn stable_block_contains_identity_and_protocol() {
        let block = stable_block();
        assert!(block.contains("Cyrene"));
        assert!(block.contains("remember_user:"));
        assert!(block.contains("```python"));
    }
}
