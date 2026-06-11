//! In-chat slash-command registry and live suggestions.
//!
//! A single source of truth for every `/command` the chat REPL understands,
//! used three ways:
//!
//! - rendering `/help`,
//! - showing a suggestion menu the moment the user types a bare `/` (the
//!   Claude/Hermes-style "what can I type here?" affordance), and
//! - turning a typo (`/modl`) into "did you mean /model?" instead of a flat
//!   "unknown command".
//!
//! The REPL is a plain line reader (no raw-mode TUI), so suggestions are
//! printed when the user submits a `/`-prefixed line rather than rendered as a
//! live dropdown — same intent, no terminal-handling risk.

/// One slash command: its canonical name, accepted aliases, a one-line summary,
/// the argument hint shown in suggestions, and the help group it belongs to.
pub struct SlashCommand {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub summary: &'static str,
    /// Argument placeholder shown after the name (empty when it takes none).
    pub arg: &'static str,
    pub group: Group,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Chat,
    Models,
    Python,
    Integrations,
    Memory,
    Channels,
    Info,
}

impl Group {
    fn title(self) -> &'static str {
        match self {
            Group::Chat => "Chat",
            Group::Models => "Models",
            Group::Python => "Python",
            Group::Integrations => "Integrations & automation",
            Group::Memory => "Memory",
            Group::Channels => "Channels",
            Group::Info => "Info & tools",
        }
    }

    /// Display order for help/suggestions.
    const ORDER: &'static [Group] = &[
        Group::Chat,
        Group::Models,
        Group::Python,
        Group::Integrations,
        Group::Memory,
        Group::Channels,
        Group::Info,
    ];
}

impl SlashCommand {
    /// Does `token` (a command word without its leading `/`) name this command?
    fn matches_exact(&self, token: &str) -> bool {
        self.name == token || self.aliases.contains(&token)
    }

    /// Does this command start with `prefix` (for as-you-type suggestions)?
    fn starts_with(&self, prefix: &str) -> bool {
        self.name.starts_with(prefix) || self.aliases.iter().any(|a| a.starts_with(prefix))
    }
}

/// The full command table. Keep this in sync with the `match cmd` arms in
/// `run_chat`: every entry here should have a handler there, and vice versa.
pub const COMMANDS: &[SlashCommand] = &[
    // Chat
    SlashCommand {
        name: "clear",
        aliases: &["new", "reset"],
        summary: "Start a fresh conversation",
        arg: "",
        group: Group::Chat,
    },
    SlashCommand {
        name: "retry",
        aliases: &[],
        summary: "Re-run your last message",
        arg: "",
        group: Group::Chat,
    },
    SlashCommand {
        name: "undo",
        aliases: &[],
        summary: "Remove the last exchange",
        arg: "",
        group: Group::Chat,
    },
    SlashCommand {
        name: "history",
        aliases: &[],
        summary: "Show the transcript",
        arg: "",
        group: Group::Chat,
    },
    SlashCommand {
        name: "save",
        aliases: &[],
        summary: "Save the transcript to JSON",
        arg: "",
        group: Group::Chat,
    },
    SlashCommand {
        name: "copy",
        aliases: &[],
        summary: "Copy the last reply to the clipboard",
        arg: "",
        group: Group::Chat,
    },
    // Models
    SlashCommand {
        name: "models",
        aliases: &["providers"],
        summary: "List configured providers (● = active)",
        arg: "",
        group: Group::Models,
    },
    SlashCommand {
        name: "model",
        aliases: &["use", "switch"],
        summary: "Switch provider/model (no arg opens a picker)",
        arg: "[name]",
        group: Group::Models,
    },
    SlashCommand {
        name: "connect",
        aliases: &[],
        summary: "Add or update a provider + API key",
        arg: "",
        group: Group::Models,
    },
    SlashCommand {
        name: "reload",
        aliases: &[],
        summary: "Re-read ~/.cyrene/.env",
        arg: "",
        group: Group::Models,
    },
    // Python
    SlashCommand {
        name: "py",
        aliases: &["python"],
        summary: "Run inline Python code",
        arg: "<code>",
        group: Group::Python,
    },
    SlashCommand {
        name: "run",
        aliases: &[],
        summary: "Run a saved script or .py file",
        arg: "<name|file.py>",
        group: Group::Python,
    },
    SlashCommand {
        name: "autorun",
        aliases: &[],
        summary: "Toggle auto-running Python from replies",
        arg: "",
        group: Group::Python,
    },
    // Integrations & automation
    SlashCommand {
        name: "key",
        aliases: &[],
        summary: "Save an API key/secret to .env for scripts",
        arg: "NAME value",
        group: Group::Integrations,
    },
    SlashCommand {
        name: "script",
        aliases: &[],
        summary: "Save Cyrene's last Python as a named script",
        arg: "<name>",
        group: Group::Integrations,
    },
    SlashCommand {
        name: "scripts",
        aliases: &[],
        summary: "List saved scripts",
        arg: "",
        group: Group::Integrations,
    },
    SlashCommand {
        name: "cron",
        aliases: &[],
        summary: "Schedule a saved script to report on a timer",
        arg: "<name> <script> <when> [chan]",
        group: Group::Integrations,
    },
    // Memory
    SlashCommand {
        name: "remember",
        aliases: &["note"],
        summary: "Save a fact Cyrene should keep in mind",
        arg: "<fact>",
        group: Group::Memory,
    },
    SlashCommand {
        name: "memories",
        aliases: &["recall"],
        summary: "Show what Cyrene remembers",
        arg: "",
        group: Group::Memory,
    },
    SlashCommand {
        name: "forget",
        aliases: &[],
        summary: "Clear saved memories",
        arg: "",
        group: Group::Memory,
    },
    // Channels
    SlashCommand {
        name: "telegram",
        aliases: &[],
        summary: "Connect this chat to Telegram",
        arg: "",
        group: Group::Channels,
    },
    SlashCommand {
        name: "whatsapp",
        aliases: &[],
        summary: "Connect this chat to WhatsApp",
        arg: "",
        group: Group::Channels,
    },
    // Info & tools
    SlashCommand {
        name: "status",
        aliases: &[],
        summary: "Active provider, model, and token counts",
        arg: "",
        group: Group::Info,
    },
    SlashCommand {
        name: "usage",
        aliases: &[],
        summary: "Token usage this session",
        arg: "",
        group: Group::Info,
    },
    SlashCommand {
        name: "verbose",
        aliases: &[],
        summary: "Toggle per-reply token counts",
        arg: "",
        group: Group::Info,
    },
    SlashCommand {
        name: "doctor",
        aliases: &[],
        summary: "Configuration health check",
        arg: "",
        group: Group::Info,
    },
    SlashCommand {
        name: "tools",
        aliases: &[],
        summary: "List built-in tools",
        arg: "",
        group: Group::Info,
    },
    SlashCommand {
        name: "skills",
        aliases: &[],
        summary: "List bundled skills",
        arg: "",
        group: Group::Info,
    },
    SlashCommand {
        name: "version",
        aliases: &[],
        summary: "Show installed vs latest version",
        arg: "",
        group: Group::Info,
    },
    SlashCommand {
        name: "update",
        aliases: &[],
        summary: "Update Cyrene to the latest release",
        arg: "",
        group: Group::Info,
    },
    SlashCommand {
        name: "fortune",
        aliases: &[],
        summary: "A little encouragement",
        arg: "",
        group: Group::Info,
    },
    SlashCommand {
        name: "help",
        aliases: &["h", "?"],
        summary: "Show this help",
        arg: "",
        group: Group::Info,
    },
    SlashCommand {
        name: "exit",
        aliases: &["quit", "q"],
        summary: "Leave the chat",
        arg: "",
        group: Group::Info,
    },
];

