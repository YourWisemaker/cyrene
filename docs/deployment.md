# Deployment

This guide covers running Cyrene as an unattended service on a low-resource host
(Requirement 33.4) and pointing the runtime at a remote execution backend —
SSH or a container host — that still honors every safety constraint
(Requirement 33.5).

- [Low-Resource Unattended Deployment](#low-resource-unattended-deployment)
- [Remote Execution Backends](#remote-execution-backends)

---

## Low-Resource Unattended Deployment

Cyrene is designed to live on a small, always-on host — a "$5 VPS" class machine
(1 vCPU, 512MB–1GB RAM) — and run unattended within the Requirement 1 idle
budget: **no more than 1% average CPU over any 60-second window while idle.**

### Why it fits the idle budget

The runtime holds idle CPU under the budget by design, not by tuning:

- **Event-driven, no busy loops.** The Tokio reactor parks until a channel
  message, a webhook, a heartbeat tick, or a cron tick wakes it. Between events
  the process consumes effectively no CPU.
- **Single embedded datastore.** All state lives in one SQLite file — there are
  no companion database servers, brokers, or sidecars idling alongside it.
- **One process.** The runtime + gateway are a single binary, so there is no
  inter-service chatter on the idle path.

The compose file bounds the *burst* ceiling (`cpus: "1.0"`, `memory: 512M`) so a
single agent cannot starve a shared host. These limits do not affect the idle
figure — they cap spikes during active work.

### One-command unattended deployment

```bash
cp .env.example .env        # fill in only the secrets you actually use
docker compose -f docker/docker-compose.yml up -d
```

That brings up the runtime + gateway with:

- **Persistent state on a single volume.** Everything mutable (the SQLite db,
  the Receipt Ledger, State Tree blobs, and the Skill Library) lives under
  `/home/cyrene/.cyrene/data`, mounted as the `cyrene-data` volume, so state
  survives restarts (R33.2).
- **Secrets from the environment only.** `env_file: ../.env` feeds secrets in at
  runtime; nothing secret is baked into the image (R33.3).
- **Automatic restart.** `restart: unless-stopped` plus a `cyrene doctor`
  healthcheck means the host recovers the service on crash or reboot without a
  human present.
- **A bounded log footprint.** JSON logs roll at `10m × 3` files so logs never
  fill a small disk.

### Verifying the idle budget on your host

After the container has been up and idle (no active sessions) for a minute:

```bash
# Average CPU over a short window — should sit well under 1%.
docker stats --no-stream cyrene-agent
```

`docker stats` reports the live CPU percentage; for an idle agent it should read
a small fraction of a percent. If it does not, check that no channel is
hot-polling (most channels are push/long-poll) and that no heartbeat or cron job
is scheduled to run continuously.

### Running without Docker

On a host where you prefer a native service, install the binary and register it
with the OS service manager (the same path `cyrene` uses on a desktop):

- **Linux** — a `systemd` **user** unit started with `systemctl --user enable
  --now cyrene`.
- **macOS** — a `launchd` agent.
- **Windows** — a Windows Service.

The service model is identical to the always-on daemon described in
[Architecture](architecture.md); the container just wraps it. Either way the
idle path is event-driven, so the idle budget holds.

### Hardening the network surface

The gateway is network-exposed and **requires authentication** (R22.5). The
compose file binds it to `127.0.0.1:8080` by default so it is not reachable from
the public internet. To expose it, put a TLS-terminating reverse proxy (Caddy,
nginx, Traefik) in front of it and keep authentication enabled. The container
also runs as a non-root user with `no-new-privileges:true` as defense-in-depth
around the in-process OS sandbox.

---

## Remote Execution Backends

By default Cyrene executes every Step **locally**, inside the OS-level sandbox
and behind the autonomy policy and Approval Gate. A Maintainer can instead point
the runtime at a **remote execution backend** so Steps run off the local
machine:

- **SSH** — run Steps on a remote host over SSH.
- **Container host** — run Steps inside a container on a (possibly remote)
  Docker-compatible host.

This mirrors the multiple terminal backends of comparable agents while keeping
Cyrene's safety pipeline intact.

### The core guarantee: location changes, gating does not

Selecting a remote backend changes only **where** a Step runs, never **whether**
it is gated. Concretely, the same three constraints from
[the security model](security.md) apply on every backend (R22, R6):

| Constraint | How it is preserved remotely |
|---|---|
| **Autonomy** | Every command is gated by the same `AutonomyPolicy` *before* a backend is ever chosen. The decision is **backend-invariant**: a command that needs approval locally needs approval over SSH too. |
| **Sandboxing** | Each backend carries a **workspace boundary** (`remote_workspace`) — a directory on the remote host or inside the container. Prepared commands always run with that boundary as their working directory, and the boundary check rejects paths outside it, including `..` traversal. |
| **Approval** | Gating happens *before* the invocation is rendered, so the runtime cannot dispatch a command to any backend without first clearing the Approval Gate. |

There is deliberately **no** config switch that disables gating for a remote
backend. Choosing a remote backend is an explicit, reviewable config edit, just
like raising the autonomy level (R22.4).

### How a command is dispatched

When a Step's command clears the gate, the backend renders an exact,
ready-to-spawn invocation using an argument **vector** (never a
shell-interpolated string). The user command is carried as a single opaque
argument, prefixed with a `cd <boundary>` so it runs inside the workspace
boundary — the *local* shell never re-parses it:

- **SSH** → `ssh -p <port> -o StrictHostKeyChecking=… user@host -- 'cd <remote_workspace> && <cmd>'`
- **Container** → `docker [-H <host>] run --rm -w <remote_workspace> <image> sh -c 'cd <remote_workspace> && <cmd>'`

If the command is withheld by the policy (approval required or blocked), **no
invocation is produced at all**, so it can never reach the backend.

### SSH backend

```toml
[execution]
backend = "ssh"

[execution.ssh]
host = "build.example.com"
user = "cyrene"
port = 22
key_env = "CYRENE_SSH_KEY"                   # env var holding the key PATH
remote_workspace = "/srv/cyrene/workspace"   # boundary enforced on the remote
strict_host_key_checking = true              # turn off only on trusted networks
```

```bash
# .env
CYRENE_SSH_KEY=/home/you/.ssh/cyrene_remote_ed25519
```

Use a **dedicated, least-privilege key** and a dedicated remote user whose shell
access is scoped to `remote_workspace`. Keep `strict_host_key_checking = true`
so a changed host key aborts the connection. The key value is referenced only by
the *name* of the environment variable that holds its path; it never appears in
`config.toml`.

### Container-host backend

```toml
[execution]
backend = "container"

[execution.container]
host = "tcp://10.0.0.5:2376"          # omit for the local Docker socket
image = "cyrene/runner:latest"
remote_workspace = "/workspace"        # boundary enforced inside the container
credential_env = "CYRENE_DOCKER_TLS"  # env var for a TLS cert path, if needed
```

```bash
# .env
CYRENE_DOCKER_TLS=/home/you/.docker/certs
```

Each Step runs in a fresh `--rm` container scoped to `remote_workspace`. When the
host is remote, prefer a **TLS-secured** Docker endpoint and reference the client
cert directory by `credential_env`. As with SSH, no credential value lives in the
config file.

### Verifying a remote backend

Run `cyrene doctor` after configuring a backend. It validates that the selected
backend has its required settings (host/image and a non-empty `remote_workspace`
boundary) and that every referenced secret environment variable
(`key_env`, `credential_env`) is present. A remote backend that is missing its
section or its boundary fails validation rather than silently running unconfined.

### Combining with the unattended deployment

A common topology is a tiny always-on **control host** running the Cyrene
container within the idle budget, configured with a remote backend that points
at a larger **worker** (an SSH build box or a container host) for heavy Steps.
The control host stays cheap and idle; the worker only spins up work when a Step
clears the gate — and that gate is identical to the one a local deployment uses.
