# Agent of Empires (aoe)

[![Documentation](https://img.shields.io/badge/docs-aoe-blue)](https://njbrake.github.io/agent-of-empires/)
[![CI](https://github.com/njbrake/agent-of-empires/actions/workflows/ci.yml/badge.svg)](https://github.com/njbrake/agent-of-empires/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![GitHub release](https://img.shields.io/github/v/release/njbrake/agent-of-empires)](https://github.com/njbrake/agent-of-empires/releases)
[![MSRV](https://img.shields.io/badge/MSRV-1.74-blue?logo=rust)](https://blog.rust-lang.org/2023/11/16/Rust-1.74.0.html)

A terminal session manager for Linux and macOS using tmux to aid in management and monitoring of AI coding agents, written in Rust.

- Git worktree management for easily handling parallel agents in one codebase
- Easily sandbox your agents in docker containers
- Interact via either TUI or CLI

`aoe` manages cleanup of worktrees and sandboxes once you've completed your session. 

![Agent of Empires Demo](assets/demo.gif)

## Prerequisites

- [tmux](https://github.com/tmux/tmux/wiki) (required)
- [Docker](https://www.docker.com/) (optional, for sandboxing agents in containers)

## Installation

**Quick install (Linux & macOS):**

```bash
curl -fsSL \
  https://raw.githubusercontent.com/njbrake/agent-of-empires/main/scripts/install.sh \
  | bash
```

**Homebrew:**

```bash
brew install njbrake/aoe/aoe
```

Update via `brew update && brew upgrade aoe`.

**Build from source:**

```bash
git clone https://github.com/njbrake/agent-of-empires
cd agent-of-empires
cargo build --release
```

## How It Works

Agent of Empires (aoe) is a wrapper around [tmux](https://github.com/tmux/tmux/wiki), the terminal multiplexer. Each AI coding session you create is actually a tmux session under the hood.

Once you attach to a session, you're working directly in tmux. Basic tmux knowledge helps:

| tmux Command | What It Does |
|--------------|--------------|
| `Ctrl+b d` | Detach from session (return to Agent of Empires) |
| `Ctrl+b [` | Enter scroll/copy mode |
| `Ctrl+b n` / `Ctrl+b p` | Next/previous window |

If you're new to tmux, the key thing to remember is `Ctrl+b d` to detach and return to the TUI, and that with Claude Code you'll need to enter scroll mode in order to scroll up in the Claude Code window (this isn't necessary when using opencode).

## Features

- **TUI Dashboard** - Visual interface to manage all your AI coding sessions
- **Session Management** - Create, attach, detach, and delete sessions
- **Group Organization** - Organize sessions into hierarchical folders
- **Status Detection** - Automatic status detection for Claude Code and OpenCode
- **tmux Integration** - Sessions persist in tmux for reliability
- **Multi-profile Support** - Separate workspaces for different projects

## Quick Start

```bash
# Launch the TUI
aoe

# Or add a session directly from CLI
aoe add /path/to/project
```

## Configuration

### Profiles

Profiles let you maintain separate workspaces with their own sessions and groups. This is useful when you want to keep different contexts isolated—for example, work projects vs personal projects, or different client engagements.

```bash
aoe                 # Uses "default" profile
aoe -p work         # Uses "work" profile
aoe -p client-xyz   # Uses "client-xyz" profile
```

Each profile stores its own `sessions.json` and `groups.json`, so switching profiles gives you a completely different set of sessions.

### File Locations

Configuration is stored in `~/.agent-of-empires/`:

```
~/.agent-of-empires/
├── config.toml           # Global configuration
├── profiles/
│   └── default/
│       ├── sessions.json # Session data
│       └── groups.json   # Group structure
└── logs/                 # Session logs
```

### Environment Variables

| Variable | Description |
|----------|-------------|
| `AGENT_OF_EMPIRES_PROFILE` | Default profile to use |
| `AGENT_OF_EMPIRES_DEBUG` | Enable debug logging |

## Development

```bash
# Check code
cargo check

# Run tests
cargo test

# Format code
cargo fmt

# Lint
cargo clippy

# Run in debug mode
AGENT_OF_EMPIRES_DEBUG=1 cargo run

# Build release binary
cargo build --release
```

## FAQ

### What happens when I close aoe?

Nothing! Your sessions keep running. Since aoe is just a frontend for tmux, all your agent sessions are actually tmux sessions running independently in the background. You can freely open and close aoe as often as you like—your sessions will still be there when you come back.

Sessions are never deleted automatically. They only get removed when you explicitly delete them (either through aoe's interface or with tmux commands like `tmux kill-session`).

## Troubleshooting

### Using aoe with mobile SSH clients (Termius, Blink, etc.)

If you're connecting via SSH from a mobile app like Termius, you may encounter issues when attaching to sessions. The recommended approach is to run `aoe` inside a tmux session:

```bash
# Start a tmux session first
tmux new-session -s main

# Then run aoe inside it
aoe
```

When you attach to an agent session, tmux will switch to that session. To navigate back to `aoe` use the tmux command `Ctrl+b L` to switch to last session (toggle back to aoe)

### Claude Code is flickering

This is not an issue with `aoe`: it's a known problem with Claude Code: https://github.com/anthropics/claude-code/issues/1913 

## Acknowledgments

Inspired by [agent-deck](https://github.com/asheshgoplani/agent-deck) (Go + Bubble Tea).

## License

MIT License - see [LICENSE](LICENSE) for details.
