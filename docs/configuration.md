# Configuration

Cyrene is configured by a single TOML file at `~/.cyrene/config.toml`.

## Example Configuration

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

## Secrets

Secrets are **never** stored in the config file. Each entry references a secret by the name of the environment variable that holds it (e.g. `api_key_env`). Values come from your environment or `.env` file.

## Autonomy Levels

| Level | Default | Description |
|-------|---------|-------------|
| `low` | `auto` | Low-risk steps run automatically |
| `medium` | `approval` | Medium-risk steps require user approval |
| `high` | `blocked` | High-risk steps are blocked until autonomy is raised |
