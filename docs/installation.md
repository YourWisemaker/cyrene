# Installation

## One-Line Install (Linux/macOS)

```bash
curl -sSf https://raw.githubusercontent.com/cyrene-agent/cyrene/main/install.sh | bash
```

## Build from Source

```bash
git clone https://github.com/cyrene-agent/cyrene.git
cd cyrene
cargo build --release --bin cyrene
cp target/release/cyrene /usr/local/bin/
```

## Docker

```bash
docker compose -f docker/docker-compose.yml up -d
```

## Nix

```bash
nix build github:cyrene-agent/cyrene
```

## After Installation

1. Run `cyrene onboard` to configure a model provider and channel
2. Copy `.env.example` to `.env` and add your API keys
3. Run `cyrene doctor` to verify your setup
4. Run `cyrene gateway` to start Cyrene
