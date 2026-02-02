# Agent of Empires

[![YouTube](https://img.shields.io/badge/YouTube-channel-red?logo=youtube)](https://www.youtube.com/@agent-of-empires)

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

<div class="cta-box">
<p><strong>Ready to get started?</strong></p>
<p><a href="installation.html">Install AoE</a></p>
</div>
