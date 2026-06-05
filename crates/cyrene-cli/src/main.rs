use clap::{Parser, Subcommand};

const BANNER: &str = r#"
   ╔════════════════════════════════════════════════════════╗
   ║                                                        ║
   ║    ██████╗██╗   ██╗██████╗ ███████╗███╗   ██╗███████╗  ║
   ║   ██╔════╝╚██╗ ██╔╝██╔══██╗██╔════╝████╗  ██║██╔════╝  ║
   ║   ██║      ╚████╔╝ ██████╔╝█████╗  ██╔██╗ ██║█████╗    ║
   ║   ██║       ╚██╔╝  ██╔══██╗██╔══╝  ██║╚██╗██║██╔══╝    ║
   ║   ╚██████╗   ██║   ██║  ██║███████╗██║ ╚████║███████╗   ║
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
            println!("  ○ No .env file found (copy .env.example to .env)");
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

fn run_onboarding(non_interactive: bool, provider: Option<&str>, channel: Option<&str>) {
    println!("Welcome to Cyrene! Let's get you set up.\n");

    let cyrene_dir = cyrene_config::cyrene_home_dir().unwrap_or_default();
    if !cyrene_dir.exists() {
        let _ = std::fs::create_dir_all(&cyrene_dir);
    }

    // Non-interactive path for scripts, CI, and the end-to-end smoke test:
    // pick providers/channels from flags (defaulting to the zero-setup combo of
    // ollama + cli) without prompting, then write the config (R23.4).
    let (provider_type, channel_type) = if non_interactive {
        let provider_type = normalize_provider(provider.unwrap_or("ollama"));
        let channel_type = normalize_channel(channel.unwrap_or("cli"));
        println!("Running onboarding non-interactively.");
        println!("  Provider: {provider_type}");
        println!("  Channel:  {channel_type}");
        (provider_type, channel_type)
    } else {
        interactive_selection()
    };

    let config = build_onboarding_config(provider_type, channel_type);

    let config_path = cyrene_dir.join("config.toml");
    if let Err(e) = std::fs::write(&config_path, &config) {
        eprintln!("\n  Error writing config: {e}");
        return;
    }

    println!("\n✓ Configuration saved to: {}", config_path.display());
    println!("\nNext steps:");
    println!("  1. Copy .env.example to .env and add your API keys");
    println!("  2. Run `cyrene doctor` to verify your setup");
    println!("  3. Run `cyrene gateway` to start Cyrene");
    println!("\nHappy automating! 🚀");
}

/// Maps a free-form provider name to a supported provider type, defaulting to
/// `ollama` for an unrecognized value.
fn normalize_provider(name: &str) -> &'static str {
    match name {
        "openai" => "openai",
        "anthropic" => "anthropic",
        "openrouter" => "openrouter",
        "gemini" => "gemini",
        "openai-compat" => "openai-compat",
        _ => "ollama",
    }
}

/// Maps a free-form channel name to a supported channel type, defaulting to
/// `cli` for an unrecognized value.
fn normalize_channel(name: &str) -> &'static str {
    match name {
        "telegram" => "telegram",
        "slack" => "slack",
        "discord" => "discord",
        _ => "cli",
    }
}

/// Runs the interactive provider/channel selection wizard and returns the
/// chosen provider and channel types.
fn interactive_selection() -> (&'static str, &'static str) {
    println!("Step 1: Configure a Model Provider");
    println!(
        "  Supported providers: openai, anthropic, openrouter, gemini, ollama, openai-compat\n"
    );

    let providers = vec![
        "ollama (local, free — recommended to start)",
        "openai",
        "anthropic",
        "openrouter",
        "gemini",
        "openai-compat (DeepSeek, xAI, Groq, etc.)",
    ];

    println!("Available providers:");
    for (i, p) in providers.iter().enumerate() {
        println!("  {}. {}", i + 1, p);
    }
    println!();

    let provider_idx = dialoguer::Select::new()
        .with_prompt("Select a model provider")
        .items(&providers)
        .default(0)
        .interact()
        .unwrap_or(0);

    let provider_type = match provider_idx {
        0 => "ollama",
        1 => "openai",
        2 => "anthropic",
        3 => "openrouter",
        4 => "gemini",
        5 => "openai-compat",
        _ => "ollama",
    };

    println!("\n  Selected: {provider_type}");

    if provider_type == "ollama" {
        println!("  Ollama runs locally and needs no API key.");
        println!("  Make sure Ollama is running: https://ollama.com");
    } else {
        let api_key_var = match provider_type {
            "openai" => "OPENAI_API_KEY",
            "anthropic" => "ANTHROPIC_API_KEY",
            "openrouter" => "OPENROUTER_API_KEY",
            "gemini" => "GEMINI_API_KEY",
            _ => "API_KEY",
        };
        println!("  You'll need to set {api_key_var} in your .env file.");
        println!("  Copy .env.example to .env and fill in your key.");
    }

    println!("\nStep 2: Configure a Channel");
    let channels = vec![
        "cli (command-line — no setup needed)",
        "telegram",
        "slack",
        "discord",
    ];

    println!("Available channels:");
    for (i, c) in channels.iter().enumerate() {
        println!("  {}. {}", i + 1, c);
    }
    println!();

    let channel_idx = dialoguer::Select::new()
        .with_prompt("Select a channel")
        .items(&channels)
        .default(0)
        .interact()
        .unwrap_or(0);

    let channel_type = match channel_idx {
        0 => "cli",
        1 => "telegram",
        2 => "slack",
        3 => "discord",
        _ => "cli",
    };

    println!("\n  Selected: {channel_type}");

    if channel_type == "cli" {
        println!("  CLI channel works out of the box — no tokens needed.");
    } else {
        println!("  Set the appropriate token in your .env file.");
    }

    (provider_type, channel_type)
}

/// Builds the onboarding `config.toml` contents for the chosen provider and
/// channel. Kept as a pure function so onboarding output is testable and shared
/// between the interactive and non-interactive paths.
fn build_onboarding_config(provider_type: &str, channel_type: &str) -> String {
    format!(
        r#"# Cyrene configuration — generated by `cyrene onboard`
# Edit this file to customize your setup.

[providers.{provider_type}.default]
model = "{}"
tier = "{}"

[channels.{channel_type}.default]

[memory.sqlite.default]
path = "~/.cyrene/cyrene.db"

[autonomy]
low = "auto"
medium = "approval"
high = "blocked"
command_allowlist = ["git", "ls", "cat"]
"#,
        if provider_type == "ollama" {
            "llama3.1"
        } else {
            "default"
        },
        if provider_type == "ollama" {
            "Local"
        } else {
            "Premium"
        },
    )
}
