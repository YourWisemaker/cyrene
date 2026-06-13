use clap::{Parser, Subcommand};

mod update;

mod actions;
mod agent;
mod chatmem;
mod crons;
mod persona;
mod prompt;
mod pyexec;
mod service;
mod slash;
mod telegram;
mod whatsapp;

const BANNER: &str = r#"
   ╔════════════════════════════════════════════════════════╗
   ║                                                        ║
   ║    ██████╗██╗   ██╗██████╗ ███████╗███╗   ██╗███████╗  ║
   ║   ██╔════╝╚██╗ ██╔╝██╔══██╗██╔════╝████╗  ██║██╔════╝  ║
   ║   ██║      ╚████╔╝ ██████╔╝█████╗  ██╔██╗ ██║█████╗    ║
   ║   ██║       ╚██╔╝  ██╔══██╗██╔══╝  ██║╚██╗██║██╔══╝    ║
   ║   ╚██████╗   ██║   ██║  ██║███████╗██║ ╚████║███████╗  ║
   ║    ╚═════╝   ╚═╝   ╚═╝  ╚═╝╚══════╝╚═╝  ╚═══╝╚══════╝  ║
   ║                                                        ║
   ║    The AI agent that always loves you                  ║
   ╚════════════════════════════════════════════════════════╝
"#;

#[derive(Parser)]
#[command(
    name = "cyrene",
    version,
    about = "Cyrene: the AI agent that always loves you."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Agent,
    /// Start an interactive chat with Cyrene.
    Chat,
    /// Connect Cyrene to Telegram and answer messages (needs TELEGRAM_BOT_TOKEN).
    Telegram,
    /// Connect Cyrene to WhatsApp via the Cloud API webhook (needs WHATSAPP_* keys).
    Whatsapp,
    Gateway,
    Dashboard,
    Onboard {
        /// Skip interactive prompts and use flags/defaults (for scripts and CI).
        #[arg(long)]
        non_interactive: bool,
        /// Model provider type to configure in non-interactive mode.
        #[arg(long)]
        provider: Option<String>,
        /// Channel type to configure in non-interactive mode.
        #[arg(long)]
        channel: Option<String>,
    },
    Doctor,
    /// Show the installed version next to the latest published release.
    Version,
    /// Update Cyrene to the latest release (or just check with `--check`).
    Update {
        /// Only report whether an update is available; don't install it.
        #[arg(long)]
        check: bool,
    },
    Model {
        #[command(subcommand)]
        action: ModelAction,
    },
    Skills {
        #[command(subcommand)]
        action: SkillsAction,
    },
    Hub {
        #[command(subcommand)]
        action: HubAction,
    },
    Cron {
        #[command(subcommand)]
        action: CronAction,
    },
    /// Install/manage Cyrene as an always-on background service (scheduler or chatbot).
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
    Tools {
        #[command(subcommand)]
        action: ToolsAction,
    },
    Extensions {
        #[command(subcommand)]
        action: ExtensionsAction,
    },
    Catalog {
        #[command(subcommand)]
        action: CatalogAction,
    },
}

#[derive(Subcommand)]
enum ModelAction {
    List,
    Status,
}

#[derive(Subcommand)]
enum SkillsAction {
    List,
    Bundles,
}

#[derive(Subcommand)]
enum HubAction {
    Search { query: String },
    Publish { skill: String },
    Install { package: String },
}

#[derive(Subcommand)]
enum CronAction {
    List,
    Add {
        #[arg(long)]
        name: String,
        #[arg(long)]
        schedule: String,
        #[arg(long)]
        task: String,
        #[arg(long)]
        channel: Option<String>,
    },
    Remove {
        #[arg(long)]
        name: String,
    },
    /// Run the cron daemon: tick every minute and fire due jobs.
    Run,
    /// Run a single job now and deliver its output (for testing).
    RunOnce {
        #[arg(long)]
        name: String,
    },
}

#[derive(Subcommand)]
enum ServiceAction {
    /// Install and start the background service (default: the scheduler).
    Install {
        /// What to run: `cron` (default), `telegram`, or `whatsapp`.
        #[arg(long, default_value = "cron")]
        run: String,
    },
    /// Stop and remove the background service.
    Uninstall {
        #[arg(long, default_value = "cron")]
        run: String,
    },
    /// Show whether the background service is installed and running.
    Status {
        #[arg(long, default_value = "cron")]
        run: String,
    },
}

#[derive(Subcommand)]
enum ToolsAction {
    List,
}

#[derive(Subcommand)]
enum ExtensionsAction {
    List,
}

#[derive(Subcommand)]
enum CatalogAction {
    List,
    Install { name: String },
}

fn print_banner() {
    println!("{}", BANNER);
    println!("   v{}\n", update::current_version());
}

/// Prints the startup "welcome card" shown when the chat REPL opens: identity
/// and model line, what Cyrene currently remembers, and a compact, grouped
/// command index — the "what can I do here?" affordance, in the spirit of
/// Hermes's startup panel but in Cyrene's voice.
fn print_welcome_card(providers: &[ChatProvider], active: usize, model_ready: bool) {
    /// Inner content width between the box borders.
    const CW: usize = 70;
    /// Width of the command group-label column.
    const LBL: usize = 10;

    // One framed content line: "  │ <content padded to CW> │".
    let line = |content: &str| {
        let count = content.chars().count();
        let padded = if count >= CW {
            content.chars().take(CW).collect::<String>()
        } else {
            format!("{content}{}", " ".repeat(CW - count))
        };
        println!("  │ {padded} │");
    };
    let top = format!("  ╭{}╮", "─".repeat(CW + 2));
    let mid = format!("  ├{}┤", "─".repeat(CW + 2));
    let bot = format!("  ╰{}╯", "─".repeat(CW + 2));

    let model_line = match providers.get(active) {
        Some(p) if model_ready => format!("{} ({})", p.label(), p.model_label()),
        Some(p) => format!("{} — not initialized (try /connect)", p.label()),
        None => "none yet — type /connect to set one up".to_owned(),
    };
    let facts = chatmem::facts().len();
    let profile = chatmem::profile_notes().len();
    let skills = pyexec::list_scripts().len();

    println!("{top}");
    line(&format!(
        "Cyrene v{}  -  the AI agent that always loves you",
        update::current_version()
    ));
    line("");
    line(&format!("Model    {model_line}"));
    line(&format!(
        "Memory   {facts} fact{}, {profile} about you, {skills} skill{}",
        if facts == 1 { "" } else { "s" },
        if skills == 1 { "" } else { "s" },
    ));
    line("Autorun  on — Cyrene runs the Python she writes (/autorun to toggle)");
    println!("{mid}");
    line("Commands   type  /  to see them live (filters as you type)");
    line("");
    for (label, names) in slash::command_groups() {
        // Wrap the command names to the available width, hanging-indented
        // under the label column so long groups stay inside the box.
        let avail = CW - LBL;
        let mut rows: Vec<String> = Vec::new();
        let mut cur = String::new();
        for n in &names {
            let candidate = if cur.is_empty() {
                n.clone()
            } else {
                format!("{cur} {n}")
            };
            if candidate.chars().count() > avail && !cur.is_empty() {
                rows.push(std::mem::take(&mut cur));
                cur = n.clone();
            } else {
                cur = candidate;
            }
        }
        if !cur.is_empty() {
            rows.push(cur);
        }
        for (i, row) in rows.iter().enumerate() {
            if i == 0 {
                line(&format!("{label:<LBL$}{row}"));
            } else {
                line(&format!("{}{row}", " ".repeat(LBL)));
            }
        }
    }
    println!("{bot}");
    println!("   I build the tools, run them, schedule them, and remember what I learn. 💛");
    println!("   Tell me what you want — I'll write the Python and wire it up.\n");
}

fn cmd_doctor() {
    println!("Cyrene Doctor — checking system health\n");

    let cyrene_dir = cyrene_config::cyrene_home_dir().unwrap_or_default();

    // Load secrets from ~/.cyrene/.env so the environment check below reflects
    // what the agent will actually see at runtime.
    let _ = cyrene_config::SecretResolver::with_dotenv_path(cyrene_dir.join(".env"));

    let config_path = cyrene_dir.join("config.toml");
    if config_path.exists() {
        println!("  ✓ Config file found: {}", config_path.display());
    } else {
        println!("  ✗ Config file not found: {}", config_path.display());
        println!("    Run `cyrene onboard` to create one.");
    }

    let db_path = cyrene_dir.join("cyrene.db");
    if db_path.exists() {
        println!("  ✓ Database found: {}", db_path.display());
    } else {
        println!("  ○ Database not yet created (will be created on first run)");
    }

    let env_path = cyrene_dir.join(".env");
    if env_path.exists() {
        println!("  ✓ .env file found: {}", env_path.display());
    } else {
        println!("  ○ No .env file found (run `cyrene onboard` to add your keys)");
    }

    if let Ok(config) = cyrene_config::Config::load() {
        let secrets = config.referenced_secret_envs();
        let mut found = 0;
        let mut missing = Vec::new();
        for secret in &secrets {
            if std::env::var(secret).is_ok() {
                found += 1;
            } else {
                missing.push(secret.clone());
            }
        }
        println!("\n  Secrets referenced by config: {}", secrets.len());
        println!("  ✓ Found in environment: {found}");
        if !missing.is_empty() {
            println!("  ✗ Missing from environment:");
            for m in &missing {
                println!("      - {m}");
            }
        }

        let providers: Vec<_> = config.providers().map(|p| p.alias.to_owned()).collect();
        let channels: Vec<_> = config.channels().map(|c| c.alias.to_owned()).collect();
        println!("\n  Configured providers: {}", providers.join(", "));
        println!("  Configured channels: {}", channels.join(", "));

        // Execution backend (R33.5): local by default; a remote backend only
        // relocates where Steps run — autonomy/sandbox/approval still apply.
        match config.execution.backend {
            cyrene_config::ExecutionBackendKind::Local => {
                println!("\n  Execution backend: local (OS-level sandbox)");
            }
            kind => {
                let boundary = config.execution.remote_workspace().unwrap_or("(unset)");
                println!("\n  Execution backend: {kind:?} — boundary {boundary}");
                println!("    (autonomy, sandbox boundary, and approval still apply — R22/R6)");
            }
        }
    } else {
        println!("\n  ✗ Could not load config (run `cyrene onboard` first)");
    }

    println!("\nDoctor check complete.");
}

