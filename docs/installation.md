# Installation

## Prerequisites

- [tmux](https://github.com/tmux/tmux/wiki) (required)
- [Docker](https://www.docker.com/) (optional, for sandboxing agents in containers)
- [Node.js](https://nodejs.org/) (optional, only needed when building the web dashboard from source with `--features serve`)

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
