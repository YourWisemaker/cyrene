//! The shared conversational agent core.
//!
//! Cyrene must behave the same no matter how you reach her — the interactive
//! CLI, Telegram, WhatsApp, or any future channel. That means one set of rules
//! for: the persona/system prompt, writing-and-running Python integrations, and
//! the self-learning actions (`remember`, `remember_user`, saving skills,
//! scheduling cron jobs). This module is that single source of truth.
//!
//! The interactive REPL drives its own loop (it has a TTY, slash commands, and
//! per-step consent prompts), but it builds its system prompt from
//! [`system_prompt`] here so identity and memory stay identical. The
//! non-interactive messaging channels call [`run_turn`], which runs the *whole*
//! agent loop for one inbound message and returns the text to send back.

use std::sync::Arc;
use std::time::Duration;

use cyrene_core::{ChatMessage, Model, ModelRequest};

use crate::{actions, chatmem, crons, persona, pyexec};

/// How long a model-written Python block may run inside a messaging turn.
const PY_TIMEOUT: Duration = Duration::from_secs(120);

/// Builds Cyrene's system prompt: the persona (`~/.cyrene/SOUL.md` or the
/// built-in default) plus what she remembers — durable task facts and her
/// evolving model of the user. Every channel seeds its conversation with this,
/// so identity and memory are identical across CLI, Telegram, and WhatsApp.
#[must_use]
pub fn system_prompt() -> String {
    let mut s = persona::stable_block();
    if let Some(mem) = chatmem::context_block() {
        s.push_str("\n\n");
        s.push_str(&mem);
    }
    if let Some(profile) = chatmem::user_profile_block() {
        s.push_str("\n\n");
        s.push_str(&profile);
    }
    s
}

/// Runs one full agent turn for a non-interactive channel.
///
/// `history` must already hold the system prompt at index 0 and end with the
/// new user message. `origin_channel` is where a reply naturally goes back
/// (e.g. `"telegram:12345"`, `"whatsapp:628…"`); it becomes the default
/// delivery target for any cron the model schedules, so "report me X every
/// morning" lands back in the same chat.
///
/// The turn: call the model → if it wrote Python, run it and feed the output
/// back for interpretation → carry out the self-learning actions it emitted →
/// return the text to send the user (with a short note of what was learned or
/// scheduled).
pub async fn run_turn(
    model: &Arc<dyn Model>,
    history: &mut Vec<ChatMessage>,
    origin_channel: &str,
) -> String {
    let first = match complete(model, history).await {
        Ok(reply) => reply,
        Err(e) => return format!("Sorry, I hit an error reaching my model: {e}"),
    };

    // If Cyrene wrote Python, run it (messaging channels auto-run — there is no
    // TTY to ask) and feed the output back so she can interpret the result.
    let mut final_reply = first.clone();
    let blocks = pyexec::extract_python_blocks(&first);
    if !blocks.is_empty() {
        let mut combined = String::new();
        for block in &blocks {
            match pyexec::run_code(block, PY_TIMEOUT) {
                Ok(outcome) => {
                    combined.push_str(&outcome.summary());
                    combined.push('\n');
                }
                Err(e) => combined.push_str(&format!("[error] {e}\n")),
            }
        }
        history.push(ChatMessage::user(format!(
            "I ran the Python you wrote. Here is the output:\n{combined}\n\
             Briefly interpret the result and say what to do next."
        )));
        if let Ok(reply2) = complete(model, history).await {
            final_reply = reply2;
        }
    }

    // Carry out the self-learning actions from the original reply.
    let notes = apply_actions(&first, origin_channel, history);

    if notes.is_empty() {
        final_reply
    } else {
        format!("{final_reply}\n\n{}", notes.join("\n"))
    }
}

