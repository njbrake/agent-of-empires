<p align="center">
  <img src="assets/logo.png" alt="Agent of Empires" width="128">
  <h1 align="center">Agent of Empires (AoE)</h1>
  <p align="center">
    <a href="https://github.com/njbrake/agent-of-empires/actions/workflows/ci.yml"><img src="https://github.com/njbrake/agent-of-empires/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
    <a href="https://github.com/njbrake/agent-of-empires/releases"><img src="https://img.shields.io/github/v/release/njbrake/agent-of-empires" alt="GitHub release"></a>
    <a href="https://formulae.brew.sh/formula/aoe"><img src="https://img.shields.io/homebrew/v/aoe" alt="Homebrew"></a>
    <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-yellow.svg" alt="License: MIT"></a>
    <a href="https://clawhub.ai/njbrake/aoe"><img src="https://img.shields.io/badge/ClawHub-aoe-blue" alt="ClawHub"></a>
    <br>
    <a href="https://www.youtube.com/@agent-of-empires"><img src="https://img.shields.io/badge/YouTube-channel-red?logo=youtube" alt="YouTube"></a>
    <a href="https://x.com/natebrake"><img src="https://img.shields.io/badge/follow-%40natebrake-black?logo=x&logoColor=white" alt="Follow @natebrake"></a>
  </p>
</p>

A session manager for AI coding agents on Linux and macOS. Use it from the terminal (TUI) or from any browser ([web dashboard](docs/guides/web-dashboard.md), experimental).

Run multiple AI agents in parallel across different branches of your codebase, each in its own isolated session with optional Docker sandboxing. Access your agents from your laptop, phone, or tablet.

## Why AoE?

Running one AI agent is easy. Running five of them across different branches, keeping track of which is stuck, which is waiting on input, and which just made a mess of your working tree, becomes a part-time job. AoE makes it a glance: one dashboard, one status column, git worktrees and Docker sandboxes set up for you, and sessions that outlive your terminal.

> If you find this project useful, please consider giving it a star on GitHub: it helps others discover the project!

<p align="center">
  <img src="docs/assets/demo.gif" alt="Agent of Empires Demo" width="800">
  <br>
  <a href="https://www.youtube.com/watch?v=Kk8dX_F-P4E">Watch the getting started video</a>
</p>

## Features

- **Multi-agent support**: Claude Code, OpenCode, Mistral Vibe, Codex CLI, Gemini CLI, Cursor CLI, Copilot CLI, Pi.dev, and Factory Droid
- **TUI app**: visual interface to create, monitor, and manage sessions
- **Web app**: create, monitor, and control your agents from any browser, installable as a PWA ([experimental](docs/guides/web-dashboard.md))
- **CLI app**: create, monitor, and control agents from the command line (integrates with tools like OpenClaw)
- **Remote access from your phone**: press `R` in the TUI to expose the web dashboard over a Cloudflare tunnel with QR + passphrase auth ([guide](docs/guides/remote-phone-access.md))
- **Status detection**: see which agents are running, waiting for input, or idle
- **Git worktrees**: run parallel agents on different branches of the same repo
- **Docker sandboxing**: isolate agents in containers with shared auth volumes
- **Diff view**: review git changes and edit files without leaving the TUI
- **Profiles**: separate workspaces for different projects or clients

## Web Dashboard (Beta)

Access your agents from any browser. The real agent terminal renders in the page; switch sessions, type into the terminal, and review diffs without leaving the tab. Press `R` in the TUI to start the server, or see the [web dashboard guide](docs/guides/web-dashboard.md) for details.

<p align="center">
  <img src="docs/assets/web-desktop.gif" alt="Web dashboard" width="800">
</p>

## How It Works

