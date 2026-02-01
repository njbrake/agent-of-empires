# CLAUDE.md - Development Guide for OpenClaw Studio

This is the development guide for the OpenClaw Studio project (forked from agent-of-empires).

## Project Overview

OpenClaw Studio is a terminal session manager for AI coding agents, with integrated project management, task tracking, and monitoring capabilities.

**Original:** https://github.com/njbrake/agent-of-empires
**Fork:** https://github.com/kgkgzrtk/agent-of-empires (to be renamed to openclaw-studio)

## Architecture

### Directory Structure

```
src/
├── cli/           # CLI commands (add, list, remove, status, etc.)
├── docker/        # Docker sandbox management (CLI-based)
├── git/           # Git operations (worktrees, diff)
├── migrations/    # SQLite migrations
├── process/       # Process management (macOS/Linux)
├── session/       # Session state management
│   ├── instance.rs      # Session instance (1396 lines)
│   ├── config.rs        # Configuration (835 lines)
│   ├── groups.rs        # Group management (551 lines)
│   └── ...
├── tmux/          # tmux integration
├── tui/           # TUI (ratatui-based)
│   ├── app.rs           # Main app (21K lines)
│   ├── dialogs/         # Dialog components
│   ├── home/            # Home screen
│   └── ...
└── update/        # Auto-update mechanism
```

### Key Dependencies

```toml
# TUI
ratatui = "0.29"
crossterm = "0.28"

# Async
tokio = "1.42"

# Serialization
serde = "1.0"
toml = "0.8"

# Git
git2 = "0.19"

# Process
nix = "0.29"
portable-pty = "0.8"
```

### Data Flow

```
User Input (CLI/TUI)
       │
       ▼
┌──────────────┐
│   Session    │ ──▶ SQLite DB (migrations/)
│   Manager    │
└──────┬───────┘
       │
       ▼
┌──────────────┐     ┌──────────────┐
│    tmux      │ ◀─▶ │   Docker     │
│   Sessions   │     │  Containers  │
└──────────────┘     └──────────────┘
```

## Development Phases

### Phase 1: Core Understanding (Current)
- [x] Fork repository
- [ ] Build and test
- [ ] Code analysis
- [ ] Document key components

### Phase 2: OpenClaw Integration (Next)
- [ ] Add OpenClaw Gateway client
- [ ] Integrate bollard for Docker (vs CLI)
- [ ] Add channel binding support

### Phase 3: Orchestration Features
- [ ] Project context switching
- [ ] Task management (TASKS.md sync)
- [ ] Cron/Heartbeat integration
- [ ] Dead Man's Switch

## Key Components to Understand

### 1. Session Instance (`src/session/instance.rs`)

The core session representation with:
- Docker sandbox configuration
- Environment variable handling
- Shell command generation

```rust
pub struct Session {
    pub id: Uuid,
    pub name: String,
    pub path: PathBuf,
    pub sandbox: Option<SandboxInfo>,
    // ...
}
```

### 2. TUI App (`src/tui/app.rs`)

Main application state and event loop using ratatui.

### 3. tmux Integration (`src/tmux/`)

Session management and status detection for AI agents.

## Integration Points with ocpm

| ocpm Feature | aoe Equivalent | Integration Plan |
|--------------|----------------|------------------|
| bollard Docker | CLI Docker | Keep both, adapter pattern |
| Channel binding | N/A | Add to session config |
| OpenClaw config | N/A | New module `src/openclaw/` |
| Browser enable | N/A | Add to sandbox config |

## Build & Test

```bash
# Build
cargo build --release

# Run TUI
./target/release/aoe

# Run CLI
./target/release/aoe add /path/to/project
./target/release/aoe list
```

## Coding Conventions

1. **Error Handling**: Use `anyhow::Result` for CLI, custom errors for library
2. **Async**: Tokio runtime, prefer async where possible
3. **TUI**: Follow existing ratatui patterns in `src/tui/`
4. **Tests**: Unit tests in same file, integration tests in `tests/`

## Next Steps

1. Complete build verification
2. Test basic functionality (add session, TUI, Docker sandbox)
3. Create integration branch for OpenClaw features
4. Implement OpenClaw Gateway client module

---

*Last Updated: 2026-02-01*
