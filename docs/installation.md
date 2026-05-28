# Installation

## Prerequisites

- [tmux](https://github.com/tmux/tmux/wiki) (required)
- [Docker](https://www.docker.com/) (optional, for sandboxing agents in containers)
- [Node.js](https://nodejs.org/) (optional, only needed when building the web dashboard from source with `--features serve`)

### Linux glibc requirement

The published Linux binaries (`aoe-linux-amd64`, `aoe-linux-arm64`) are built inside the `manylinux_2_28` container (AlmaLinux 8 base, **glibc 2.28**) and need that version or newer at runtime. Supported distros include Ubuntu 20.04+, Debian 11+, RHEL/Rocky/Alma 8+, Amazon Linux 2/2023, and Fedora 30+. Anything older (CentOS 7, Ubuntu 18.04, Debian 10) should install via Homebrew (Linuxbrew) or [build from source](#build-from-source).

The install script checks your glibc version before downloading and refuses to install if it's too old, so you won't end up with a broken binary on PATH. The release workflow also runs `objdump` against the built artifact and fails CI if the binary would require a newer glibc than this floor, so the bound here, `scripts/install.sh`, and the runtime symbol requirement stay in sync.

## Install Agent of Empires

### Quick Install (Recommended)

Run the install script:

```bash
curl -fsSL \
  https://raw.githubusercontent.com/agent-of-empires/agent-of-empires/main/scripts/install.sh \
  | bash
```

### Homebrew

```bash
brew install aoe
```

### Build from Source

```bash
git clone https://github.com/agent-of-empires/agent-of-empires
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

## Updating

```bash
aoe update
```

The `aoe update` command detects how aoe was installed (Homebrew, the curl install script, Nix, or Cargo) and dispatches to the right upgrade mechanism. For Nix and Cargo it prints the manual upgrade command instead of attempting an automatic update, since those cases need external tooling.

Inside the TUI, press `u` when the update bar is visible to run the same flow without leaving the app. Press `Ctrl+x` to dismiss the bar for the current session.

## Uninstall

To remove Agent of Empires:

```bash
aoe uninstall
```

This will guide you through removing the binary, configuration, and tmux settings.
