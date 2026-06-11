<p align="center">
  <img src="cyrenelogo.png" alt="Cyrene Logo" width="300">
</p>

<h1 align="center">Cyrene</h1>

<p align="center">
  <strong>The AI agent that always loves you</strong>
</p>

<p align="center">
  <em>"Of course, this will be a romantic story like none that has come before. You think so too, right?"</em> — Cyrene
</p>

<p align="center">
  <a href="https://github.com/YourWisemaker/cyrene/blob/master/LICENSE">
    <img src="https://img.shields.io/badge/license-Apache--2.0-blue" alt="License">
  </a>
  <a href="https://github.com/YourWisemaker/cyrene/actions/workflows/ci.yml">
    <img src="https://img.shields.io/github/actions/workflow/status/YourWisemaker/cyrene/ci.yml?branch=master" alt="CI">
  </a>
  <a href="https://github.com/YourWisemaker/cyrene">
    <img src="https://img.shields.io/badge/Rust-1.82+-orange?logo=rust" alt="Rust">
  </a>
</p>

<p align="center">
  <img src="cyrene-book.jpg" alt="The story of Cyrene" width="600">
</p>

---

## Why Cyrene?

Most open-source agents do one thing well but leave gaps everywhere else. Some are secure but don't learn. Some learn but aren't fast or auditable. Some run everywhere but aren't safe. Cyrene fuses the best ideas from the field into a single binary — secure, learning, omnipresent, efficient, and auditable — and fills the gaps none of them cover alone.

**Cyrene's unique combination:**

- **Single Rust binary** — sub-100ms latency, <1% idle CPU, zero external services (SQLite only). No Python, no Node, no Docker required.
- **Safety as a composable pipeline** — injection scan → plan → shadow execution → approval gate → execute → receipt → checkpoint. A mandatory chain, not a toggle.
- **Signed, hash-chained audit trail** — every action produces an Ed25519-signed, SHA-256 hash-chained receipt. Append-only and tamper-evident.
- **Memory that can't be poisoned or hijacked** — two independent trust boundaries guard the knowledge graph. Untrusted content (a web page like `evil.com`, tool output) is injection-scanned *before* it can be stored and neutralized on recall, so it can never resurface as a smuggled instruction; and memory is owned by the authenticated user, so a spoofed or hijacked session is refused at the write — only the owner can read or rewrite what Cyrene remembers.
- **Self-improvement at native speed** — skills are generated, tested, saved, and improved without Python overhead.
- **Multi-model routing with budget guardrails** — local models handle the common case; the router escalates only on repeated failure and only if budget allows.
- **Extension SDK with permission scoping** — plugins declare what they need and get sandboxed accordingly.

> Secure, learning, omnipresent, efficient, and auditable — all in a single binary that always loves you.

---

## What is Cyrene?

Cyrene is the AI agent that always loves you — open-source, self-improving, and written in Rust. It connects to any messaging channel (Telegram, WhatsApp, Discord, email, CLI, and more), receives tasks, plans and executes them safely, and improves its own skills over time — all while keeping you in control through a comprehensive safety pipeline.

Cyrene doesn't just answer — she **builds the tools to get things done**. When a task needs computation, scraping, an API call, or automation, she writes a complete Python program, runs it, and saves it as a reusable skill you can re-run or schedule. She curates her own memory, keeps an evolving picture of who you are, and behaves identically whether you reach her from the terminal or from Telegram/WhatsApp.

### Key Features

- **Trait-based modularity** — swap out any Channel, Memory, Model, or Tool via config
- **Safety pipeline** — every request flows through: injection scan → plan → shadow execution → approval gate → execute → receipt → checkpoint
- **Writes her own tools** — generates Python integrations for scraping, APIs, and automation, runs them, interprets the output, and saves the good ones as skills
- **Self-improvement** — generates, tests, and saves reusable `SKILL.md` definitions
- **Agent-curated memory + user model** — remembers durable facts and builds a deepening picture of who you are across sessions; the persona is editable via `~/.cyrene/SOUL.md`
- **Scheduled automations** — natural-language recurring tasks ("every day at 7am, summarize X and message me") run as full agent turns and deliver back to the chat they were set up in
- **Always-on** — `cyrene service install` registers a background service (launchd on macOS, systemd on Linux) so the scheduler and chatbot keep running across reboots
- **Same brain on every channel** — CLI, Telegram, and WhatsApp all run the full agent loop (persona, Python execution, memory, scheduling), not a stripped-down echo
- **Multi-channel** — CLI, Telegram, Slack, Discord, WhatsApp, email, Signal, Matrix
- **Live command menu** — a Claude-style `/` menu filters commands as you type, right in the terminal
- **Bundled skill library** — 200+ curated skills across 20 categories
- **Extension SDK** — load custom providers, channels, and tools via `cyrene.plugin.toml`
- **Hardware integration** — optional GPIO/I2C/SPI/serial support with companion firmware
- **Signed audit trail** — every action produces a hash-chained, Ed25519-signed receipt
- **Multi-model** — OpenAI, Anthropic, Gemini, OpenRouter, Ollama, and any OpenAI-compatible endpoint
- **Self-hostable** — install via npm, Homebrew, Docker, Nix, PowerShell, or bare-metal; runs on Linux, macOS, Windows, and Raspberry Pi

