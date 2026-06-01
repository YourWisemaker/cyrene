#!/usr/bin/env bash
# Cyrene install helper script — called by install.sh or CI.
set -euo pipefail

echo "Building Cyrene from source..."
cargo build --release --bin cyrene
echo "✓ Build complete"