fn cmd_extensions_list() {
    let extensions_dir = std::env::current_dir()
        .unwrap_or_default()
        .join("extensions");

    match cyrene_config::discover_extensions(&extensions_dir, cyrene_sdk::SDK_VERSION) {
        Ok(report) => {
            println!(
                "Extensions ({} loaded, {} skipped):\n",
                report.loaded_count(),
                report.skipped_count()
            );
            let list = cyrene_config::format_extension_list(&report);
            for ext in &list {
                let status = if ext.get("enabled").map(|s| s.as_str()) == Some("yes") {
                    "✓"
                } else {
                    "✗"
                };
                println!(
                    "  {} {} v{} — {} [{}]",
                    status,
                    ext.get("name").unwrap_or(&"?".to_owned()),
                    ext.get("version").unwrap_or(&"?".to_owned()),
                    ext.get("description").unwrap_or(&String::new()),
                    ext.get("capabilities").unwrap_or(&"?".to_owned()),
                );
                if let Some(err) = ext.get("error") {
                    println!("      Error: {err}");
                }
            }
        }
        Err(e) => {
            eprintln!("Error scanning extensions: {e}");
        }
    }
}

fn cmd_skills_list() {
    let skills_dir = std::env::current_dir().unwrap_or_default().join("skills");

    if !skills_dir.exists() {
        println!("No bundled skills found.");
        return;
    }

    let mut categories: Vec<(String, Vec<String>)> = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&skills_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let cat_name = path.file_name().unwrap().to_string_lossy().to_string();
                let mut skills = Vec::new();
                if let Ok(files) = std::fs::read_dir(&path) {
                    for f in files.flatten() {
                        let fname = f.file_name().to_string_lossy().to_string();
                        if fname.ends_with(".md") {
                            skills.push(fname.replace(".md", ""));
                        }
                    }
                }
                skills.sort();
                categories.push((cat_name, skills));
            }
        }
    }

    categories.sort_by(|a, b| a.0.cmp(&b.0));

    let total: usize = categories.iter().map(|(_, s)| s.len()).sum();
    println!("Bundled Skills Library ({total} skills):\n");

    for (cat, skills) in &categories {
        println!("  {} ({}):", cat, skills.len());
        for skill in skills {
            println!("    - {skill}");
        }
        println!();
    }
}

fn cmd_tools_list() {
    println!("Built-in Tools:\n");
    println!("  fs.read       — Read files from the workspace       [risk: low]");
    println!("  fs.write      — Write files to the workspace        [risk: medium]");
    println!("  fs.edit       — Edit existing files                 [risk: medium]");
    println!("  shell.run     — Execute shell commands (sandboxed)  [risk: high]");
    println!("  web.fetch     — Fetch content from URLs             [risk: low]");
    println!("  web.search    — Search the web                      [risk: low]");
    println!("  image.gen     — Generate images                     [risk: low]");
    println!("  tts.speak     — Text-to-speech conversion           [risk: low]");
}

fn cmd_model_list() {
    let Ok(config) = cyrene_config::Config::load() else {
        println!("Could not load config. Run `cyrene onboard` first.");
        return;
    };

    println!("Configured Model Providers:\n");
    for p in config.providers() {
        let tier = p
            .entry
            .tier
            .map(|t| format!("{t:?}"))
            .unwrap_or_else(|| "Local".to_owned());
        let model = p.entry.model.as_deref().unwrap_or("(default)");
        println!("  {}.{} — {} [{}]", p.type_name, p.alias, model, tier);
    }
}

fn cmd_catalog_list() {
    let catalog_dir = std::env::current_dir()
        .unwrap_or_default()
        .join("optional-mcps");

    println!("Optional Component & MCP Catalog:\n");

    if catalog_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&catalog_dir) {
            let mut found = false;
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let manifest = path.join("cyrene.plugin.toml");
                    if manifest.exists() {
                        if let Ok(raw) = std::fs::read_to_string(&manifest) {
                            if let Ok(ext) = cyrene_sdk::ExtensionManifest::from_toml(&raw) {
                                println!(
                                    "  {} v{} — {} [{}]",
                                    ext.name,
                                    ext.version,
                                    ext.description,
                                    ext.capabilities
                                        .iter()
                                        .map(|c| c.to_string())
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                );
                                found = true;
                            }
                        }
                    }
                }
            }
            if !found {
                println!("  (No optional components in catalog)");
            }
        }
    } else {
        println!("  (No optional components in catalog)");
    }
}

/// An owned snapshot of a configured provider, decoupled from the borrowed
/// [`cyrene_config::Config`] so the chat loop can reload and switch providers
/// at runtime (e.g. after `/connect`).
struct ChatProvider {
    type_name: String,
    alias: String,
    entry: cyrene_config::ProviderEntry,
}

impl ChatProvider {
    /// `type.alias` label, e.g. `opencode-go.default`.
    fn label(&self) -> String {
        format!("{}.{}", self.type_name, self.alias)
    }

    /// The model name this provider will use, or its preset/type default.
    fn model_label(&self) -> &str {
        self.entry.model.as_deref().unwrap_or(&self.type_name)
    }
}

/// Loads every configured provider as an owned [`ChatProvider`]. Returns an
/// empty list when no config exists yet (so the caller can offer `/connect`).
fn load_chat_providers() -> Vec<ChatProvider> {
    let Ok(config) = cyrene_config::Config::load() else {
        return Vec::new();
    };
    config
        .providers()
        .map(|p| ChatProvider {
            type_name: p.type_name.to_owned(),
            alias: p.alias.to_owned(),
            entry: p.entry.clone(),
        })
        .collect()
}

/// Instantiates a [`cyrene_core::Model`] for the given provider snapshot.
fn build_chat_model(
    p: &ChatProvider,
    secrets: &cyrene_config::SecretResolver,
) -> Result<std::sync::Arc<dyn cyrene_core::Model>, cyrene_config::BoxError> {
    cyrene_models::create_provider(&p.type_name, &p.alias, &p.entry, secrets)
}

/// Lists configured providers, marking the active one.
fn print_chat_models(providers: &[ChatProvider], active: usize) {
    if providers.is_empty() {
        println!("\nNo providers configured. Use /connect to add one.\n");
        return;
    }
    println!("\nConfigured providers:");
    for (i, p) in providers.iter().enumerate() {
        let marker = if i == active { "●" } else { " " };
        println!("  {marker} {}. {} ({})", i + 1, p.label(), p.model_label());
    }
    println!("\nSwitch with `/model <alias|type.alias|number>`.\n");
}

/// Resolves a `/model` argument (alias, `type.alias`, or 1-based index) to a
/// provider index.
fn resolve_provider_arg(providers: &[ChatProvider], arg: &str) -> Option<usize> {
    let arg = arg.trim();
    if arg.is_empty() {
        return None;
    }
    if let Ok(n) = arg.parse::<usize>() {
        if n >= 1 && n <= providers.len() {
            return Some(n - 1);
        }
    }
    if let Some(i) = providers.iter().position(|p| p.label() == arg) {
        return Some(i);
    }
    providers.iter().position(|p| p.alias == arg)
}

/// Hermes-style interactive model picker: choose a provider, discover its
/// models live (OpenAI `/v1/models` or Ollama `/api/tags`), pick one (or type a
/// custom id), apply it for the session, and return the rebuilt model. Returns
/// `None` on cancel, non-TTY, or build failure.
fn interactive_model_picker(
    providers: &mut [ChatProvider],
    secrets: &cyrene_config::SecretResolver,
    rt: &tokio::runtime::Runtime,
) -> Option<(usize, std::sync::Arc<dyn cyrene_core::Model>)> {
    use std::io::IsTerminal;

    if providers.is_empty() {
        println!("No providers configured. Use /connect first.");
        return None;
    }
    // Without a TTY the dialoguer prompts can't run; just show the list.
    if !std::io::stdin().is_terminal() {
        return None;
    }

    let labels: Vec<String> = providers
        .iter()
        .map(|p| format!("{} ({})", p.label(), p.model_label()))
        .collect();
    let pidx = dialoguer::Select::new()
        .with_prompt("Select a provider")
        .items(&labels)
        .default(0)
        .interact()
        .ok()?;

    // Discover the provider's catalog live. Build a probe model with the current
    // entry, then ask it for its model list (best-effort).
    let discovered: Vec<String> = match build_chat_model(&providers[pidx], secrets) {
        Ok(probe) => {
            println!("  Fetching models from {}…", providers[pidx].type_name);
            match rt.block_on(probe.list_models()) {
                Ok(list) => list,
                Err(e) => {
                    eprintln!("  (couldn't fetch model list: {e})");
                    Vec::new()
                }
            }
        }
        Err(_) => Vec::new(),
    };

    let chosen_model: Option<String> = if discovered.is_empty() {
        let current = providers[pidx].model_label().to_owned();
        let m: String = dialoguer::Input::new()
            .with_prompt("Model name")
            .with_initial_text(current)
            .allow_empty(true)
            .interact_text()
            .ok()?;
        let m = m.trim();
        (!m.is_empty()).then(|| m.to_owned())
    } else {
        let mut items: Vec<String> = discovered.clone();
        items.push("custom…".to_owned());
        // Preselect the currently configured model if it's in the list.
        let default_idx = providers[pidx]
            .entry
            .model
            .as_deref()
            .and_then(|cur| discovered.iter().position(|m| m == cur))
            .unwrap_or(0);
        let midx = dialoguer::Select::new()
            .with_prompt("Select a model")
            .items(&items)
            .default(default_idx)
            .interact()
            .ok()?;
        if midx == discovered.len() {
            let m: String = dialoguer::Input::new()
                .with_prompt("Model name")
                .interact_text()
                .ok()?;
            let m = m.trim();
            (!m.is_empty()).then(|| m.to_owned())
        } else {
            Some(discovered[midx].clone())
        }
    };

    if let Some(m) = chosen_model {
        providers[pidx].entry.model = Some(m);
    }

    match build_chat_model(&providers[pidx], secrets) {
        Ok(model) => Some((pidx, model)),
        Err(e) => {
            eprintln!("  ✗ Could not switch to {}: {e}", providers[pidx].label());
            eprintln!("    Check its API key with /connect.");
            None
        }
    }
}

