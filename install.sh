#!/usr/bin/env bash
# Cyrene installer — the AI agent that always loves you.
#
# Downloads the prebuilt single binary for your platform from GitHub Releases
# (Linux, macOS, Raspberry Pi) and installs it. Falls back to building from
# source when no prebuilt binary matches. Re-runnable and used for self-update.
#
#   curl -fsSL https://raw.githubusercontent.com/YourWisemaker/cyrene/master/install.sh | bash
#
# Environment overrides:
#   CYRENE_INSTALL_DIR   install location (default: /usr/local/bin or ~/.local/bin)
#   CYRENE_VERSION       version/tag to install (default: latest)
#   CYRENE_FROM_SOURCE   set to 1 to force a source build
set -euo pipefail

REPO="YourWisemaker/cyrene"
BINARY="cyrene"

# Pick a sensible install dir: prefer /usr/local/bin when writable, else a
# per-user bin that does not need sudo (handy on locked-down Pis and CI).
default_install_dir() {
    if [ -n "${CYRENE_INSTALL_DIR:-}" ]; then
        echo "$CYRENE_INSTALL_DIR"
    elif [ -w "/usr/local/bin" ] || [ "$(id -u)" = "0" ]; then
        echo "/usr/local/bin"
    else
        echo "$HOME/.local/bin"
    fi
}
INSTALL_DIR="$(default_install_dir)"

echo "╔═══════════════════════════════════════════════════╗"
echo "║  Cyrene Installer                                  ║"
echo "║  The AI agent that always loves you                ║"
echo "╚═══════════════════════════════════════════════════╝"
echo ""

# Map uname output to a Rust target triple. Raspberry Pi reports armv6l/armv7l
# (32-bit Raspberry Pi OS) or aarch64 (64-bit OS); both are supported.
detect_target() {
    local os arch libc
    os="$(uname -s)"
    arch="$(uname -m)"
    libc="gnu"

    case "$os" in
        Linux)
            # musl-based distros (Alpine) need the static musl build.
            if ldd --version 2>&1 | grep -qi musl; then
                libc="musl"
            fi
            case "$arch" in
                x86_64|amd64)   TARGET="x86_64-unknown-linux-${libc}" ;;
                aarch64|arm64)  TARGET="aarch64-unknown-linux-${libc}" ;;
                armv7l|armv6l|armhf)
                    # Raspberry Pi 32-bit. Only a gnueabihf build is published.
                    TARGET="armv7-unknown-linux-gnueabihf"
                    IS_PI=1 ;;
                *) echo "Unsupported Linux architecture: $arch"; TARGET="" ;;
            esac
            ;;
        Darwin)
            case "$arch" in
                x86_64) TARGET="x86_64-apple-darwin" ;;
                arm64)  TARGET="aarch64-apple-darwin" ;;
                *) echo "Unsupported macOS architecture: $arch"; TARGET="" ;;
            esac
            ;;
        MINGW*|MSYS*|CYGWIN*)
            echo "On Windows, please use the PowerShell installer:"
            echo "  irm https://raw.githubusercontent.com/${REPO}/master/install.ps1 | iex"
            exit 1
            ;;
        *) echo "Unsupported OS: $os"; TARGET="" ;;
    esac
}