Each agent runs in its own [tmux](https://github.com/tmux/tmux/wiki) session, so your agents keep running when you close the TUI, disconnect SSH, or your terminal crashes. Reopen `aoe` and everything is exactly where you left it.

The key tmux shortcut to know: **`Ctrl+b d`** detaches from a session and returns to the TUI.

## Installation

**Prerequisites:** [tmux](https://github.com/tmux/tmux/wiki) (required), [Docker](https://www.docker.com/) (optional, for sandboxing)

```bash
# Quick install (Linux & macOS)
curl -fsSL \
  https://raw.githubusercontent.com/njbrake/agent-of-empires/main/scripts/install.sh \
  | bash

# Homebrew (TUI only, web dashboard not yet included)
brew install aoe

# Nix
nix run github:njbrake/agent-of-empires

# Build from source
git clone https://github.com/njbrake/agent-of-empires
cd agent-of-empires && cargo build --release
```

## Quick Start

```bash
aoe                          # Launch the TUI
aoe add --cmd claude         # Create a session running Claude Code
aoe serve                    # Start the web dashboard
```

In the TUI, press `?` for help. The bottom information bar shows all available keybindings in context.

## Documentation

- **[Installation](https://www.agent-of-empires.com/docs/installation/)**: prerequisites and install methods
- **[Quick Start](https://www.agent-of-empires.com/docs/quick-start/)**: first steps and basic usage
- **[Remote Phone Access](https://www.agent-of-empires.com/guides/remote-phone-access/)**: check on your agents from your phone via a Cloudflare tunnel
- **[Git Worktrees](https://www.agent-of-empires.com/guides/worktrees/)**: parallel agents on different branches
- **[Docker Sandbox](https://www.agent-of-empires.com/guides/sandbox/)**: container isolation for agents
- **[Repo Config & Hooks](https://www.agent-of-empires.com/guides/repo-config/)**: per-project settings and automation
- **[Diff View](https://www.agent-of-empires.com/guides/diff-view/)**: review and edit changes in the TUI
- **[tmux Status Bar](https://www.agent-of-empires.com/guides/tmux-status-bar/)**: integrated session monitoring
- **[Sound Effects](https://www.agent-of-empires.com/docs/sounds/)**: audible agent status notifications
- **[Configuration Reference](https://www.agent-of-empires.com/docs/guides/configuration/)**: all config options
- **[CLI Reference](https://www.agent-of-empires.com/docs/cli/reference/)**: complete command documentation
- **[Development](https://www.agent-of-empires.com/docs/development/)**: contributing and local setup

## FAQ

### What happens when I close aoe?

Nothing. Sessions are tmux sessions running in the background. Open and close `aoe` as often as you like. Sessions only get removed when you explicitly delete them.

### Which AI tools are supported?

Claude Code, OpenCode, Mistral Vibe, Codex CLI, Gemini CLI, Cursor CLI, Copilot CLI, Pi.dev, and Factory Droid. AoE auto-detects which are installed on your system.

### Can I use AoE over SSH?

Yes. AoE runs in your terminal and sessions persist across disconnects. If your mobile SSH client drops the connection, reconnect and `aoe` finds every session still running. See [mobile SSH clients](#using-aoe-with-mobile-ssh-clients-termius-blink-etc) for the one extra step needed on mobile.

### Does it work on Windows?

Only through WSL2. AoE depends on tmux and POSIX process handling, so native Windows is not supported.

### How is this different from just using tmux directly?

tmux gives you persistent sessions. AoE adds agent-aware status detection (running, waiting, idle, error), git worktree management, Docker sandboxing, a web dashboard, remote phone access, and a diff viewer, all wrapped around your existing tmux workflow. You can still `tmux attach` to any AoE session directly.

## Troubleshooting

### Using aoe with mobile SSH clients (Termius, Blink, etc.)

Run `aoe` inside a tmux session when connecting from mobile:

```bash
tmux new-session -s main
aoe
```

Use `Ctrl+b L` to toggle back to `aoe` after attaching to an agent session.

### Claude Code is flickering

This is a known Claude Code issue, not an aoe problem: https://github.com/anthropics/claude-code/issues/1913

## Development

```bash
cargo check          # Type-check
cargo test           # Run tests
cargo fmt            # Format
cargo clippy         # Lint
cargo build --release  # Release build

# Debug logging (writes to debug.log in app data dir)
AGENT_OF_EMPIRES_DEBUG=1 cargo run
```

## Star History

[![Star History Chart](https://api.star-history.com/svg?repos=njbrake/agent-of-empires&type=date&legend=top-left)](https://www.star-history.com/#njbrake/agent-of-empires&type=date&legend=top-left)

## Acknowledgments

Inspired by [agent-deck](https://github.com/asheshgoplani/agent-deck) (Go + Bubble Tea).

## Author

Created by [Nate Brake](https://x.com/natebrake) ([@natebrake](https://x.com/natebrake)), Machine Learning Engineer at [Mozilla.ai](https://www.mozilla.ai/).

## License

MIT License -- see [LICENSE](LICENSE) for details.
