use clap::{Parser, Subcommand};

mod update;

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
}

fn cmd_doctor() {
    println!("Cyrene Doctor — checking system health\n");

    let cyrene_dir = cyrene_config::cyrene_home_dir().unwrap_or_default();

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

    let env_path = std::env::current_dir().ok().map(|d| d.join(".env"));
    match env_path {
        Some(p) if p.exists() => {
            println!("  ✓ .env file found: {}", p.display());
        }
        _ => {
            println!("  ○ No .env file found (run `cyrene onboard` to add your keys)");
        }
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

fn cmd_cron_list() {
    println!("Cron Jobs:\n");
    println!("  (No cron jobs configured yet. Use `cyrene cron add` to create one.)");
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

fn main() {
    let cli = Cli::parse();

    // Surface a one-line notice when a newer release exists. Rate-limited to one
    // network check per day and skipped for `update` itself (which checks live).
    if !matches!(cli.command, Some(Commands::Update { .. })) {
        update::maybe_notify();
    }

    match cli.command {
        None => {
            print_banner();
            println!(
                "Run `cyrene --help` for available commands, or `cyrene onboard` to get started."
            );
        }
        Some(cmd) => match cmd {
            Commands::Agent => {
                print_banner();
                println!("Starting Cyrene agent mode...");
                println!("(Agent loop not yet wired — run the runtime daemon instead)");
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
                CronAction::List => cmd_cron_list(),
                CronAction::Add {
                    name,
                    schedule,
                    task,
                    channel,
                } => {
                    println!("Adding cron job: {name}");
                    println!("  Schedule: {schedule}");
                    println!("  Task: {task}");
                    if let Some(ch) = channel {
                        println!("  Channel: {ch}");
                    }
                    println!("(Cron scheduler not yet wired)");
                }
                CronAction::Remove { name } => {
                    println!("Removing cron job: {name}");
                    println!("(Cron scheduler not yet wired)");
                }
            },
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
        let env_path = std::env::current_dir().unwrap_or_default().join(".env");
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
    println!("  2. Run `cyrene gateway` to start Cyrene");
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
