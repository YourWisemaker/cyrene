# Installation

Cyrene ships as a single native binary. Pick the method that fits your platform —
all of them install the same `cyrene` executable. Prebuilt binaries are published
for Linux (x86_64, aarch64, armv7), macOS (Intel + Apple Silicon), Windows
(x64 + ARM64), and Raspberry Pi.

## Quick install

### Linux / macOS / Raspberry Pi

```bash
curl -fsSL https://raw.githubusercontent.com/YourWisemaker/cyrene/master/install.sh | bash
```

The script detects your OS and CPU (including 32-bit Raspberry Pi and musl
distros like Alpine), downloads the matching prebuilt binary, verifies its
checksum, and falls back to building from source if no prebuilt binary matches.

Override defaults with environment variables:

```bash
CYRENE_INSTALL_DIR="$HOME/.local/bin" \
CYRENE_VERSION=0.1.0 \
curl -fsSL https://raw.githubusercontent.com/YourWisemaker/cyrene/master/install.sh | bash
```

### Windows (PowerShell)

```powershell
irm https://raw.githubusercontent.com/YourWisemaker/cyrene/master/install.ps1 | iex
```

Installs `cyrene.exe` to `%LOCALAPPDATA%\Cyrene\bin` and adds it to your user
PATH. Works on Windows PowerShell 5.1+ and PowerShell 7+, x64 and ARM64.

## Package managers

### npm / pnpm / yarn

The npm package downloads the prebuilt binary for your platform on install.

```bash
npm install -g cyrene      # npm
pnpm add -g cyrene         # pnpm
yarn global add cyrene     # yarn
```

Or run without installing:

```bash
npx cyrene --help
```

### Homebrew (macOS / Linux)

```bash
brew tap YourWisemaker/cyrene
brew install cyrene
```

### Nix

```bash
nix build github:YourWisemaker/cyrene
```

### Docker

```bash
docker compose -f docker/docker-compose.yml up -d
```

## Raspberry Pi

Cyrene runs on Raspberry Pi out of the box:

- **64-bit Raspberry Pi OS** → `aarch64-unknown-linux-gnu` binary
- **32-bit Raspberry Pi OS** → `armv7-unknown-linux-gnueabihf` binary

Both are selected automatically by the `install.sh` script. A Pi 3 or newer is
recommended. For headless setups, the install runs non-interactively and you can
configure later with `cyrene onboard`.

## Build from source

Requires the Rust toolchain (1.82+).

```bash
git clone https://github.com/YourWisemaker/cyrene.git
cd cyrene
cargo build --release --bin cyrene
cp target/release/cyrene /usr/local/bin/
```

To force a source build through the installer:

```bash
CYRENE_FROM_SOURCE=1 curl -fsSL https://raw.githubusercontent.com/YourWisemaker/cyrene/master/install.sh | bash
```

## After installation

1. Run `cyrene onboard` to configure a model provider and channel
2. Add your API keys (the onboarding wizard creates `.env` for you)
3. Run `cyrene doctor` to verify your setup
4. Run `cyrene gateway` to start Cyrene

## Updating

```bash
cyrene update          # download and install the latest release
cyrene update --check  # just check whether an update is available
cyrene version         # show installed vs. latest version
```
