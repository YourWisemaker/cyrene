# Cyrene Documentation

Guides for installing, configuring, operating, and understanding Cyrene — the AI
agent that always loves you. New here? Start with the
[main README](../README.md) for the overview and a 60-second quick start, then
come back for depth.

## Guides

| Guide | Read it when you want to… |
|-------|---------------------------|
| [Installation](installation.md) | Install Cyrene on Linux, macOS, Windows, or a Raspberry Pi — via npm, the install script, Nix, Docker, or from source |
| [Configuration](configuration.md) | Understand `~/.cyrene/config.toml`: model providers, channels, the persona, autonomy levels, and how secrets stay out of config |
| [Architecture](architecture.md) | See how a request flows through the safety pipeline and how the workspace crates fit together |
| [Security Model](security.md) | Learn the defense-in-depth layers, the two memory trust boundaries, and operator best practices |
| [Deployment](deployment.md) | Run Cyrene unattended on a low-resource host, or dispatch work to a remote SSH/container backend without loosening safety |

## Quick links

- **Get started:** `cyrene onboard` → `cyrene doctor` → `cyrene`
- **Stay current:** `cyrene update` (or `cyrene update --check`)
- **Always-on service:** `cyrene service install`
- **All commands:** `cyrene --help`, or the live `/` menu inside a chat
