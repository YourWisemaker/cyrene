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

## Adding a New Component

To add a new Channel, Memory, or Model:

1. Create a new crate under `crates/`
2. Implement the appropriate trait from `cyrene-core`
3. Register a factory in the Plugin Registry
4. Add configuration support in `cyrene-config`
5. Document the new component in the config example
