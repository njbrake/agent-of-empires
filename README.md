<p align="center">
  <img src="assets/tengu.png" alt="kokorro" width="128">
  <h1 align="center">kokorro</h1>
  <p align="center">
    <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-yellow.svg" alt="License: MIT"></a>
  </p>
</p>

A kokorro fork of [Agent of Empires](https://github.com/njbrake/agent-of-empires) -- a terminal session manager for AI coding agents on Linux and macOS. Built on tmux, written in Rust.

Run multiple AI agents in parallel across different branches of your codebase, each in its own isolated session with sandboxing.

## Features

- **Multi-agent support** -- Claude Code, OpenCode, Mistral Vibe, Codex CLI, Gemini CLI, Cursor CLI, Copilot CLI, Pi.dev
- **TUI dashboard** -- visual interface to create, monitor, and manage sessions
- **Agent + terminal views** -- toggle between your AI agents and paired shell terminals with `t`
- **Status detection** -- see which agents are running, waiting for input, or idle
- **Git worktrees** -- run parallel agents on different branches of the same repo
- **Docker sandboxing** -- isolate agents in containers with shared auth volumes
- **Safehouse integration** -- `command_wrapper` config for macOS native sandboxing via [agent-safehouse](https://github.com/anthropics/agent-safehouse)
- **Session capture** -- record and replay agent sessions
- **Diff view** -- review git changes and edit files without leaving the TUI
- **Per-repo config** -- `.aoe/config.toml` for project-specific settings and hooks
- **Profiles** -- separate workspaces for different projects or clients
- **CLI and TUI** -- full functionality from both interfaces

## Install

**Prerequisites:** [tmux](https://github.com/tmux/tmux/wiki) (required), [Docker](https://www.docker.com/) (optional, for sandboxing)

```bash
# Build from source
git clone https://github.com/kokorro-labs/snyper
cd snyper && cargo install --path .
```

This installs the `koko` binary to `~/.cargo/bin/`.

## Quick Start

```bash
# Launch the TUI
koko

# Add a session from CLI
koko add /path/to/project

# Add a session on a new git branch
koko add . -w feat/my-feature -b

# Add a sandboxed session
koko add --sandbox .
```

In the TUI: `n` to create a session, `Enter` to attach, `t` to toggle terminal view, `D` for diff view, `d` to delete, `?` for help.

## How It Works

kokorro wraps [tmux](https://github.com/tmux/tmux/wiki). Each session is a tmux session, so agents keep running when you close the TUI. Reopen `koko` and everything is still there.

The key tmux shortcut to know: **`Ctrl+b d`** detaches from a session and returns to the TUI.

## Roadmap

Things we're building into kokorro on top of upstream AoE:

- [ ] **Safehouse as first-class sandbox backend** -- select safehouse as a sandbox runtime in TUI settings, with dedicated config surface for `--enable`, `--append-profile`, `--add-dirs-ro` flags
- [ ] **Binary rename** -- full codebase sweep of `aoe` references to `koko` (CLI help text, hook commands, error messages, tmux session prefixes)
- [ ] **kokorro-specific profiles** -- default profile templates tailored for kokorro workflows
- [ ] **Upstream sync automation** -- streamlined merge workflow to keep up with AoE releases
- [x] **Command wrapper config** -- `command_wrapper` field in sandbox config to prefix agent launch commands (e.g. wrapping with safehouse)
- [x] **Merge upstream v0.16.1** -- session capture, OpenClaw/ClawHub, Copilot CLI, unified profiles TUI, Docker fixes

## FAQ

### What happens when I close koko?

Nothing. Sessions are tmux sessions running in the background. Open and close `koko` as often as you like. Sessions only get removed when you explicitly delete them.

### Which AI tools are supported?

Claude Code, OpenCode, Mistral Vibe, Codex CLI, Gemini CLI, Cursor CLI, Copilot CLI, and Pi.dev. kokorro auto-detects which are installed on your system.

### What's the relationship to Agent of Empires?

kokorro is a fork of [Agent of Empires](https://github.com/njbrake/agent-of-empires) by [Nate Brake](https://x.com/natebrake). We track upstream and merge in new features while adding kokorro-specific integrations like safehouse sandboxing.

## Development

```bash
cargo check            # Type-check
cargo test             # Run tests
cargo fmt              # Format
cargo clippy           # Lint
cargo build --release  # Release build

# Install locally
cargo install --path .

# Debug logging
AGENT_OF_EMPIRES_DEBUG=1 cargo run
```

## License

MIT License -- see [LICENSE](LICENSE) for details.

Based on [Agent of Empires](https://github.com/njbrake/agent-of-empires) by [Nate Brake](https://x.com/natebrake).
