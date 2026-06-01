# Cyrene Agent Guide

Cyrene is an open-source, self-improving autonomous AI agent written in Rust.

## Architecture

- **Cargo workspace** with each subsystem in its own crate under `crates/`
- **Trait-based modularity**: `Channel`, `Memory`, `Model` are swappable traits
- **Safety pipeline**: injection scan → plan → shadow execution → approval gate → execute → receipt → checkpoint

## Build & Test

```bash
cargo build --workspace
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

## Key Crates

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
| `cyrene-cli` | CLI binary, onboarding, doctor |

## Security Conventions

- **No secrets in code**: API keys/tokens are loaded from env/`.env` only
- **Autonomy defaults**: low=auto, medium=approval, high=blocked
- **Every action is logged**: signed, hash-chained receipts in the ledger
- **Workspace boundary**: all execution confined to workspace directory

## Contribution Guidelines

1. Run `cargo fmt` and `cargo clippy` before committing
2. Write tests for new functionality (unit + property-based where applicable)
3. Follow existing code style and patterns
4. Never commit `.env`, secrets, or API keys
5. Each task corresponds to a crate or module; commit after completing each task