/// Runs an interactive chat REPL against the configured model provider.
///
/// This is what `cyrene` (no subcommand) and `cyrene agent`/`cyrene chat` launch:
/// a conversational loop that reads a line, sends the running conversation to
/// the model, and prints the reply. Secrets load from `~/.cyrene/.env`. In-chat
/// slash commands (`/models`, `/model`, `/connect`, …) manage providers without
/// leaving the session.
/// Runs one completion turn: sends the current conversation, prints the reply,
/// accumulates token usage, and (on error) drops the trailing user turn so the
/// next attempt starts clean. Shared by normal input and `/retry`.
fn complete_turn(
    rt: &tokio::runtime::Runtime,
    model: &std::sync::Arc<dyn cyrene_core::Model>,
    history: &mut Vec<cyrene_core::ChatMessage>,
    usage_total: &mut cyrene_core::TokenUsage,
    verbose: bool,
) {
    use cyrene_core::{ChatMessage, ModelRequest, Role};

    let req = ModelRequest::new(history.clone());
    match rt.block_on(model.complete(req)) {
        Ok(resp) => {
            let reply = resp.content.trim().to_owned();
            println!("\ncyrene ▸ {reply}\n");
            usage_total.input_tokens = usage_total
                .input_tokens
                .saturating_add(resp.usage.input_tokens);
            usage_total.output_tokens = usage_total
                .output_tokens
                .saturating_add(resp.usage.output_tokens);
            if verbose {
                println!(
                    "  [tokens: +{} in, +{} out · session total {}]\n",
                    resp.usage.input_tokens,
                    resp.usage.output_tokens,
                    usage_total.total()
                );
            }
            history.push(ChatMessage::assistant(reply));
        }
        Err(e) => {
            eprintln!("\n  ✗ {e}");
            let msg = e.to_string().to_lowercase();
            if msg.contains("insufficient")
                || msg.contains("balance")
                || msg.contains("credit")
                || msg.contains("401")
                || msg.contains("unauthorized")
            {
                eprintln!(
                    "    Tip: top up this provider, or switch with /models then /model <name>."
                );
            }
            eprintln!();
            if history.last().map(|m| m.role) == Some(Role::User) {
                history.pop();
            }
        }
    }
}

/// Copies text to the OS clipboard via the platform's standard utility.
/// Returns `false` if no clipboard tool is available.
fn copy_to_clipboard(text: &str) -> bool {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let candidates: &[(&str, &[&str])] = if cfg!(target_os = "macos") {
        &[("pbcopy", &[])]
    } else if cfg!(target_os = "windows") {
        &[("clip", &[])]
    } else {
        &[
            ("wl-copy", &[]),
            ("xclip", &["-selection", "clipboard"]),
            ("xsel", &["--clipboard", "--input"]),
        ]
    };

    for (cmd, args) in candidates {
        if let Ok(mut child) = Command::new(cmd).args(*args).stdin(Stdio::piped()).spawn() {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            if child.wait().map(|s| s.success()).unwrap_or(false) {
                return true;
            }
        }
    }
    false
}

/// A small rotating set of encouragements for `/fortune`.
fn random_fortune() -> &'static str {
    const FORTUNES: &[&str] = &[
        "Small steps still move you forward.",
        "The best time to start was yesterday; the next best is now.",
        "You don't have to be perfect to be making progress.",
        "Ship it, then make it better.",
        "Every expert was once a beginner who kept going.",
        "Rest is part of the work.",
        "Cyrene believes in you. 💛",
    ];
    let idx = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as usize)
        .unwrap_or(0)
        % FORTUNES.len();
    FORTUNES[idx]
}

/// Prints the active provider/model, message count, and session token usage.
fn print_chat_status(
    providers: &[ChatProvider],
    active: usize,
    history: &[cyrene_core::ChatMessage],
    usage_total: &cyrene_core::TokenUsage,
) {
    use cyrene_core::Role;
    println!("\nSession status:");
    match providers.get(active) {
        Some(p) => println!("  Provider: {} ({})", p.label(), p.model_label()),
        None => println!("  Provider: (none — use /connect)"),
    }
    let msgs = history.iter().filter(|m| m.role != Role::System).count();
    println!("  Messages: {msgs}");
    println!(
        "  Tokens:   {} in / {} out ({} total)\n",
        usage_total.input_tokens,
        usage_total.output_tokens,
        usage_total.total()
    );
}

/// Prints the user/assistant transcript (skipping the system prompt).
fn print_chat_history(history: &[cyrene_core::ChatMessage]) {
    use cyrene_core::Role;
    println!("\nTranscript:");
    let mut any = false;
    for m in history {
        let who = match m.role {
            Role::User => "you",
            Role::Assistant => "cyrene",
            Role::Tool => "tool",
            Role::System => continue,
        };
        println!("  {who} ▸ {}", m.content.trim());
        any = true;
    }
    if !any {
        println!("  (empty)");
    }
    println!();
}

/// Saves the transcript as pretty JSON under `~/.cyrene/transcripts/`.
fn save_transcript(
    dir: &std::path::Path,
    history: &[cyrene_core::ChatMessage],
) -> std::io::Result<std::path::PathBuf> {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = dir.join("transcripts").join(format!("chat-{secs}.json"));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(history).map_err(std::io::Error::other)?;
    std::fs::write(&path, json)?;
    Ok(path)
}

/// Builds the configured default-provider model (alias `default`, else first),
/// loading secrets from `~/.cyrene/.env`. Shared by the standalone Telegram and
/// gateway entry points.
fn build_default_model() -> Option<std::sync::Arc<dyn cyrene_core::Model>> {
    let cyrene_dir = cyrene_config::cyrene_home_dir().unwrap_or_default();
    let secrets = cyrene_config::SecretResolver::with_dotenv_path(cyrene_dir.join(".env"));
    let providers = load_chat_providers();
    let idx = providers
        .iter()
        .position(|p| p.alias == "default")
        .unwrap_or(0);
    providers
        .get(idx)
        .and_then(|p| build_chat_model(p, &secrets).ok())
}

/// `cyrene telegram` / `cyrene gateway`: connect Cyrene to Telegram using
/// `TELEGRAM_BOT_TOKEN` (from `~/.cyrene/.env`) and answer messages with the
/// configured model. Blocks until interrupted.
fn run_telegram() {
    let cyrene_dir = cyrene_config::cyrene_home_dir().unwrap_or_default();
    let _ = cyrene_config::SecretResolver::with_dotenv_path(cyrene_dir.join(".env"));

    let Ok(token) = std::env::var("TELEGRAM_BOT_TOKEN") else {
        println!("No TELEGRAM_BOT_TOKEN set.");
        println!("Run `cyrene onboard` and choose the Telegram channel, or add the token to");
        println!("~/.cyrene/.env, then try again. Create a bot with @BotFather on Telegram.");
        return;
    };
    if token.trim().is_empty() {
        println!("TELEGRAM_BOT_TOKEN is empty. Add your @BotFather token to ~/.cyrene/.env.");
        return;
    }

    let Some(model) = build_default_model() else {
        println!("No usable model provider. Run `cyrene onboard` (or `cyrene` then /connect).");
        return;
    };

    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("  ✗ Could not start the async runtime: {e}");
            return;
        }
    };
    telegram::run(&rt, model, token.trim());
}

/// `cyrene whatsapp`: run the WhatsApp Cloud API webhook bridge, answering
/// messages with the configured model. Needs `WHATSAPP_ACCESS_TOKEN`,
/// `WHATSAPP_PHONE_NUMBER_ID`, and `WHATSAPP_VERIFY_TOKEN` in `~/.cyrene/.env`.
/// Blocks until interrupted.
fn run_whatsapp() {
    let cyrene_dir = cyrene_config::cyrene_home_dir().unwrap_or_default();
    let _ = cyrene_config::SecretResolver::with_dotenv_path(cyrene_dir.join(".env"));

    let settings = match whatsapp::Settings::from_env() {
        Ok(s) => s,
        Err(e) => {
            println!("WhatsApp is not configured: {e}");
            println!(
                "Add WHATSAPP_ACCESS_TOKEN, WHATSAPP_PHONE_NUMBER_ID, and WHATSAPP_VERIFY_TOKEN to"
            );
            println!(
                "~/.cyrene/.env (from the Meta WhatsApp Cloud API dashboard), then try again."
            );
            return;
        }
    };

    let Some(model) = build_default_model() else {
        println!("No usable model provider. Run `cyrene onboard` (or `cyrene` then /connect).");
        return;
    };

    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("  ✗ Could not start the async runtime: {e}");
            return;
        }
    };
    whatsapp::run(&rt, model, settings);
}

