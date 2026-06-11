# cyrene-agent

> Cyrene: the AI agent that always loves you — open-source, self-improving, written in Rust.

This npm package is a thin installer. On install it downloads the prebuilt
native `cyrene` binary for your platform from
[GitHub Releases](https://github.com/YourWisemaker/cyrene/releases), verifies its
checksum, and exposes it as the `cyrene` command. No Rust toolchain required.

> The published package is named `cyrene-agent` because the bare `cyrene` name is
> already taken on npm. The installed command is still **`cyrene`**.

## Install

```bash
npm install -g cyrene-agent      # npm
pnpm add -g cyrene-agent         # pnpm
yarn global add cyrene-agent     # yarn
```

Run once without installing:

```bash
npx cyrene-agent --help
```

## Supported platforms

| OS | Architectures |
|----|---------------|
| Linux | x86_64, aarch64 (glibc + musl), armv7 (Raspberry Pi 32-bit) |
| macOS | Intel (x86_64), Apple Silicon (arm64) |
| Windows | x64, ARM64 |

Raspberry Pi (32-bit and 64-bit Raspberry Pi OS) is detected automatically.

## Usage

```bash
cyrene onboard     # configure a model provider and channel
cyrene doctor      # verify your setup
cyrene gateway     # start Cyrene
cyrene --help      # all commands
```

## Environment variables

| Variable | Purpose |
|----------|---------|
| `CYRENE_VERSION` | Install a specific version instead of the package default |
| `CYRENE_FORCE_INSTALL` | Re-download the binary even if one is already present |

## Other install methods

- **Linux/macOS/Pi:** `curl -fsSL https://raw.githubusercontent.com/YourWisemaker/cyrene/master/install.sh | bash`
- **Windows (PowerShell):** `irm https://raw.githubusercontent.com/YourWisemaker/cyrene/master/install.ps1 | iex`
- **Homebrew:** `brew tap YourWisemaker/cyrene && brew install cyrene-agent`
- **From source:** see the [main README](https://github.com/YourWisemaker/cyrene#build-from-source)

## License

[Apache-2.0](https://github.com/YourWisemaker/cyrene/blob/master/LICENSE)