---

## Quick Start

### Install

**npm (cross-platform — Linux, macOS, Windows)** — the package downloads the
prebuilt binary for your platform on install:

```bash
npm install -g cyrene-agent     # or: pnpm add -g cyrene-agent  /  yarn global add cyrene-agent
npx cyrene-agent --help         # run once without installing
```

**Linux / macOS / Raspberry Pi** — downloads the prebuilt binary for your platform:

```bash
curl -fsSL https://raw.githubusercontent.com/YourWisemaker/cyrene/master/install.sh | bash
```

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/YourWisemaker/cyrene/master/install.ps1 | iex
```

**Other package managers:**

```bash
nix build github:YourWisemaker/cyrene                  # Nix
```

**Raspberry Pi:** both 64-bit (aarch64) and 32-bit (armv7) Raspberry Pi OS are
detected automatically by the install script — no extra flags needed.

See [docs/installation.md](docs/installation.md) for all methods and options.

#### Supported Platforms

Prebuilt single-binary releases are published for every platform below; the
installers pick the right one automatically.

| OS | Architecture | Target | Notes |
|----|--------------|--------|-------|
| Linux | x86_64 | `x86_64-unknown-linux-gnu` / `-musl` | musl build is fully static (Alpine, minimal distros) |
| Linux | aarch64 | `aarch64-unknown-linux-gnu` / `-musl` | 64-bit ARM servers, Raspberry Pi OS 64-bit |
| Linux | armv7 | `armv7-unknown-linux-gnueabihf` | Raspberry Pi OS 32-bit (Pi 3+) |
| macOS | x86_64 | `x86_64-apple-darwin` | Intel Macs |
| macOS | aarch64 | `aarch64-apple-darwin` | Apple Silicon (M-series) |
| Windows | x86_64 | `x86_64-pc-windows-msvc` | Windows 10/11 x64 |
| Windows | aarch64 | `aarch64-pc-windows-msvc` | Windows on ARM64 |

### Build from Source

**Prerequisites:** Rust 1.82+ with Cargo. If you don't have it yet, install via [rustup](https://rustup.rs):

```bash
# Linux/macOS — installs rustc + cargo and adds them to your PATH
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"      # or restart your shell

# Verify
cargo --version
```

Then build and install the binary:

```bash
git clone https://github.com/YourWisemaker/cyrene.git
cd cyrene
cargo build --release --bin cyrene
cp target/release/cyrene /usr/local/bin/
```

#### If `cp` fails with "Permission denied"

`/usr/local/bin` is usually root-owned, so a plain `cp` can't write there. Pick any one of these:

```bash
# Option A — install system-wide with elevated privileges
sudo cp target/release/cyrene /usr/local/bin/

# Option B — install to a user-owned directory (no sudo needed)
mkdir -p ~/.local/bin
cp target/release/cyrene ~/.local/bin/
# Then make sure ~/.local/bin is on your PATH (add to ~/.zshrc or ~/.bashrc):
export PATH="$HOME/.local/bin:$PATH"

# Option C — let Cargo install it to ~/.cargo/bin (already on PATH after rustup)
cargo install --path crates/cyrene-cli
```

### Uninstall

```bash
# Remove the binary from wherever you installed it:
sudo rm /usr/local/bin/cyrene     # if installed system-wide
rm ~/.local/bin/cyrene            # if installed to ~/.local/bin
cargo uninstall cyrene-cli        # if installed via `cargo install`