/// Builds the system prompt for the interactive REPL. Delegates to
/// [`agent::system_prompt`] so every channel (CLI, Telegram, WhatsApp) shares
/// one identity and memory. Called at startup and whenever memory changes.
fn rebuild_system_prompt() -> String {
    agent::system_prompt()
}

/// How long an in-chat Python run may take before it's killed.
const PY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// Runs an inline Python snippet (`/py`) and prints the captured output.
fn run_python_snippet(code: &str) {
    println!("\n  running Python…");
    match pyexec::run_code(code, PY_TIMEOUT) {
        Ok(outcome) => print_py_outcome(&outcome),
        Err(e) => eprintln!("  ✗ {e}\n"),
    }
}

/// Runs a Python file (`/run <file.py>`) and prints the captured output.
fn run_python_path(path: &str) {
    let Some(py) = pyexec::interpreter() else {
        eprintln!("  ✗ No Python interpreter found. Install Python 3 and try again.\n");
        return;
    };
    // Accept either a filesystem path or a saved-script name.
    let Some(p) = pyexec::resolve_script(path) else {
        eprintln!("  ✗ No such file or saved script: {path}\n");
        return;
    };
    println!("\n  running {}…", p.display());
    match pyexec::run_file(py, &p, PY_TIMEOUT) {
        Ok(outcome) => print_py_outcome(&outcome),
        Err(e) => eprintln!("  ✗ {e}\n"),
    }
}

/// The Python from Cyrene's most recent reply (all blocks joined), or `None`.
/// Backs `/script`, which saves what she just wrote as a named script.
fn last_assistant_python(history: &[cyrene_core::ChatMessage]) -> Option<String> {
    use cyrene_core::Role;
    let reply = history.iter().rev().find(|m| m.role == Role::Assistant)?;
    let blocks = pyexec::extract_python_blocks(&reply.content);
    if blocks.is_empty() {
        return None;
    }
    Some(blocks.join("\n"))
}

/// Pretty-prints a Python run's stdout/stderr/exit status.
fn print_py_outcome(outcome: &pyexec::PyOutcome) {
    if !outcome.stdout.trim().is_empty() {
        println!("\n{}", outcome.stdout.trim_end());
    }
    if !outcome.stderr.trim().is_empty() {
        eprintln!("\n  stderr:\n{}", outcome.stderr.trim_end());
    }
    match outcome.status {
        Some(0) => println!("\n  ✓ exit 0\n"),
        Some(c) => println!("\n  ✗ exit {c}\n"),
        None => println!("\n  ✗ timed out\n"),
    }
}

/// Carries out the self-learning actions in Cyrene's latest reply: she saves
/// named skills, curates her own memory, and proposes schedules — no slash
/// commands from the user. Saving a skill and writing a memory are low-risk and
/// automatic; scheduling runs code on a timer, so it's gated on consent.
fn apply_learned_actions(history: &mut [cyrene_core::ChatMessage]) {
    use cyrene_core::{ChatMessage, Role};
    use std::io::IsTerminal;

    let Some(content) = history
        .iter()
        .rev()
        .find(|m| m.role == Role::Assistant)
        .map(|m| m.content.clone())
    else {
        return;
    };
    let acts = actions::parse(&content);
    if acts.is_empty() {
        return;
    }

    let mut memory_changed = false;
    for act in acts {
        match act {
            actions::Action::Remember(fact) => {
                chatmem::record_fact(&fact);
                memory_changed = true;
                println!("  📝 learned: {fact}");
            }
            actions::Action::RememberUser(note) => {
                chatmem::record_profile(&note);
                memory_changed = true;
                println!("  💛 noted about you: {note}");
            }
            actions::Action::LearnSkill { name, code } => match pyexec::save_script(&name, &code) {
                Ok(path) => {
                    let stem = pyexec::sanitize_name(&name).unwrap_or_default();
                    chatmem::record_fact(&format!(
                        "has a saved skill `{stem}` at {}",
                        path.display()
                    ));
                    memory_changed = true;
                    println!("  💾 learned skill `{stem}` — run it with /run {stem}");
                }
                Err(e) => eprintln!("  ✗ could not save skill `{name}`: {e}"),
            },
            actions::Action::Schedule {
                name,
                script,
                schedule,
                channel,
            } => {
                let consent = if std::io::stdin().is_terminal() {
                    use std::io::Write;
                    print!(
                        "  ↳ Cyrene wants to schedule `{name}` ({script} @ {schedule} → {channel}). Allow? [y/N] "
                    );
                    let _ = std::io::stdout().flush();
                    let mut a = String::new();
                    let _ = std::io::stdin().read_line(&mut a);
                    matches!(a.trim().to_lowercase().as_str(), "y" | "yes")
                } else {
                    false
                };
                if consent {
                    if let Err(e) = crons::add(&name, &schedule, &script, &channel) {
                        eprintln!("  ✗ {e}");
                    }
                } else {
                    println!(
                        "  (skipped — schedule it yourself with /cron {name} {script} {schedule} {channel})"
                    );
                }
            }
            actions::Action::ScheduleAgent {
                name,
                schedule,
                prompt,
                channel,
            } => {
                let target = if channel.is_empty() { "cli" } else { &channel };
                let consent = if std::io::stdin().is_terminal() {
                    use std::io::Write;
                    print!(
                        "  ↳ Cyrene wants to schedule a recurring task `{name}` ({schedule} → {target}):\n     “{prompt}”\n    Allow? [y/N] "
                    );
                    let _ = std::io::stdout().flush();
                    let mut a = String::new();
                    let _ = std::io::stdin().read_line(&mut a);
                    matches!(a.trim().to_lowercase().as_str(), "y" | "yes")
                } else {
                    false
                };
                if consent {
                    if let Err(e) = crons::add_agent(&name, &schedule, &prompt, target) {
                        eprintln!("  ✗ {e}");
                    }
                } else {
                    println!("  (skipped the recurring task `{name}`)");
                }
            }
        }
    }

    if memory_changed {
        history[0] = ChatMessage::system(rebuild_system_prompt());
    }
}

/// After Cyrene replies, look for ```python blocks she wrote and — with consent
/// — run them, feeding the output back so she can react. This is the
/// "write a script and run it" loop the user asked for. `autorun` skips the
/// prompt; otherwise we only ask when stdin is a real terminal.
fn offer_reply_python(
    rt: &tokio::runtime::Runtime,
    model: &std::sync::Arc<dyn cyrene_core::Model>,
    history: &mut Vec<cyrene_core::ChatMessage>,
    usage_total: &mut cyrene_core::TokenUsage,
    verbose: bool,
    autorun: bool,
) {
    use cyrene_core::{ChatMessage, Role};
    use std::io::IsTerminal;

    let Some(reply) = history.iter().rev().find(|m| m.role == Role::Assistant) else {
        return;
    };
    let blocks = pyexec::extract_python_blocks(&reply.content);
    if blocks.is_empty() {
        return;
    }

    let consent = if autorun {
        true
    } else if std::io::stdin().is_terminal() {
        use std::io::Write;
        print!(
            "  ↳ Cyrene wrote {} Python block{}. Run {}? [y/N] ",
            blocks.len(),
            if blocks.len() == 1 { "" } else { "s" },
            if blocks.len() == 1 { "it" } else { "them" }
        );
        let _ = std::io::stdout().flush();
        let mut ans = String::new();
        let _ = std::io::stdin().read_line(&mut ans);
        matches!(ans.trim().to_lowercase().as_str(), "y" | "yes")
    } else {
        false
    };
    if !consent {
        return;
    }

    let mut combined = String::new();
    for (i, block) in blocks.iter().enumerate() {
        if blocks.len() > 1 {
            println!("\n  running block {}/{}…", i + 1, blocks.len());
        } else {
            println!("\n  running Python…");
        }
        match pyexec::run_code(block, PY_TIMEOUT) {
            Ok(outcome) => {
                print_py_outcome(&outcome);
                combined.push_str(&outcome.summary());
                combined.push('\n');
            }
            Err(e) => {
                eprintln!("  ✗ {e}\n");
                combined.push_str(&format!("[error] {e}\n"));
            }
        }
    }

    // Feed the result back so Cyrene can interpret it and continue the task.
    history.push(ChatMessage::user(format!(
        "I ran the Python you wrote. Here is the output:\n{combined}\n\
         Briefly interpret the result and say what to do next."
    )));
    complete_turn(rt, model, history, usage_total, verbose);
}

