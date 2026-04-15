# Installation

## Prerequisites

- [tmux](https://github.com/tmux/tmux/wiki) (required)
- [Docker](https://www.docker.com/) (optional, for sandboxing agents in containers)
- [Node.js](https://nodejs.org/) (optional, only for building the web dashboard with `--features serve`)

## Install Agent of Empires

### Quick Install (Recommended)

Run the install script:

```bash
curl -fsSL \
  https://raw.githubusercontent.com/njbrake/agent-of-empires/main/scripts/install.sh \
  | bash
```

### Homebrew

```bash
brew install aoe
```

> **Note:** The Homebrew formula does not yet include the web dashboard (`aoe serve`) since the feature is still experimental. To use the web dashboard, install via the [quick install script](#quick-install-recommended) or [build from source](#build-from-source) with `--features serve`.

### Build from Source

```bash
git clone https://github.com/njbrake/agent-of-empires
cd agent-of-empires
cargo build --release
```

The binary will be at `target/release/aoe`.

To include the web dashboard (browser access):

```bash
cargo build --release --features serve
```

This requires Node.js and npm. The web frontend is built automatically during compilation.

## Verify Installation

```bash
aoe --version
```

## Uninstall

To remove Agent of Empires:

```bash
aoe uninstall
```

This will guide you through removing the binary, configuration, and tmux settings.
