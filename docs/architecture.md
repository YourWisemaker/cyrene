# Architecture

Cyrene is built as a Rust Cargo workspace with each subsystem in its own crate.

## Request Lifecycle

```
User/Event → Channel Gateway → Injection Scanner → Model Router
    → Shadow Executor (if irreversible) → Approval Gate
    → Executor → Checkpoint → Receipt Ledger → Response
```

## Core Traits

- **Channel** — external messaging surface (CLI, Telegram, Slack, etc.)
- **Memory** — knowledge graph backend (SQLite, pluggable)
- **Model** — LLM provider endpoint (OpenAI, Anthropic, Ollama, etc.)
- **Tool** — discrete capability the agent can invoke

## Safety Pipeline

Every request flows through a fixed safety pipeline:

1. **Injection Scanner** — inspects untrusted content for prompt injections
2. **Planning** — model selects tools and creates a plan
3. **Shadow Execution** — if irreversible, runs plan in sandbox first
4. **Approval Gate** — halts before irreversible actions, requests user approval
5. **Real Execution** — performs approved actions within workspace boundary
6. **Receipt** — logs every action in signed, append-only ledger
7. **Checkpoint** — snapshots state for rollback

## Workspace Crates

Each subsystem lives in its own crate under `crates/*`, so it builds and tests in
isolation and the dependency graph enforces the layering: `cyrene-core` depends on
nothing app-specific, adapters depend on core, and `cyrene-cli` wires everything
together.

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
| `cyrene-acp` | Agent Client Protocol adapter — drive the agent loop from an editor/IDE over JSON-RPC |
| `cyrene-presence` | Presence + Persona engines — real-time thinking signals and the editable persona |
| `cyrene-trajectory` | Trajectory compressor — distills a subagent's execution log into a reusable blueprint |
| `cyrene-compress` | RTK-style compression of tool output before it enters model context |
| `cyrene-render` | Report renderer — formats agent output as PDF and interactive HTML |
| `cyrene-hub` | Skills Hub client — publish, search, and install community skills |

## Adding a New Component

To add a new Channel, Memory, or Model:

1. Create a new crate under `crates/`
2. Implement the appropriate trait from `cyrene-core`
3. Register a factory in the Plugin Registry
4. Add configuration support in `cyrene-config`
5. Document the new component in the config example