fn run_chat() {
    use cyrene_core::{ChatMessage, Role};

    let cyrene_dir = cyrene_config::cyrene_home_dir().unwrap_or_default();
    let env_path = cyrene_dir.join(".env");

    // Seed the process environment from ~/.cyrene/.env so configured
    // `api_key_env` secrets resolve without the user exporting them by hand.
    let mut secrets = cyrene_config::SecretResolver::with_dotenv_path(&env_path);

    let mut providers = load_chat_providers();
    let mut active = providers
        .iter()
        .position(|p| p.alias == "default")
        .unwrap_or(0);

    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("  ✗ Could not start the async runtime: {e}");
            return;
        }
    };

    // Build the initial model. A failure here (e.g. missing key) is non-fatal:
    // the user can still `/connect`, `/models`, or `/model` to fix it.
    let mut model: Option<std::sync::Arc<dyn cyrene_core::Model>> = None;
    if let Some(p) = providers.get(active) {
        match build_chat_model(p, &secrets) {
            Ok(m) => {
                model = Some(m);
            }
            Err(e) => {
                eprintln!("  ✗ Could not initialize {}: {e}", p.label());
                eprintln!("    Use /connect to add a key, or /models to switch.");
            }
        }
    } else {
        eprintln!("    No model provider configured yet — use /connect to set one up.");
    }

    // Auto-start the cron scheduler for this session so anything Cyrene
    // schedules ("report me X every morning") fires while the chat is open —
    // no separate `cyrene cron run` needed.
    crons::spawn_background();

    // Neat startup card: identity, model, what Cyrene remembers, and the
    // grouped command index.
    print_welcome_card(&providers, active, model.is_some());

    let mut history: Vec<ChatMessage> = vec![ChatMessage::system(rebuild_system_prompt())];
    let mut usage_total = cyrene_core::TokenUsage::default();
    let mut verbose = false;
    // When on, Python blocks in Cyrene's replies run automatically; otherwise the
    // REPL asks first. On by default so Cyrene acts as an agent — she writes a
    // script and runs it herself, the same way the messaging channels do. Turn
    // it off any time with `/autorun` if you'd rather approve each run.
    let mut autorun = true;

    // Tracks a lone Ctrl-C waiting for a second press. Any real input clears it,
    // so only two interrupts in a row quit.
    let mut interrupted = false;

    loop {
        let input_line = match prompt::read("you ▸ ") {
            prompt::Read::Eof => {
                println!("\nGoodbye 💛");
                break;
            }
            prompt::Read::Interrupt => {
                if interrupted {
                    println!("Goodbye 💛");
                    break;
                }
                interrupted = true;
                println!("(Press Ctrl-C again or type /exit to quit.)");
                continue;
            }
            prompt::Read::Line(l) => l,
        };
        interrupted = false;

        let input = input_line.trim();
        if input.is_empty() {
            continue;
        }

        // Slash commands are handled locally and never sent to the model.
        if let Some(rest) = input.strip_prefix('/') {
            let mut parts = rest.splitn(2, char::is_whitespace);
            let cmd = parts.next().unwrap_or("");
            let arg = parts.next().unwrap_or("").trim();

            match cmd {
                // A bare `/` (or trailing space) — show the suggestion menu,
                // the "what can I type here?" affordance from Claude/Hermes.
                "" => {
                    slash::print_suggestions("");
                }
                "help" | "h" | "?" => slash::print_help(),
                "models" | "providers" => print_chat_models(&providers, active),
                "model" | "use" | "switch" => {
                    if providers.is_empty() {
                        println!("No providers configured. Use /connect first.");
                    } else if arg.is_empty() {
                        // No argument: open the interactive provider/model picker.
                        if let Some((i, m)) =
                            interactive_model_picker(&mut providers, &secrets, &rt)
                        {
                            active = i;
                            model = Some(m);
                            println!(
                                "Switched to {} ({}).",
                                providers[i].label(),
                                providers[i].model_label()
                            );
                        } else {
                            // Non-TTY or cancelled: fall back to listing.
                            print_chat_models(&providers, active);
                        }
                    } else if let Some(i) = resolve_provider_arg(&providers, arg) {
                        match build_chat_model(&providers[i], &secrets) {
                            Ok(m) => {
                                active = i;
                                model = Some(m);
                                println!(
                                    "Switched to {} ({}).",
                                    providers[i].label(),
                                    providers[i].model_label()
                                );
                            }
                            Err(e) => {
                                eprintln!("  ✗ Could not switch to {}: {e}", providers[i].label());
                                eprintln!("    Check its API key with /connect.");
                            }
                        }
                    } else {
                        println!("Unknown provider `{arg}`. Use /models to see options.");
                    }
                }
                "connect" => {
                    println!();
                    run_onboarding(false, None, None);
                    // Reload secrets (new .env keys) and providers, then rebuild.
                    secrets = cyrene_config::SecretResolver::with_dotenv_path(&env_path);
                    providers = load_chat_providers();
                    active = providers
                        .iter()
                        .position(|p| p.alias == "default")
                        .unwrap_or(0);
                    model = providers
                        .get(active)
                        .and_then(|p| build_chat_model(p, &secrets).ok());
                    if let Some(p) = providers.get(active) {
                        println!("\nNow chatting with {} ({}).\n", p.label(), p.model_label());
                    }
                }
                "clear" | "reset" | "new" => {
                    history.truncate(1); // keep the system prompt
                    usage_total = cyrene_core::TokenUsage::default();
                    println!("Conversation cleared.");
                }
                "doctor" => {
                    println!();
                    cmd_doctor();
                    println!();
                }
                "status" => print_chat_status(&providers, active, &history, &usage_total),
                "history" => print_chat_history(&history),
                "usage" => {
                    println!(
                        "\nSession usage: {} in / {} out ({} total)\n",
                        usage_total.input_tokens,
                        usage_total.output_tokens,
                        usage_total.total()
                    );
                }
                "verbose" => {
                    verbose = !verbose;
                    println!(
                        "Per-reply token counts {}.",
                        if verbose { "on" } else { "off" }
                    );
                }
                "save" => match save_transcript(&cyrene_dir, &history) {
                    Ok(path) => println!("Transcript saved to {}", path.display()),
                    Err(e) => eprintln!("  ✗ Could not save transcript: {e}"),
                },
                "copy" => match history.iter().rev().find(|m| m.role == Role::Assistant) {
                    Some(last) if copy_to_clipboard(&last.content) => {
                        println!("Copied the last reply to the clipboard.");
                    }
                    Some(_) => println!("Could not access the clipboard on this system."),
                    None => println!("No assistant message to copy yet."),
                },
                "retry" => {
                    if let Some(m) = model.clone() {
                        // Drop the previous reply (if any) and re-run the last user turn.
                        if history.last().map(|x| x.role) == Some(Role::Assistant) {
                            history.pop();
                        }
                        if history.last().map(|x| x.role) == Some(Role::User) {
                            complete_turn(&rt, &m, &mut history, &mut usage_total, verbose);
                        } else {
                            println!("Nothing to retry.");
                        }
                    } else {
                        println!("No active model. Use /connect or /models.");
                    }
                }
                "undo" => {
                    let before = history.len();
                    if history.last().map(|x| x.role) == Some(Role::Assistant) {
                        history.pop();
                    }
                    if history.last().map(|x| x.role) == Some(Role::User) {
                        history.pop();
                    }
                    if history.len() < before {
                        println!("Removed the last exchange.");
                    } else {
                        println!("Nothing to undo.");
                    }
                }
                "reload" => {
                    secrets = cyrene_config::SecretResolver::with_dotenv_path(&env_path);
                    model = providers
                        .get(active)
                        .and_then(|p| build_chat_model(p, &secrets).ok());
                    println!("Reloaded ~/.cyrene/.env.");
                }
                "tools" => {
                    println!();
                    cmd_tools_list();
                    println!();
                }
                "skills" => {
                    println!();
                    cmd_skills_list();
                    println!();
                }
                "version" => {
                    println!();
                    update::show_version();
                    println!();
                }
                "update" => update::run_update(false),
                "py" | "python" => {
                    if arg.is_empty() {
                        println!("Usage: /py <code>   e.g. /py print(2 ** 10)");
                    } else {
                        run_python_snippet(arg);
                    }
                }
                "run" => {
                    if arg.is_empty() {
                        println!("Usage: /run <file.py | saved-script-name>");
                    } else {
                        run_python_path(arg);
                    }
                }
                "autorun" => {
                    autorun = !autorun;
                    println!(
                        "Auto-running Python from replies is now {}.",
                        if autorun {
                            "ON — Cyrene runs the scripts she writes (use with care)"
                        } else {
                            "off — the REPL will ask before running"
                        }
                    );
                }
                "key" => {
                    // `/key NAME value` — stash a secret in ~/.cyrene/.env so the
                    // scripts Cyrene writes can read it via os.environ. The single
                    // hardest part of "integrate me with this API" made trivial.
                    let mut kv = arg.splitn(2, char::is_whitespace);
                    let name = kv.next().unwrap_or("").trim();
                    let value = kv.next().unwrap_or("").trim();
                    if name.is_empty() || value.is_empty() {
                        println!("Usage: /key NAME value   e.g. /key OPENWEATHER_API_KEY abc123");
                    } else {
                        let _ =
                            write_env_secrets(&env_path, &[(name.to_owned(), value.to_owned())]);
                        std::env::set_var(name, value);
                        secrets = cyrene_config::SecretResolver::with_dotenv_path(&env_path);
                        chatmem::record_fact(&format!(
                            "has an API key in env var {name} (saved to .env)"
                        ));
                        history[0] = ChatMessage::system(rebuild_system_prompt());
                        println!(
                            "✓ Saved {name} to ~/.cyrene/.env. Scripts can read it with \
                             os.environ[\"{name}\"]."
                        );
                    }
                }
                "script" => {
                    // Save the most recent Python block Cyrene wrote as a named,
                    // re-runnable script under ~/.cyrene/scripts/<name>.py.
                    if arg.is_empty() {
                        println!("Usage: /script <name>   (saves Cyrene's last Python block)");
                    } else {
                        match last_assistant_python(&history) {
                            Some(code) => match pyexec::save_script(arg, &code) {
                                Ok(path) => {
                                    chatmem::record_fact(&format!(
                                        "has a saved script `{}` at {}",
                                        pyexec::sanitize_name(arg).unwrap_or_default(),
                                        path.display()
                                    ));
                                    println!(
                                        "✓ Saved script to {}. Run it with /run {} or schedule \
                                         it with /cron.",
                                        path.display(),
                                        pyexec::sanitize_name(arg).unwrap_or_default()
                                    );
                                }
                                Err(e) => eprintln!("  ✗ {e}"),
                            },
                            None => println!(
                                "No Python block in Cyrene's last reply to save. Ask her to write \
                                 one first."
                            ),
                        }
                    }
                }
                "scripts" => {
                    let scripts = pyexec::list_scripts();
                    if scripts.is_empty() {
                        println!(
                            "\nNo saved scripts yet. Use /script <name> after Cyrene writes one.\n"
                        );
                    } else {
                        println!("\nSaved scripts (~/.cyrene/scripts):");
                        for s in &scripts {
                            println!("  - {s}   (run: /run {s})");
                        }
                        println!();
                    }
                }
                "cron" => {
                    // `/cron <name> <script> <schedule> [channel]` — schedule a
                    // saved script to run and deliver its output on a schedule.
                    let mut p = arg.splitn(4, char::is_whitespace);
                    let name = p.next().unwrap_or("").trim();
                    let script = p.next().unwrap_or("").trim();
                    let schedule = p.next().unwrap_or("").trim();
                    let channel = p.next().unwrap_or("").trim();
                    if name.is_empty() || script.is_empty() || schedule.is_empty() {
                        println!(
                            "Usage: /cron <name> <script> <schedule> [channel]\n  \
                             e.g. /cron flights flights 08:00 telegram:123456789\n  \
                             schedule: daily | hourly | HH:MM | a 5-field cron string\n  \
                             channel:  cli | telegram:<chat_id> | discord"
                        );
                    } else {
                        let ch = if channel.is_empty() { "cli" } else { channel };
                        match crons::add(name, schedule, script, ch) {
                            Ok(()) => println!(
                                "  Start the scheduler with `cyrene cron run` (keep it running), \
                                 or test now with `cyrene cron run-once --name {name}`."
                            ),
                            Err(e) => eprintln!("  ✗ {e}"),
                        }
                    }
                }
                "remember" | "note" => {
                    if arg.is_empty() {
                        println!("Usage: /remember <fact>   (kept across sessions)");
                    } else {
                        chatmem::record_fact(arg);
                        // Reflect it immediately in this session's context too.
                        history[0] = ChatMessage::system(rebuild_system_prompt());
                        println!("Got it — I'll remember that. 💛");
                    }
                }
                "memories" | "recall" => {
                    let facts = chatmem::facts();
                    let profile = chatmem::profile_notes();
                    if facts.is_empty() && profile.is_empty() {
                        println!("\nNo saved memories yet. Use /remember <fact> to add one.\n");
                    } else {
                        if !facts.is_empty() {
                            println!("\nWhat I remember:");
                            for (i, f) in facts.iter().enumerate() {
                                println!("  {}. {f}", i + 1);
                            }
                        }
                        if !profile.is_empty() {
                            println!("\nWhat I know about you:");
                            for (i, n) in profile.iter().enumerate() {
                                println!("  {}. {n}", i + 1);
                            }
                        }
                        println!();
                    }
                }
                "forget" => {
                    let n = chatmem::forget_facts();
                    history[0] = ChatMessage::system(rebuild_system_prompt());
                    println!(
                        "Cleared {n} saved memor{}.",
                        if n == 1 { "y" } else { "ies" }
                    );
                }
                "learn" => {
                    // `/learn <fact>` teaches Cyrene something; `/learn` alone
                    // shows everything she's learned — skills and memory.
                    if arg.is_empty() {
                        let skills = pyexec::list_scripts();
                        let facts = chatmem::facts();
                        println!("\nWhat Cyrene has learned:");
                        if skills.is_empty() && facts.is_empty() {
                            println!("  (nothing yet — ask me to build or remember something)");
                        }
                        if !skills.is_empty() {
                            println!("  Skills:");
                            for s in &skills {
                                println!("    - {s}   (run: /run {s})");
                            }
                        }
                        if !facts.is_empty() {
                            println!("  Memory:");
                            for f in &facts {
                                println!("    - {f}");
                            }
                        }
                        let profile = chatmem::profile_notes();
                        if !profile.is_empty() {
                            println!("  About you:");
                            for n in &profile {
                                println!("    - {n}");
                            }
                        }
                        println!();
                    } else {
                        chatmem::record_fact(arg);
                        history[0] = ChatMessage::system(rebuild_system_prompt());
                        println!("Learned it. 💛");
                    }
                }
                "telegram" => {
                    println!("\nConnecting to Telegram… (Ctrl-C to return)\n");
                    run_telegram();
                    println!();
                }
                "whatsapp" => {
                    println!("\nConnecting to WhatsApp… (Ctrl-C to return)\n");
                    run_whatsapp();
                    println!();
                }
                "fortune" => println!("\n  {}\n", random_fortune()),
                "exit" | "quit" | "q" => {
                    println!("Goodbye 💛");
                    break;
                }
                other => {
                    // Unknown command: surface the closest matches instead of a
                    // dead-end error (the Claude/Hermes "did you mean" behavior).
                    if slash::print_suggestions(other) == 0 {
                        println!("Unknown command `/{other}`. Type /help for the list.");
                    }
                }
            }
            continue;
        }

        if matches!(input, "exit" | "quit") {
            println!("Goodbye 💛");
            break;
        }

        // Pasting a @BotFather token connects Telegram on the spot — the
        // smoothest possible "hook me up to Telegram" path.
        if let Some(tok) = telegram::detect_token(input) {
            println!("\n  That looks like a Telegram bot token — saving it and connecting…\n");
            let _ = write_env_secrets(&env_path, &[("TELEGRAM_BOT_TOKEN".to_owned(), tok.clone())]);
            std::env::set_var("TELEGRAM_BOT_TOKEN", &tok);
            run_telegram();
            println!();
            continue;
        }

        // A bare command word (e.g. `model`) is almost always a missed slash;
        // nudge toward the command form, then still answer as chat.
        if let Some(c) = slash::lookup(input) {
            println!("  (tip: type /{} to run that command)", c.name);
        }

        let Some(active_model) = model.clone() else {
            println!("No active model. Use /connect to add a provider key, or /models.");
            continue;
        };

        // Remember every input so Cyrene can learn from how it's used and so
        // past sessions are recoverable (~/.cyrene/memory/chat.jsonl).
        chatmem::record_input(input);

        history.push(ChatMessage::user(input));
        complete_turn(&rt, &active_model, &mut history, &mut usage_total, verbose);
        // Self-learning: enact any skills/memory/schedules Cyrene put in her
        // reply, then offer to run the Python she wrote.
        apply_learned_actions(&mut history);
        offer_reply_python(
            &rt,
            &active_model,
            &mut history,
            &mut usage_total,
            verbose,
            autorun,
        );
    }
}