/// Applies the actions Cyrene emitted (`remember`, `remember_user`, named
/// skills, `schedule`) for a non-interactive channel and returns short,
/// user-facing notes describing what happened. Rebuilds the system prompt in
/// place when memory changed so later turns see it immediately.
fn apply_actions(reply: &str, origin_channel: &str, history: &mut [ChatMessage]) -> Vec<String> {
    let mut notes = Vec::new();
    let mut memory_changed = false;

    for act in actions::parse(reply) {
        match act {
            actions::Action::Remember(fact) => {
                chatmem::record_fact(&fact);
                memory_changed = true;
            }
            actions::Action::RememberUser(note) => {
                chatmem::record_profile(&note);
                memory_changed = true;
            }
            actions::Action::LearnSkill { name, code } => match pyexec::save_script(&name, &code) {
                Ok(path) => {
                    let stem = pyexec::sanitize_name(&name).unwrap_or_default();
                    chatmem::record_fact(&format!(
                        "has a saved skill `{stem}` at {}",
                        path.display()
                    ));
                    memory_changed = true;
                    notes.push(format!("💾 Saved a reusable skill `{stem}`."));
                }
                Err(e) => notes.push(format!("⚠️ Couldn't save skill `{name}`: {e}")),
            },
            actions::Action::Schedule {
                name,
                script,
                schedule,
                channel,
            } => {
                // The user explicitly asked for the recurring report, so on a
                // messaging channel we create it (no TTY to confirm) and send
                // it back to the chat they asked from by default.
                let target = resolve_schedule_channel(&channel, origin_channel);
                match crons::add(&name, &schedule, &script, &target) {
                    Ok(()) => notes.push(format!(
                        "⏰ Scheduled `{name}` ({schedule}) — I'll deliver it here while \
                         this session is running. Say \"cancel {name}\" or run \
                         `cyrene cron remove {name}` to stop."
                    )),
                    Err(e) => notes.push(format!("⚠️ Couldn't schedule `{name}`: {e}")),
                }
            }
        }
    }

    if memory_changed {
        if let Some(slot) = history.get_mut(0) {
            *slot = ChatMessage::system(system_prompt());
        }
    }
    notes
}

/// Picks the cron delivery channel: honor an explicit, real target the model
/// chose, otherwise fall back to where the message came from. A placeholder
/// (`<chat_id>`), an empty value, or `cli` all defer to the origin so a
/// scheduled report goes back to the user who asked for it.
fn resolve_schedule_channel(model_channel: &str, origin: &str) -> String {
    let c = model_channel.trim();
    if c.is_empty() || c.eq_ignore_ascii_case("cli") || c.contains('<') {
        return origin.to_owned();
    }
    c.to_owned()
}

/// One model call against `history`. On success pushes the assistant reply and
/// returns it; on error drops the trailing user turn so a retry starts clean.
async fn complete(
    model: &Arc<dyn Model>,
    history: &mut Vec<ChatMessage>,
) -> Result<String, String> {
    use cyrene_core::Role;

    match model.complete(ModelRequest::new(history.clone())).await {
        Ok(resp) => {
            let reply = resp.content.trim().to_owned();
            let reply = if reply.is_empty() {
                "(no response)".to_owned()
            } else {
                reply
            };
            history.push(ChatMessage::assistant(reply.clone()));
            Ok(reply)
        }
        Err(e) => {
            if history.last().map(|m| m.role) == Some(Role::User) {
                history.pop();
            }
            Err(e.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_real_channel_is_honored() {
        assert_eq!(
            resolve_schedule_channel("discord", "telegram:12345"),
            "discord"
        );
        assert_eq!(
            resolve_schedule_channel("telegram:999", "telegram:12345"),
            "telegram:999"
        );
    }

    #[test]
    fn placeholder_or_cli_defers_to_origin() {
        assert_eq!(
            resolve_schedule_channel("telegram:<chat_id>", "telegram:12345"),
            "telegram:12345"
        );
        assert_eq!(
            resolve_schedule_channel("cli", "whatsapp:628"),
            "whatsapp:628"
        );
        assert_eq!(resolve_schedule_channel("", "telegram:1"), "telegram:1");
    }

    #[test]
    fn system_prompt_carries_persona() {
        let p = system_prompt();
        assert!(p.contains("Cyrene"));
        assert!(p.contains("remember_user:"));
    }
}
