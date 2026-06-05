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

## Remote Execution Backend

By default Cyrene runs every Step **locally**, inside the OS-level sandbox. You
can instead point the runtime at a **remote execution backend** — an SSH host or
a container host — so the heavy lifting runs off the local machine.

Selecting a remote backend changes only **where** a Step runs, never **whether**
it is gated. The autonomy policy, the workspace-boundary sandbox, and the
Approval Gate apply identically on every backend; the remote `remote_workspace`
simply becomes the boundary that confinement is enforced against. There is no
config switch that disables gating for a remote backend.

```toml
[execution]
backend = "ssh"            # one of: "local" (default), "ssh", "container"

[execution.ssh]
host = "build.example.com"
user = "cyrene"
port = 22
key_env = "CYRENE_SSH_KEY"                   # env var holding the key PATH
remote_workspace = "/srv/cyrene/workspace"   # boundary enforced on the remote
strict_host_key_checking = true
```

```toml
[execution]
backend = "container"

[execution.container]
host = "tcp://10.0.0.5:2376"          # omit for the local Docker socket
image = "cyrene/runner:latest"
remote_workspace = "/workspace"        # boundary enforced inside the container
credential_env = "CYRENE_DOCKER_TLS"  # env var for a TLS cert path, if needed
```

As everywhere, **no secret values live in the config**: an SSH key or a TLS cert
is referenced only by the *name* of the environment variable that holds its path
(`key_env`, `credential_env`), resolved from your environment or `.env`. Omit the
`[execution]` section entirely to keep the secure local default.

See [Deployment](deployment.md) for how remote backends preserve the safety
pipeline and for a low-resource unattended deployment guide.