fn main() {
    let cli = Cli::parse();
    // Surface a one-line notice when a newer release exists. Rate-limited to one
    // network check per day and skipped for the commands that already check live.
    if !matches!(
        cli.command,
        Some(Commands::Update { .. } | Commands::Version)
    ) {
        update::maybe_notify();
    }

    match cli.command {
        None => {
            print_banner();
            // If the user has onboarded, drop straight into an interactive chat;
            // otherwise point them at onboarding.
            if cyrene_config::Config::load().is_ok() {
                run_chat();
            } else {
                println!(
                    "Run `cyrene --help` for available commands, or `cyrene onboard` to get started."
                );
            }
        }
        Some(cmd) => match cmd {
            Commands::Agent => {
                print_banner();
                run_chat();
            }
            Commands::Chat => {
                run_chat();
            }
            Commands::Telegram => {
                run_telegram();
            }
            Commands::Whatsapp => {
                run_whatsapp();
            }
            Commands::Gateway => {
                print_banner();
                println!("Starting Cyrene gateway...");
                println!("(Gateway not yet wired — runtime daemon pending)");
            }
            Commands::Dashboard => {
                print_banner();
                println!("Starting Cyrene dashboard on http://localhost:8080");
                println!("(Dashboard not yet wired)");
            }
            Commands::Onboard {
                non_interactive,
                provider,
                channel,
            } => {
                print_banner();
                run_onboarding(non_interactive, provider.as_deref(), channel.as_deref());
            }
            Commands::Doctor => {
                cmd_doctor();
            }
            Commands::Version => {
                update::show_version();
            }
            Commands::Update { check } => {
                update::run_update(check);
            }
            Commands::Model { action } => match action {
                ModelAction::List => cmd_model_list(),
                ModelAction::Status => {
                    println!("Model status:");
                    cmd_model_list();
                }
            },
            Commands::Skills { action } => match action {
                SkillsAction::List => cmd_skills_list(),
                SkillsAction::Bundles => {
                    println!("Installed Skill Bundles:");
                    let optional_dir = std::env::current_dir()
                        .unwrap_or_default()
                        .join("optional-skills");
                    if optional_dir.exists() {
                        if let Ok(entries) = std::fs::read_dir(&optional_dir) {
                            for entry in entries.flatten() {
                                let path = entry.path();
                                if path.is_dir() {
                                    let name = path.file_name().unwrap().to_string_lossy();
                                    let count = std::fs::read_dir(&path)
                                        .map(|e| {
                                            e.flatten()
                                                .filter(|f| {
                                                    f.path().extension().is_some_and(|e| e == "md")
                                                })
                                                .count()
                                        })
                                        .unwrap_or(0);
                                    println!("  {} ({} skills)", name, count);
                                }
                            }
                        }
                    } else {
                        println!("  (No optional skill bundles found)");
                    }
                }
            },
            Commands::Hub { action } => match action {
                HubAction::Search { query } => {
                    println!("Searching Skills Hub for: {query}");
                    println!("(Skills Hub client not yet connected)");
                }
                HubAction::Publish { skill } => {
                    println!("Publishing skill: {skill}");
                    println!("(Skills Hub client not yet connected)");
                }
                HubAction::Install { package } => {
                    println!("Installing package: {package}");
                    println!("(Skills Hub client not yet connected)");
                }
            },
            Commands::Cron { action } => match action {
                CronAction::List => crons::list(),
                CronAction::Add {
                    name,
                    schedule,
                    task,
                    channel,
                } => {
                    let ch = channel.unwrap_or_else(|| "cli".to_owned());
                    if let Err(e) = crons::add(&name, &schedule, &task, &ch) {
                        eprintln!("  ✗ {e}");
                    }
                }
                CronAction::Remove { name } => crons::remove(&name),
                CronAction::Run => crons::run_daemon(),
                CronAction::RunOnce { name } => {
                    if let Err(e) = crons::run_once(&name) {
                        eprintln!("  ✗ {e}");
                    }
                }
            },
            Commands::Service { action } => {
                let parse = |run: String| match service::ServiceJob::parse(&run) {
                    Ok(job) => Some(job),
                    Err(e) => {
                        eprintln!("  ✗ {e}");
                        None
                    }
                };
                match action {
                    ServiceAction::Install { run } => {
                        if let Some(job) = parse(run) {
                            service::install(job);
                        }
                    }
                    ServiceAction::Uninstall { run } => {
                        if let Some(job) = parse(run) {
                            service::uninstall(job);
                        }
                    }
                    ServiceAction::Status { run } => {
                        if let Some(job) = parse(run) {
                            service::status(job);
                        }
                    }
                }
            }
            Commands::Tools { action } => match action {
                ToolsAction::List => cmd_tools_list(),
            },
            Commands::Extensions { action } => match action {
                ExtensionsAction::List => cmd_extensions_list(),
            },
            Commands::Catalog { action } => match action {
                CatalogAction::List => cmd_catalog_list(),
                CatalogAction::Install { name } => {
                    println!("Installing catalog component: {name}");
                    println!("(Catalog install not yet wired)");
                }
            },
        },
    }
}