check_deps() {
    local missing=()
    for cmd in curl tar; do
        command -v "$cmd" &>/dev/null || missing+=("$cmd")
    done
    if [ ${#missing[@]} -gt 0 ]; then
        echo "Missing required dependencies: ${missing[*]}"
        echo "Please install them and re-run this script."
        exit 1
    fi
}

# Resolve the version to install: an explicit CYRENE_VERSION, or the latest tag.
resolve_version() {
    if [ -n "${CYRENE_VERSION:-}" ]; then
        echo "${CYRENE_VERSION#v}"
        return
    fi
    curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null \
        | grep -m1 '"tag_name"' \
        | sed -E 's/.*"tag_name" *: *"v?([^"]+)".*/\1/'
}

install_from_release() {
    local version="$1"
    local url="https://github.com/${REPO}/releases/download/v${version}/cyrene-${TARGET}.tar.gz"
    local tmpdir
    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' RETURN

    echo "Downloading cyrene v${version} (${TARGET})..."
    if ! curl -fsSL "$url" -o "$tmpdir/cyrene.tar.gz"; then
        return 1
    fi

    # Verify the checksum when the sidecar file is available (best-effort).
    if curl -fsSL "${url}.sha256" -o "$tmpdir/cyrene.tar.gz.sha256" 2>/dev/null; then
        echo "Verifying checksum..."
        local expected actual
        expected="$(awk '{print $1}' "$tmpdir/cyrene.tar.gz.sha256")"
        if command -v sha256sum &>/dev/null; then
            actual="$(sha256sum "$tmpdir/cyrene.tar.gz" | awk '{print $1}')"
        else
            actual="$(shasum -a 256 "$tmpdir/cyrene.tar.gz" | awk '{print $1}')"
        fi
        if [ -n "$expected" ] && [ "$expected" != "$actual" ]; then
            echo "✗ Checksum mismatch — aborting (expected $expected, got $actual)."
            exit 1
        fi
    fi

    tar -xzf "$tmpdir/cyrene.tar.gz" -C "$tmpdir"
    mkdir -p "$INSTALL_DIR"
    cp "$tmpdir/cyrene-${TARGET}/$BINARY" "$INSTALL_DIR/$BINARY"
    chmod +x "$INSTALL_DIR/$BINARY"
    echo "✓ Cyrene installed to $INSTALL_DIR/$BINARY"
}

install_from_source() {
    echo "Building Cyrene from source (this can take a few minutes)..."
    for cmd in git; do
        command -v "$cmd" &>/dev/null || { echo "Missing dependency: $cmd"; exit 1; }
    done
    if ! command -v cargo &>/dev/null; then
        echo "Rust toolchain not found. Installing via rustup..."
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        # shellcheck disable=SC1091
        source "$HOME/.cargo/env"
    fi
    local tmpdir
    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' RETURN
    git clone --depth 1 "https://github.com/${REPO}.git" "$tmpdir/cyrene"
    ( cd "$tmpdir/cyrene" && cargo build --release --bin cyrene )
    mkdir -p "$INSTALL_DIR"
    cp "$tmpdir/cyrene/target/release/$BINARY" "$INSTALL_DIR/$BINARY"
    chmod +x "$INSTALL_DIR/$BINARY"
    echo "✓ Cyrene built and installed to $INSTALL_DIR/$BINARY"
}

# Warn (don't fail) if the chosen install dir isn't on PATH.
check_path() {
    case ":$PATH:" in
        *":$INSTALL_DIR:"*) ;;
        *)
            echo ""
            echo "⚠  $INSTALL_DIR is not on your PATH. Add this to your shell profile:"
            echo "     export PATH=\"$INSTALL_DIR:\$PATH\""
            ;;
    esac
}

main() {
    check_deps
    IS_PI=0
    detect_target
    [ "${IS_PI:-0}" = "1" ] && echo "Raspberry Pi (32-bit) detected."

    local version installed=0
    version="$(resolve_version || true)"

    if [ "${CYRENE_FROM_SOURCE:-0}" = "1" ] || [ -z "$TARGET" ]; then
        install_from_source && installed=1
    else
        if [ -z "$version" ]; then
            echo "Could not determine the latest version; building from source instead."
            install_from_source && installed=1
        elif install_from_release "$version"; then
            installed=1
        else
            echo "No prebuilt binary for ${TARGET} v${version}; building from source instead."
            install_from_source && installed=1
        fi
    fi

    [ "$installed" = "1" ] || { echo "✗ Installation failed."; exit 1; }

    check_path
    echo ""
    echo "Setting up Cyrene..."
    mkdir -p "$HOME/.cyrene"
    if [ ! -f "$HOME/.cyrene/config.toml" ]; then
        if [ -t 0 ]; then
            echo "Running onboarding wizard..."
            "$INSTALL_DIR/$BINARY" onboard || true
        else
            echo "Run 'cyrene onboard' to configure your model provider and channel."
        fi
    else
        echo "Config already exists at $HOME/.cyrene/config.toml"
        echo "Run 'cyrene onboard' to reconfigure, or 'cyrene doctor' to check health."
    fi

    echo ""
    echo "✓ Installation complete! Run 'cyrene --help' to get started."
}

main "$@"
