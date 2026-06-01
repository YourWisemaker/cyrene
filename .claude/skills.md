# Cyrene Skills for Claude

## Project Overview
Cyrene is a Rust workspace implementing an autonomous AI agent with trait-based modularity.

## Key Commands
- `cargo build --workspace` — Build all crates
- `cargo test --workspace` — Run all tests
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — Lint
- `cargo fmt --all -- --check` — Format check

## Architecture Rules
- Core types go in `cyrene-core`; adapters implement its traits
- Safety pipeline is mandatory: injection → shadow → approval → execute
- No secrets in source code; use env vars only
- Every tool invocation must produce a receipt

## File Structure
- `crates/` — Rust workspace members
- `skills/` — Bundled SKILL.md files (200+)
- `extensions/` — Built-in extension manifests
- `firmware/` — MCU companion firmware
