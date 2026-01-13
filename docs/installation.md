# Installation

## Prerequisites

Agent of Empires requires [tmux](https://github.com/tmux/tmux/wiki) to be installed on your system.

**macOS:**
```bash
brew install tmux
```

**Ubuntu/Debian:**
```bash
sudo apt install tmux
```

**Fedora:**
```bash
sudo dnf install tmux
```

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
brew install njbrake/aoe/aoe
```

Update via `brew update && brew upgrade aoe`.

### Build from Source

```bash
git clone https://github.com/njbrake/agent-of-empires
cd agent-of-empires
cargo build --release
```

The binary will be at `target/release/aoe`.

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