# Optionally remove configuration, database, and state:
rm -rf ~/.cyrene
```

### Docker

```bash
docker compose -f docker/docker-compose.yml up -d
```

### Nix

```bash
nix build github:YourWisemaker/cyrene
```

### Get Started

```bash
cyrene onboard     # Interactive setup wizard
cyrene doctor      # Check your configuration
cyrene             # Start chatting (or: cyrene chat)
```

Inside the chat, a Claude-style `/` menu appears as you type and filters live. Slash commands manage the session without leaving it:

```text
/model [name]   Switch provider/model (no arg opens a live picker)
/models         List configured providers (● = active)
/connect        Add or update a provider + API key
/py /run        Run inline Python, or a saved script
/key NAME val   Save an API key/secret to .env for scripts
/script /cron   Save the last Python as a skill, or schedule it
/remember       Save a durable fact; /memories shows what she knows
/telegram       Connect this chat to Telegram (also /whatsapp)
/status /usage  Active provider/model and token usage
/history /save  View or save the transcript
/retry /undo    Re-run or remove the last exchange
/tools /skills  List built-in tools and bundled skills
/help           Full command list
```

The model picker fetches each provider's catalog live (OpenAI `/v1/models`,
Ollama `/api/tags`), so you choose from the models your key actually has access
to — including all OpenCode Go open models.

### Self-learning in action

You don't drive Cyrene with slash commands — you just ask. She writes the
Python, runs it, learns, and schedules follow-ups on her own:

```text
you ▸ track the cheapest JOG→Tokyo flights and message me on Telegram every morning at 7