/// One selectable model provider in the onboarding wizard, plus everything
/// needed to write a working `config.toml` (and matching `.env` entry) for it.
struct ProviderSpec {
    /// Menu key accepted by `--provider` and shown in the list.
    key: &'static str,
    /// Human-readable label for the interactive menu.
    label: &'static str,
    /// Config `type` in `[providers.<type>.<alias>]`. Hosted gateways use their
    /// own preset name; bespoke endpoints use the generic `openai_compat` type.
    type_name: &'static str,
    /// Env-var name holding the API key (empty = no key needed, e.g. Ollama).
    api_key_env: &'static str,
    /// Default model written to config (empty = inherit the provider's default).
    model: &'static str,
    /// Provider tier (`"Local"` or `"Premium"`).
    tier: &'static str,
    /// When true, the wizard prompts for a base URL (custom OpenAI-compatible
    /// endpoints that have no built-in preset).
    needs_base_url: bool,
}

/// Every provider the onboarding wizard can configure. Hosted gateways
/// (deepseek, groq, xai, opencode, commandcode, …) are served by the preset
/// table in `cyrene-models`, so the config just names the `type` and key.
const PROVIDERS: &[ProviderSpec] = &[
    ProviderSpec {
        key: "ollama",
        label: "ollama (local, free — recommended to start)",
        type_name: "ollama",
        api_key_env: "",
        model: "llama3.1",
        tier: "Local",
        needs_base_url: false,
    },
    ProviderSpec {
        key: "openai",
        label: "openai (GPT-4o, o-series)",
        type_name: "openai",
        api_key_env: "OPENAI_API_KEY",
        model: "gpt-4o",
        tier: "Premium",
        needs_base_url: false,
    },
    ProviderSpec {
        key: "anthropic",
        label: "anthropic (Claude)",
        type_name: "anthropic",
        api_key_env: "ANTHROPIC_API_KEY",
        model: "claude-sonnet-4-5",
        tier: "Premium",
        needs_base_url: false,
    },
    ProviderSpec {
        key: "openrouter",
        label: "openrouter (300+ models, one key)",
        type_name: "openrouter",
        api_key_env: "OPENROUTER_API_KEY",
        model: "",
        tier: "Premium",
        needs_base_url: false,
    },
    ProviderSpec {
        key: "gemini",
        label: "gemini (Google)",
        type_name: "gemini",
        api_key_env: "GEMINI_API_KEY",
        model: "gemini-2.0-flash",
        tier: "Premium",
        needs_base_url: false,
    },
    ProviderSpec {
        key: "deepseek",
        label: "deepseek",
        type_name: "deepseek",
        api_key_env: "DEEPSEEK_API_KEY",
        model: "",
        tier: "Premium",
        needs_base_url: false,
    },
    ProviderSpec {
        key: "groq",
        label: "groq (fast inference)",
        type_name: "groq",
        api_key_env: "GROQ_API_KEY",
        model: "",
        tier: "Premium",
        needs_base_url: false,
    },
    ProviderSpec {
        key: "xai",
        label: "xai (Grok)",
        type_name: "xai",
        api_key_env: "XAI_API_KEY",
        model: "",
        tier: "Premium",
        needs_base_url: false,
    },
    ProviderSpec {
        key: "opencode",
        label: "opencode (OpenCode Zen gateway)",
        type_name: "opencode",
        api_key_env: "OPENCODE_ZEN_API_KEY",
        model: "",
        tier: "Premium",
        needs_base_url: false,
    },
    ProviderSpec {
        key: "opencode-go",
        label: "opencode-go (OpenCode Go subscription — open models)",
        type_name: "opencode-go",
        api_key_env: "OPENCODE_API_KEY",
        model: "",
        tier: "Premium",
        needs_base_url: false,
    },
    ProviderSpec {
        key: "commandcode",
        label: "commandcode (Command Code gateway)",
        type_name: "commandcode",
        api_key_env: "COMMANDCODE_API_KEY",
        model: "",
        tier: "Premium",
        needs_base_url: false,
    },
    ProviderSpec {
        key: "openai-compat",
        label: "openai-compat (any other OpenAI-compatible endpoint)",
        type_name: "openai_compat",
        api_key_env: "API_KEY",
        model: "",
        tier: "Premium",
        needs_base_url: true,
    },
];

/// The channels the wizard can configure, with the env var that holds each
/// channel's bot token (empty = no token needed).
const CHANNELS: &[(&str, &str, &str)] = &[
    // (key, label, token_env)
    ("cli", "cli (command-line — no setup needed)", ""),
    ("telegram", "telegram", "TELEGRAM_BOT_TOKEN"),
    ("slack", "slack", "SLACK_BOT_TOKEN"),
    ("discord", "discord", "DISCORD_BOT_TOKEN"),
];

/// Looks up a provider spec by `--provider` key, falling back to Ollama (the
/// zero-setup default) for an unrecognized value.
fn find_provider(key: &str) -> &'static ProviderSpec {
    PROVIDERS
        .iter()
        .find(|p| p.key == key)
        .unwrap_or(&PROVIDERS[0])
}

/// Looks up a channel spec by `--channel` key, falling back to CLI.
fn find_channel(key: &str) -> &'static (&'static str, &'static str, &'static str) {
    CHANNELS.iter().find(|c| c.0 == key).unwrap_or(&CHANNELS[0])
}

/// Captures the user's onboarding choices and any secrets they typed in, so the
/// wizard can both render `config.toml` and persist secrets to `.env`.
struct OnboardingChoice {
    provider: &'static ProviderSpec,
    /// Base URL for custom OpenAI-compatible providers (`needs_base_url`).
    base_url: Option<String>,
    channel: &'static (&'static str, &'static str, &'static str),
    /// Collected `(ENV_VAR, value)` secrets to write to `.env`.
    secrets: Vec<(String, String)>,
}

fn run_onboarding(non_interactive: bool, provider: Option<&str>, channel: Option<&str>) {
    println!("Welcome to Cyrene! Let's get you set up.\n");

    let cyrene_dir = cyrene_config::cyrene_home_dir().unwrap_or_default();
    if !cyrene_dir.exists() {
        let _ = std::fs::create_dir_all(&cyrene_dir);
    }

    // Non-interactive path for scripts, CI, and the end-to-end smoke test:
    // pick providers/channels from flags (defaulting to the zero-setup combo of
    // ollama + cli) without prompting, then write the config (R23.4). Secrets
    // are left for the environment to supply.
    let choice = if non_interactive {
        let provider = find_provider(provider.unwrap_or("ollama"));
        let channel = find_channel(channel.unwrap_or("cli"));
        println!("Running onboarding non-interactively.");
        println!("  Provider: {}", provider.type_name);
        println!("  Channel:  {}", channel.0);
        OnboardingChoice {
            provider,
            base_url: None,
            channel,
            secrets: Vec::new(),
        }
    } else {
        interactive_selection()
    };

    let config = build_onboarding_config(&choice);

    let config_path = cyrene_dir.join("config.toml");
    if let Err(e) = std::fs::write(&config_path, &config) {
        eprintln!("\n  Error writing config: {e}");
        return;
    }
    println!("\n✓ Configuration saved to: {}", config_path.display());

    // Hands-off secrets: when the chosen provider/channel needs any key, Cyrene
    // creates `.env` for the user (seeded from the bundled template) and fills in
    // whatever they typed — no manual `cp .env.example .env` step.
    let needs_env = !choice.provider.api_key_env.is_empty() || !choice.channel.2.is_empty();
    if needs_env || !choice.secrets.is_empty() {
        let env_path = cyrene_dir.join(".env");
        match write_env_secrets(&env_path, &choice.secrets) {
            Ok(()) => {
                if choice.secrets.is_empty() {
                    println!("✓ Created .env for you:   {}", env_path.display());
                    println!(
                        "  Add your {} there before running.",
                        choice.provider.api_key_env
                    );
                } else {
                    println!("✓ Keys saved to .env:     {}", env_path.display());
                }
            }
            Err(e) => eprintln!("  Warning: could not write .env ({e}); set the keys manually."),
        }
    }

    println!("\nNext steps:");
    println!("  1. Run `cyrene doctor` to verify your setup");
    println!("  2. Run `cyrene` (or `cyrene chat`) to start chatting");
    println!("\nHappy automating! 🚀");
}

