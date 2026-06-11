#!/usr/bin/env bash
set -euo pipefail

REPO="YourWisemaker/cyrene"
BINARY="cyrene"
INSTALL_DIR="${CYRENE_INSTALL_DIR:-/usr/local/bin}"

echo "╔═══════════════════════════════════════════════════╗"
echo "║  Cyrene Installer                                  ║"
echo "║  The AI agent that always loves you                 ║"
echo "╚═══════════════════════════════════════════════════╝"
echo ""

detect_platform() {
    local os arch
    os="$(uname -s)"
    arch="$(uname -m)"
    case "$os" in
        Linux)  PLATFORM="unknown-linux-gnu" ;;
        Darwin) PLATFORM="apple-darwin" ;;
        MINGW*|MSYS*|CYGWIN*) PLATFORM="pc-windows-msvc" ;;
        *) echo "Unsupported OS: $os"; exit 1 ;;
    esac
    case "$arch" in
        x86_64|amd64) ARCH="x86_64" ;;
        aarch64|arm64) ARCH="aarch64" ;;
        *) echo "Unsupported architecture: $arch"; exit 1 ;;
    esac
    TARGET="${ARCH}-${PLATFORM}"
}

install_from_source() {
    echo "Building Cyrene from source..."
    if ! command -v cargo &>/dev/null; then
        echo "Rust toolchain not found. Installing via rustup..."
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        source "$HOME/.cargo/env"
    fi
    echo "Cloning repository..."
    local tmpdir
    tmpdir="$(mktemp -d)"
    git clone --depth 1 "https://github.com/${REPO}.git" "$tmpdir/cyrene"
    cd "$tmpdir/cyrene"
    cargo build --release --bin cyrene
    mkdir -p "$INSTALL_DIR"
    cp "target/release/$BINARY" "$INSTALL_DIR/$BINARY"
    chmod +x "$INSTALL_DIR/$BINARY"
    rm -rf "$tmpdir"
    echo "✓ Cyrene installed to $INSTALL_DIR/$BINARY"
}

check_deps() {
    local missing=()
    for cmd in git curl; do
        if ! command -v "$cmd" &>/dev/null; then
            missing+=("$cmd")
        fi
    done
    if [ ${#missing[@]} -gt 0 ]; then
        echo "Missing required dependencies: ${missing[*]}"
        echo "Please install them and re-run this script."
        exit 1
    fi
}

main() {
    check_deps
    detect_platform
    echo "Detected platform: $TARGET"
    echo ""

    if command -v "$BINARY" &>/dev/null; then
        echo "Cyrene is already installed: $(command -v "$BINARY")"
        echo "  Current version: $($BINARY --version 2>/dev/null || echo 'unknown')"
        read -rp "Reinstall? [y/N] " answer
        if [[ ! "$answer" =~ ^[Yy] ]]; then
            echo "Skipping installation."
        else
            install_from_source
        fi
    else
        install_from_source
    fi

    echo ""
    echo "Setting up Cyrene..."
    mkdir -p "$HOME/.cyrene"

    if [ ! -f "$HOME/.cyrene/config.toml" ]; then
        echo "Running onboarding wizard..."
        "$INSTALL_DIR/$BINARY" onboard
    else
        echo "Config already exists at $HOME/.cyrene/config.toml"
        echo "Run 'cyrene onboard' to reconfigure, or 'cyrene doctor' to check health."
    fi

    echo ""
    echo "✓ Installation complete!"
    echo "  Run 'cyrene --help' to get started."
}

main "$@"
