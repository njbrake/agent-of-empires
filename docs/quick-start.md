# Quick Start

## Launch the TUI

The simplest way to use Agent of Empires is through the TUI dashboard:

```bash
aoe
```

This opens an interactive interface where you can:

- View all your coding sessions
- Create new sessions with `n`
- Attach to sessions with `Enter`
- Delete sessions with `d`
- Quit with `q`

## CLI Quick Reference

### Add a Session

```bash
# Add session in current directory
aoe add

# Add session with custom title
aoe add -t "my-feature"

# Add and launch immediately
aoe add -l

# Add session for specific project
aoe add /path/to/project
```

### List Sessions

```bash
# Table format
aoe list

# JSON format
aoe list --json
```

### Manage Sessions

```bash
# Attach to a session
aoe session attach my-session

# Start a stopped session
aoe session start my-session

# Stop a running session
aoe session stop my-session

# Show session details
aoe session show my-session
```

### Check Status

```bash
# Summary of all sessions
aoe status

# Detailed status
aoe status -v

# JSON format (for scripts)
aoe status --json
```

## Profiles

Profiles let you maintain separate workspaces:

```bash
# Use default profile
aoe

# Use a specific profile
aoe -p work

# Create a new profile
aoe profile create client-xyz

# List all profiles
aoe profile list
```

## Configuration

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

## Environment Variables

| Variable | Description |
|----------|-------------|
| `AGENT_OF_EMPIRES_PROFILE` | Default profile to use |
| `AGENT_OF_EMPIRES_DEBUG` | Enable debug logging |

## tmux Status Bar

By default, aoe displays session info in the tmux status bar for users without an existing tmux configuration. This shows:

- **Session title** - The name of your aoe session
- **Git branch** - For worktree sessions

If you have your own `~/.tmux.conf`, aoe won't modify your status bar. You can:

- Set `status_bar = "enabled"` in `~/.agent-of-empires/config.toml` to always show aoe info
- Or add `#(aoe tmux status)` to your tmux.conf for custom integration

See [tmux Status Bar Guide](guides/tmux-status-bar.md) for details.

## Next Steps

- See the [CLI Reference](cli/reference.md) for complete command documentation
- Learn about [Git Worktrees](guides/worktrees.md) for parallel development
- Customize the [tmux Status Bar](guides/tmux-status-bar.md)