/// Runs the interactive provider/channel wizard, prompting for any required API
/// keys/tokens and base URL up front so the user never has to hand-edit files.
fn interactive_selection() -> OnboardingChoice {
    println!("Step 1: Configure a Model Provider\n");

    println!("Available providers:");
    for (i, p) in PROVIDERS.iter().enumerate() {
        println!("  {}. {}", i + 1, p.label);
    }
    println!();

    let labels: Vec<&str> = PROVIDERS.iter().map(|p| p.label).collect();
    let idx = dialoguer::Select::new()
        .with_prompt("Select a model provider")
        .items(&labels)
        .default(0)
        .interact()
        .unwrap_or(0);
    let provider = &PROVIDERS[idx];
    println!("\n  Selected: {}", provider.key);

    let mut secrets: Vec<(String, String)> = Vec::new();
    let mut base_url = None;

    if provider.key == "ollama" {
        println!("  Ollama runs locally and needs no API key.");
        println!("  Make sure Ollama is running: https://ollama.com");
    } else {
        if provider.needs_base_url {
            let url: String = dialoguer::Input::new()
                .with_prompt("  Base URL (OpenAI-compatible /v1 endpoint)")
                .interact_text()
                .unwrap_or_default();
            if !url.trim().is_empty() {
                base_url = Some(url.trim().to_owned());
            }
        }
        // Prompt for the key right here and stash it for `.env`; no manual copy.
        let key: String = dialoguer::Password::new()
            .with_prompt(format!(
                "  Paste your {} (leave blank to set later)",
                provider.api_key_env
            ))
            .allow_empty_password(true)
            .interact()
            .unwrap_or_default();
        if key.trim().is_empty() {
            println!(
                "  No key entered — set {} in .env before running.",
                provider.api_key_env
            );
        } else {
            secrets.push((provider.api_key_env.to_owned(), key.trim().to_owned()));
            println!("  ✓ Key captured (will be saved to .env).");
        }
    }

    println!("\nStep 2: Configure a Channel\n");
    println!("Available channels:");
    for (i, c) in CHANNELS.iter().enumerate() {
        println!("  {}. {}", i + 1, c.1);
    }
    println!();

    let channel_labels: Vec<&str> = CHANNELS.iter().map(|c| c.1).collect();
    let cidx = dialoguer::Select::new()
        .with_prompt("Select a channel")
        .items(&channel_labels)
        .default(0)
        .interact()
        .unwrap_or(0);
    let channel = &CHANNELS[cidx];
    println!("\n  Selected: {}", channel.0);

    if channel.2.is_empty() {
        println!("  CLI channel works out of the box — no tokens needed.");
    } else {
        let token: String = dialoguer::Password::new()
            .with_prompt(format!(
                "  Paste your {} (leave blank to set later)",
                channel.2
            ))
            .allow_empty_password(true)
            .interact()
            .unwrap_or_default();
        if token.trim().is_empty() {
            println!(
                "  No token entered — set {} in .env before running.",
                channel.2
            );
        } else {
            secrets.push((channel.2.to_owned(), token.trim().to_owned()));
            println!("  ✓ Token captured (will be saved to .env).");
        }
    }

    OnboardingChoice {
        provider,
        base_url,
        channel,
        secrets,
    }
}

/// The `.env.example` template, embedded so Cyrene can scaffold a user's `.env`
/// itself — no `cp .env.example .env` and no repo checkout required.
const ENV_TEMPLATE: &str = include_str!("../../../.env.example");

/// Writes (or updates) `KEY=value` lines in a `.env` file without disturbing
/// unrelated entries. A missing `.env` is seeded from the bundled
/// [`ENV_TEMPLATE`] so the user gets a complete, documented file; existing keys
/// (template placeholders included) are replaced in place, and any new keys are
/// appended. The file is created with `0600` perms on Unix.
fn write_env_secrets(path: &std::path::Path, secrets: &[(String, String)]) -> std::io::Result<()> {
    let mut lines: Vec<String> = match std::fs::read_to_string(path) {
        Ok(existing) => existing.lines().map(str::to_owned).collect(),
        // No .env yet: start from the bundled template instead of an empty file.
        Err(_) => ENV_TEMPLATE.lines().map(str::to_owned).collect(),
    };

    for (key, value) in secrets {
        let prefix = format!("{key}=");
        let new_line = format!("{key}={value}");
        match lines
            .iter_mut()
            .find(|l| l.trim_start().starts_with(&prefix))
        {
            Some(existing) => *existing = new_line,
            None => lines.push(new_line),
        }
    }

    let mut body = lines.join("\n");
    body.push('\n');
    std::fs::write(path, body)?;

    // Secrets file: tighten permissions on Unix so other users can't read keys.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// Builds the onboarding `config.toml` for the chosen provider and channel.
/// Kept as a pure function so output is testable and shared between the
/// interactive and non-interactive paths.
fn build_onboarding_config(choice: &OnboardingChoice) -> String {
    let p = choice.provider;
    let (ptype, channel_type) = (p.type_name, choice.channel.0);

    let mut provider_block = format!("[providers.{ptype}.default]\n");
    if !p.model.is_empty() {
        provider_block.push_str(&format!("model = \"{}\"\n", p.model));
    }
    provider_block.push_str(&format!("tier = \"{}\"\n", p.tier));
    if !p.api_key_env.is_empty() {
        provider_block.push_str(&format!("api_key_env = \"{}\"\n", p.api_key_env));
    }
    if let Some(url) = &choice.base_url {
        provider_block.push_str(&format!("base_url = \"{url}\"\n"));
    }

    let mut channel_block = format!("[channels.{channel_type}.default]\n");
    if !choice.channel.2.is_empty() {
        channel_block.push_str(&format!("token_env = \"{}\"\n", choice.channel.2));
    }

    format!(
        r#"# Cyrene configuration — generated by `cyrene onboard`
# Edit this file to customize your setup.

{provider_block}
{channel_block}
[memory.sqlite.default]
path = "~/.cyrene/cyrene.db"

[autonomy]
low = "auto"
medium = "approval"
high = "blocked"
command_allowlist = ["git", "ls", "cat"]
"#,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(key: &str) -> &'static ProviderSpec {
        find_provider(key)
    }

    #[test]
    fn missing_env_is_seeded_from_template_with_key_filled_in() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");

        write_env_secrets(
            &path,
            &[("DEEPSEEK_API_KEY".to_owned(), "sk-real-123".to_owned())],
        )
        .unwrap();

        let body = std::fs::read_to_string(&path).unwrap();
        // The placeholder line is replaced in place (not duplicated)…
        assert!(body.contains("DEEPSEEK_API_KEY=sk-real-123"));
        assert!(!body.contains("DEEPSEEK_API_KEY=your-deepseek-key-here"));
        assert_eq!(body.matches("DEEPSEEK_API_KEY=").count(), 1);
        // …and the rest of the documented template is preserved.
        assert!(body.contains("OPENAI_API_KEY="));
    }

    #[test]
    fn unknown_key_is_appended_and_existing_entries_kept() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        std::fs::write(&path, "EXISTING=keep-me\n").unwrap();

        write_env_secrets(&path, &[("NEW_KEY".to_owned(), "val".to_owned())]).unwrap();

        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("EXISTING=keep-me"));
        assert!(body.contains("NEW_KEY=val"));
        // An existing file is NOT re-seeded from the template.
        assert!(!body.contains("OPENAI_API_KEY="));
    }

    #[test]
    fn preset_provider_config_omits_model_and_sets_key_env() {
        let choice = OnboardingChoice {
            provider: provider("commandcode"),
            base_url: None,
            channel: find_channel("cli"),
            secrets: Vec::new(),
        };
        let toml = build_onboarding_config(&choice);
        assert!(toml.contains("[providers.commandcode.default]"));
        assert!(toml.contains("api_key_env = \"COMMANDCODE_API_KEY\""));
        // Presets inherit their default model, so none is pinned in config.
        assert!(!toml.contains("model ="));
        // Config must parse as valid TOML.
        toml.parse::<toml::Value>().expect("valid TOML");
    }

    #[test]
    fn custom_compat_provider_writes_base_url_and_channel_token() {
        let choice = OnboardingChoice {
            provider: provider("openai-compat"),
            base_url: Some("https://example.test/v1".to_owned()),
            channel: find_channel("telegram"),
            secrets: Vec::new(),
        };
        let toml = build_onboarding_config(&choice);
        assert!(toml.contains("[providers.openai_compat.default]"));
        assert!(toml.contains("base_url = \"https://example.test/v1\""));
        assert!(toml.contains("[channels.telegram.default]"));
        assert!(toml.contains("token_env = \"TELEGRAM_BOT_TOKEN\""));
        toml.parse::<toml::Value>().expect("valid TOML");
    }

    #[test]
    fn unknown_provider_key_falls_back_to_ollama() {
        assert_eq!(find_provider("nonsense").key, "ollama");
        assert_eq!(find_channel("nonsense").0, "cli");
    }
}