cyrene ▸ (writes a Python scraper, runs it, shows today's result, then:)
  💾 Saved a reusable skill `flights`.
  ⏰ Scheduled recurring task `flights` (07:00) — each run I'll think it
     through and deliver here.
  💛 noted about you: flies JOG→Tokyo, prefers cheap fares
```

Recurring tasks created with natural language run as full agent turns (model +
Python + memory), so a daily job can scrape, reason about what's new, post an
update, and remember what it learned — not just re-run a static script.

To keep those jobs and bots running across reboots, install the background
service once:

```bash
cyrene service install                 # always-on scheduler (launchd/systemd)
cyrene service install --run telegram  # keep the Telegram bot always-on
cyrene service status                  # check it
```

Keep Cyrene current at any time — `cyrene update` re-runs the right installer for
your platform (PowerShell on Windows, the shell script elsewhere):

```bash
cyrene update          # download and install the latest release
cyrene update --check  # check whether an update is available
cyrene version         # show installed vs. latest version
```

---

## Architecture

```
User/Event → Channel Gateway → Injection Scanner → Model Router
    → Shadow Executor (if irreversible) → Approval Gate
    → Executor → Checkpoint → Receipt Ledger → Response
```

### Workspace Crates

| Crate | Purpose |
|-------|---------|
| `cyrene-core` | Domain types, traits, error model |
| `cyrene-config` | TOML config loading, Plugin Registry, extension loader |
| `cyrene-sdk` | Extension SDK: public traits + Host API |
| `cyrene-ledger` | Signed, append-only receipt ledger |
| `cyrene-state` | Git-style state tree and checkpoints |
| `cyrene-safety` | Sandbox, shadow executor, approval gate, injection scanner |
| `cyrene-models` | Model providers, router, budget guard |
| `cyrene-runtime` | Agent loop, daemon, supervisor |
| `cyrene-channels` | Channel gateway and built-in messaging channels |
| `cyrene-memory` | SQLite-backed knowledge graph |
| `cyrene-skills` | Skill engine, library, and bundles |
| `cyrene-tools` | Tool registry, built-in tools, MCP client |
| `cyrene-events` | Webhook listener, heartbeat engine, cron scheduler |
| `cyrene-hardware` | GPIO/I2C/SPI/serial peripheral control (optional) |
| `cyrene-cli` | CLI binary, onboarding, doctor |
| `cyrene-dashboard` | Local web dashboard control plane |
| `cyrene-bridge` | Workspace bridge (browser, terminal, cloud) |

---

## Configuration

Cyrene is configured by a single TOML file at `~/.cyrene/config.toml`:

```toml
[providers.openai.coding]
model = "gpt-4o"
tier = "Premium"
api_key_env = "OPENAI_API_KEY"

[providers.ollama.local]
model = "llama3.1"
tier = "Local"

[channels.cli.default]

[channels.telegram.personal]
token_env = "TELEGRAM_BOT_TOKEN"
allowlist = ["123456789"]

[memory.sqlite.default]
path = "~/.cyrene/cyrene.db"

[autonomy]
low = "auto"
medium = "approval"
high = "blocked"
command_allowlist = ["git", "ls", "cat"]
```

Secrets are loaded from environment variables or a `.env` file — never from the config.

---

## Skills Library

Cyrene ships with **200+ bundled skills** across categories:

| Category | Examples |
|----------|---------|
| software-development | code-review, debug-error, write-unit-test |
| devops | docker-compose, ci-pipeline, k8s-deployment |
| data-science | data-cleaning, model-train, sql-query |
| ai-ml | prompt-engineer, rag-build, agent-design |
| security | vulnerability-scan, threat-model, secret-scan |
| productivity | task-prioritize, project-plan, workflow-automate |
| ...and 14 more | finance, creative, communication, smart-home, etc. |

Optional skill bundles are available under `optional-skills/` for specialized domains.

---

## Extensions

Cyrene uses a plugin architecture with `cyrene.plugin.toml` manifests:

```toml
name = "my-provider"
version = "1.0.0"
capabilities = ["model_provider"]
host_compat = ">=0.1.0"

[permissions]
network = true
secrets = ["MY_API_KEY"]
```

List installed extensions:
```bash
cyrene extensions list
```

---

## Hardware & Firmware

Cyrene can control hardware peripherals (GPIO, I2C, SPI, serial) through the optional `cyrene-hardware` crate and companion firmware for ESP32.

Build and flash firmware:
```bash
cd firmware/esp32
idf.py build && idf.py flash
```

---

## Development

### Prerequisites

- Rust 1.82+ (install via [rustup](https://rustup.rs))

### Build & Test

```bash
cargo build --workspace
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

### Fuzz Testing

`cargo-fuzz` (libFuzzer) harnesses fuzz every untrusted-input parser. libFuzzer
requires a nightly toolchain.

```bash
cargo install cargo-fuzz
rustup toolchain install nightly

# Available targets: fuzz_config, fuzz_tool_params, fuzz_json_payload, fuzz_skill_parse
cargo +nightly fuzz run fuzz_config
cargo +nightly fuzz run fuzz_tool_params
```

Crashing inputs are retained as regression cases under `fuzz/corpus/<target>/`
and replayed on every run. See [`fuzz/README.md`](fuzz/README.md) for the full
target list and the crash-to-regression workflow.

---

## Deployment

See [`docs/deployment.md`](docs/deployment.md) for the full guide: a low-resource
unattended deployment within the idle budget, and remote execution backends
(SSH / container host) that preserve autonomy, sandboxing, and approval.

### Docker

```bash
docker compose -f docker/docker-compose.yml up -d
```

### Remote execution backend

Run Steps on a remote SSH host or container host instead of locally — without
loosening any safety constraint. Add an `[execution]` section to `config.toml`:

```toml
[execution]
backend = "ssh"            # "local" (default), "ssh", or "container"

[execution.ssh]
host = "build.example.com"
key_env = "CYRENE_SSH_KEY"                   # key PATH via env var, never inline
remote_workspace = "/srv/cyrene/workspace"   # boundary enforced on the remote
```

Selecting a remote backend changes only *where* a Step runs — the autonomy
policy, workspace-boundary sandbox, and Approval Gate apply identically on every
backend. See [`docs/deployment.md`](docs/deployment.md).

### NixOS

```nix
# In your flake.nix
inputs.cyrene.url = "github:YourWisemaker/cyrene";

# In your configuration.nix
services.cyrene.enable = true;
```

### Marketplace Templates

Pre-built templates for self-hosting platforms:
- `marketplace/coolify/`
- `marketplace/dokploy/`
- `marketplace/easypanel/`

---

## CLI Commands

```
cyrene                Start chatting (interactive REPL with live / menu)
cyrene agent          Start Cyrene in agent mode (interactive chat)
cyrene chat           Start an interactive chat with Cyrene
cyrene telegram       Run the Telegram bot (full agent loop)
cyrene whatsapp       Run the WhatsApp Cloud API bridge (full agent loop)
cyrene gateway        Start the runtime gateway
cyrene dashboard      Start the web dashboard
cyrene service ...    Install/uninstall/status an always-on background service
cyrene onboard        Run the setup wizard
cyrene doctor         Check system health
cyrene model list     List configured model providers
cyrene skills list    List bundled skills
cyrene extensions list List installed extensions
cyrene catalog list   List optional components
cyrene tools list     List available tools
cyrene cron list      List scheduled jobs
cyrene cron run       Run the scheduler in the foreground
cyrene cron run-once  Fire a single job now (for testing)
cyrene update         Update to the latest release (--check to only check)
cyrene version        Show installed and latest version
```

---

## Security Model

Cyrene implements defense-in-depth:

1. **Autonomy Policy** — risk classification with secure defaults (medium=approval, high=blocked)
2. **Sandboxing** — OS-level confinement (Landlock/Seatbelt/Job Objects)
3. **Shadow Execution** — dry-run before irreversible actions
4. **Approval Gates** — human-in-the-loop for high-stakes decisions
5. **Receipt Ledger** — immutable, signed, hash-chained audit trail
6. **Injection Scanner** — defense against prompt injection attacks

See [docs/security.md](docs/security.md) for the full security model.

---

## Contributing

See [AGENTS.md](AGENTS.md) for architecture details, build commands, and contribution guidelines.

1. Fork the repository
2. Create a feature branch
3. Run `cargo fmt` and `cargo clippy`
4. Write tests for new functionality
5. Submit a pull request

---

## License

Cyrene is licensed under the [Apache License 2.0](LICENSE).

---

<p align="center">
  <strong>Cyrene</strong> — The AI agent that always loves you.
</p>
