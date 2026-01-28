# Agent of Empires

A terminal session manager for Linux and macOS using tmux to aid in management and monitoring of AI coding agents, written in Rust.

![Agent of Empires Demo](assets/demo.gif)

## Features

- **TUI Dashboard**: Visual interface to manage all your AI coding sessions
- **Session Management**: Create, attach, detach, and delete sessions
- **Group Organization**: Organize sessions into hierarchical folders
- **Status Detection**: Automatic status detection for Claude Code, OpenCode, Mistral Vibe, Codex CLI, and Gemini CLI
- **tmux Integration**: Sessions persist in tmux for reliability
- **Multi-profile Support**: Separate workspaces for different projects
- **Git Worktrees**: Run parallel agents on different branches of the same repo

## How It Works

Agent of Empires (aoe) is a wrapper around [tmux](https://github.com/tmux/tmux/wiki), the terminal multiplexer. Each AI coding session you create is actually a tmux session under the hood.

Once you attach to a session, you're working directly in tmux. Basic tmux knowledge helps:

| tmux Command | What It Does |
|--------------|--------------|
| `Ctrl+b d` | Detach from session (return to Agent of Empires) |
| `Ctrl+b [` | Enter scroll/copy mode |
| `Ctrl+b n` / `Ctrl+b p` | Next/previous window |

If you're new to tmux, the key thing to remember is `Ctrl+b d` to detach and return to the TUI.

## Quick Links

- [Installation](installation.md): Get started with Agent of Empires
- [Quick Start](quick-start.md): Basic usage tutorial
- [CLI Reference](cli/reference.md): Complete command documentation
- [Workflow Guide](guides/workflow.md): Recommended setup and daily workflow
