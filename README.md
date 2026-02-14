<p align="center">
  <img src="assets/logo.png" alt="Agent of Empires" width="128">
  <h1 align="center">Agent of Empires (AoE)</h1>
  <p align="center">
    <a href="https://njbrake.github.io/agent-of-empires/"><img src="https://img.shields.io/badge/docs-aoe-blue" alt="Documentation"></a>
    <a href="https://github.com/njbrake/agent-of-empires/actions/workflows/ci.yml"><img src="https://github.com/njbrake/agent-of-empires/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
    <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-yellow.svg" alt="License: MIT"></a>
    <a href="https://github.com/njbrake/agent-of-empires/releases"><img src="https://img.shields.io/github/v/release/njbrake/agent-of-empires" alt="GitHub release"></a>
    <a href="https://blog.rust-lang.org/2023/11/16/Rust-1.74.0.html"><img src="https://img.shields.io/badge/MSRV-1.74-blue?logo=rust" alt="MSRV"></a>
    <a href="https://github.com/njbrake/agent-of-empires/stargazers"><img src="https://img.shields.io/github/stars/njbrake/agent-of-empires?style=social" alt="GitHub stars"></a>
    <a href="https://agent-of-empires.com/docs/credits.html"><img src="https://img.shields.io/badge/dynamic/json?url=https%3A%2F%2Fraw.githubusercontent.com%2Fnjbrake%2Fagent-of-empires%2Fcredit%2Fcredits.json&query=%24.contributors.length&label=contributors&color=blue&logo=github" alt="Contributors"></a>
    <a href="https://www.youtube.com/@agent-of-empires"><img src="https://img.shields.io/badge/YouTube-channel-red?logo=youtube" alt="YouTube"></a>
  </p>
</p>

A terminal session manager for AI coding agents on Linux and macOS. Built on tmux, written in Rust.

Run multiple AI agents in parallel across different branches of your codebase, each in its own isolated session with optional Docker sandboxing.

> If you find this project useful, please consider giving it a star on GitHub: it helps others discover the project!

![Agent of Empires Demo](docs/assets/demo.gif)

## Features

- **Multi-agent support** -- Claude Code, OpenCode, Mistral Vibe, Codex CLI, and Gemini CLI
- **TUI dashboard** -- visual interface to create, monitor, and manage sessions
- **Agent + terminal views** -- toggle between your AI agents and paired shell terminals with `t`
- **Status detection** -- see which agents are running, waiting for input, or idle
- **Git worktrees** -- run parallel agents on different branches of the same repo
- **Docker sandboxing** -- isolate agents in containers with shared auth volumes
- **Diff view** -- review git changes and edit files without leaving the TUI
- **Per-repo config** -- `.aoe/config.toml` for project-specific settings and hooks
- **Profiles** -- separate workspaces for different projects or clients
- **CLI and TUI** -- full functionality from both interfaces

## How It Works

AoE wraps [tmux](https://github.com/tmux/tmux/wiki). Each session is a tmux session, so agents keep running when you close the TUI. Reopen `aoe` and everything is still there.

The key tmux shortcut to know: **`Ctrl+b d`** detaches from a session and returns to the TUI.

## Installation

**Prerequisites:** [tmux](https://github.com/tmux/tmux/wiki) (required), [Docker](https://www.docker.com/) (optional, for sandboxing)

```bash
# Quick install (Linux & macOS)
curl -fsSL \
  https://raw.githubusercontent.com/njbrake/agent-of-empires/main/scripts/install.sh \
  | bash

# Homebrew
brew install njbrake/aoe/aoe

# Build from source
git clone https://github.com/njbrake/agent-of-empires
cd agent-of-empires && cargo build --release
```

## Quick Start

```bash
# Launch the TUI
aoe

# Add a session from CLI
aoe add /path/to/project

# Add a session on a new git branch
aoe add . -w feat/my-feature -b

# Add a sandboxed session
aoe add --sandbox .
```

In the TUI: `n` to create a session, `Enter` to attach, `t` to toggle terminal view, `D` for diff view, `d` to delete, `?` for help.

## Documentation

- **[Installation](https://njbrake.github.io/agent-of-empires/installation)** -- prerequisites and install methods
- **[Quick Start](https://njbrake.github.io/agent-of-empires/quick-start)** -- first steps and basic usage
- **[Workflow Guide](https://njbrake.github.io/agent-of-empires/guides/workflow)** -- recommended setup with bare repos and worktrees
- **[Docker Sandbox](https://njbrake.github.io/agent-of-empires/guides/sandbox)** -- container isolation for agents
- **[Repo Config & Hooks](https://njbrake.github.io/agent-of-empires/guides/repo-config)** -- per-project settings and automation
- **[Configuration Reference](https://njbrake.github.io/agent-of-empires/guides/configuration)** -- all config options
- **[CLI Reference](https://njbrake.github.io/agent-of-empires/cli/reference)** -- complete command documentation

## FAQ

### What happens when I close aoe?

Nothing. Sessions are tmux sessions running in the background. Open and close `aoe` as often as you like. Sessions only get removed when you explicitly delete them.

### Which AI tools are supported?

Claude Code, OpenCode, Mistral Vibe, Codex CLI, and Gemini CLI. AoE auto-detects which are installed on your system.

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

# Debug logging
AGENT_OF_EMPIRES_DEBUG=1 cargo run
```

## Star History

[![Star History Chart](https://api.star-history.com/svg?repos=njbrake/agent-of-empires&type=date&legend=top-left)](https://www.star-history.com/#njbrake/agent-of-empires&type=date&legend=top-left)

## Acknowledgments

Inspired by [agent-deck](https://github.com/asheshgoplani/agent-deck) (Go + Bubble Tea).

## License

MIT License -- see [LICENSE](LICENSE) for details.
