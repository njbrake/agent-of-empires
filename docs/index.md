# Agent of Empires

A terminal session manager for AI coding agents on Linux and macOS, built on tmux and written in Rust.

AoE lets you run multiple AI agents in parallel -- each in its own tmux session, optionally on its own git branch, optionally inside a Docker container. A TUI dashboard shows you what every agent is doing at a glance.

![Agent of Empires Demo](assets/demo.gif)

## Why AoE?

**The problem:** You're working with AI coding agents (Claude Code, OpenCode, Codex, etc.) and want to run several in parallel across different tasks or branches. Managing multiple terminal windows, git branches, and container lifecycles by hand gets tedious fast.

**AoE handles it for you:**

- **One dashboard for all agents.** See status (running, waiting, idle, error) at a glance. Toggle to paired shell terminals with `t`.
- **Git worktrees built in.** Create a session and AoE creates a branch + worktree automatically. Delete the session and AoE cleans up.
- **Docker sandboxing.** Run agents in isolated containers with your project mounted and auth credentials shared across containers.
- **Per-repo configuration.** Drop a `.aoe/config.toml` in your repo for project-specific settings and hooks that run on session creation or launch.
- **Sessions survive everything.** AoE wraps tmux, so agents keep running when you close the TUI, disconnect SSH, or your terminal crashes.

## Supported Agents

Claude Code, OpenCode, Mistral Vibe, Codex CLI, and Gemini CLI. AoE auto-detects which are installed.

## Documentation

### Getting Started

- [Installation](installation.md) -- prerequisites and install methods
- [Quick Start](quick-start.md) -- create your first session in under a minute

### Guides

- [Workflow Guide](guides/workflow.md) -- recommended setup with bare repos and worktrees
- [Docker Sandbox](guides/sandbox.md) -- container isolation, images, and volume mounts
- [Repo Config & Hooks](guides/repo-config.md) -- per-project settings and automation
- [Git Worktrees](guides/worktrees.md) -- branch management and worktree templates
- [Diff View](guides/diff-view.md) -- review and edit git changes in the TUI
- [tmux Status Bar](guides/tmux-status-bar.md) -- session info in your tmux status line

### Reference

- [CLI Reference](cli/reference.md) -- every command and flag
- [Configuration Reference](guides/configuration.md) -- all config options (global, profile, repo-level)

### Contributing

- [Development](development.md) -- building, testing, and generating demo assets