/// Looks up a command by its name or any alias.
#[must_use]
pub fn lookup(token: &str) -> Option<&'static SlashCommand> {
    COMMANDS.iter().find(|c| c.matches_exact(token))
}

/// Commands whose name or an alias begins with `prefix`, in table order.
#[must_use]
pub fn suggestions(prefix: &str) -> Vec<&'static SlashCommand> {
    if prefix.is_empty() {
        return COMMANDS.iter().collect();
    }
    COMMANDS.iter().filter(|c| c.starts_with(prefix)).collect()
}

/// Renders one `  /name <arg>   — summary` line.
fn render_line(c: &SlashCommand) -> String {
    let head = if c.arg.is_empty() {
        format!("/{}", c.name)
    } else {
        format!("/{} {}", c.name, c.arg)
    };
    format!("    {head:<22} {}", c.summary)
}

/// Prints the grouped command help (`/help`).
pub fn print_help() {
    println!("\nCommands:");
    for &group in Group::ORDER {
        println!("  {}", group.title());
        for c in COMMANDS.iter().filter(|c| c.group == group) {
            println!("{}", render_line(c));
        }
    }
    println!("\nTip: type `/` and press Enter to see suggestions any time.\n");
}

/// Prints the suggestion menu for a (possibly empty) `prefix`. Used when the
/// user submits a bare `/` or a partial command. Returns the number shown.
pub fn print_suggestions(prefix: &str) -> usize {
    let matches = suggestions(prefix);
    if matches.is_empty() {
        return 0;
    }
    if prefix.is_empty() {
        println!("\nType a command (or keep typing to filter):");
    } else {
        println!("\n/{prefix}… —did you mean:");
    }
    for c in &matches {
        println!("{}", render_line(c));
    }
    println!();
    matches.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_slash_lists_everything() {
        assert_eq!(suggestions("").len(), COMMANDS.len());
    }

    #[test]
    fn prefix_filters_by_name_and_alias() {
        let names: Vec<_> = suggestions("mo").iter().map(|c| c.name).collect();
        assert!(names.contains(&"model"));
        assert!(names.contains(&"models"));
        // `recall` is an alias of `memories`; prefix "rec" should surface it.
        let recall: Vec<_> = suggestions("rec").iter().map(|c| c.name).collect();
        assert!(recall.contains(&"memories"));
    }

    #[test]
    fn lookup_resolves_aliases() {
        assert_eq!(lookup("new").unwrap().name, "clear");
        assert_eq!(lookup("python").unwrap().name, "py");
        assert_eq!(lookup("q").unwrap().name, "exit");
        assert!(lookup("nope").is_none());
    }

    #[test]
    fn no_duplicate_names_or_aliases() {
        let mut seen = std::collections::HashSet::new();
        for c in COMMANDS {
            assert!(seen.insert(c.name), "duplicate command name {}", c.name);
            for a in c.aliases {
                assert!(seen.insert(a), "alias {a} collides with another command");
            }
        }
    }
}
