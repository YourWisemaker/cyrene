#!/usr/bin/env bash
# Cyrene release helper script.
set -euo pipefail

VERSION="${1:?Usage: scripts/release.sh <version>}"

echo "Preparing release v${VERSION}..."

# Verify tests pass
cargo test --workspace

# Build release binary
cargo build --release --bin cyrene

echo "✓ Release v${VERSION} ready"
echo "Binary: target/release/cyrene"
