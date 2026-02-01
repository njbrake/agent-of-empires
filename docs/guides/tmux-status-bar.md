# tmux Status Bar

Agent of Empires can display session information in your tmux status bar, showing:
- **Session title**: The name of your aoe session
- **Git branch**: For worktree sessions
- **Container name**: For sandboxed (Docker) sessions

## How It Works

When you start a session, aoe sets tmux user options (`@aoe_title`, `@aoe_branch`, `@aoe_sandbox`) and configures the status bar to display this information with aoe's phosphor green theme.

**Example status bars:**
```
aoe: My Session | 14:30                           # Basic session
aoe: My Session | feature-branch | 14:30          # Worktree session
aoe: My Session ⬡ aoe_my_container | 14:30        # Sandboxed session
aoe: My Session | main ⬡ aoe_container | 14:30    # Worktree + sandbox
```

## Auto Mode (Default)

By default, aoe uses "auto" mode for the status bar:

- **If you don't have a `~/.tmux.conf`**: aoe automatically styles the status bar for aoe sessions
- **If you have a `~/.tmux.conf`**: aoe assumes you prefer your own configuration and does not modify the status bar

This ensures beginners get a helpful status bar out of the box, while experienced tmux users retain full control.

## Configuration

Configure the status bar behavior in `~/.agent-of-empires/config.toml`:

```toml
[tmux]
# "auto" (default) - Apply only if no ~/.tmux.conf exists
# "enabled"        - Always apply aoe status bar styling
# "disabled"       - Never apply, use your own tmux config
status_bar = "auto"
mouse = "auto"    # Same modes: auto, enabled, disabled
```

### Values

| Value | Description |
|-------|-------------|
| `auto` | Apply status bar if user has no tmux config (default) |
| `enabled` | Always apply aoe status bar to aoe sessions |
| `disabled` | Never modify tmux status bar |

## Custom Integration

If you have your own tmux configuration but want to display aoe session info, use the `aoe tmux status` command.

### Basic Integration

Add this to your `~/.tmux.conf`:

```tmux
set -g status-right "#(aoe tmux status) | %H:%M"
```

This will show the aoe session title and branch when attached to an aoe session, and nothing when in other tmux sessions.

### JSON Output

For more advanced scripting:

```bash
aoe tmux status --format json
```

Output:
```json
{"title": "My Session", "branch": "feature-branch", "sandbox": null}
```

For a sandboxed session:
```json
{"title": "My Session", "branch": null, "sandbox": "aoe_my_container"}
```

Returns `null` if not in an aoe session.

### Example: Conditional Display

```tmux
# Only show aoe info if in an aoe session
set -g status-right "#{?#{==:#(aoe tmux status),},,%#(aoe tmux status) | }%H:%M"
```

## tmux User Options

When aoe starts a session with status bar enabled, it sets these tmux options:

| Option | Description |
|--------|-------------|
| `@aoe_title` | Session title |
| `@aoe_branch` | Git branch (worktree sessions only) |
| `@aoe_sandbox` | Container name (sandboxed sessions only) |

You can reference these in your own tmux config:

```tmux
set -g status-right "#{@aoe_title} #{@aoe_branch} #{@aoe_sandbox} | %H:%M"
```

## Troubleshooting

### Status bar not showing

1. Check if you have a `~/.tmux.conf` or `~/.config/tmux/tmux.conf`
2. If so, either:
   - Set `status_bar = "enabled"` in your aoe config
   - Or add `aoe tmux status` to your tmux.conf manually

### Status bar shows old info

The tmux user options are set when the session starts. If you rename a session in aoe, the status bar will show the old name until you restart the session.

### Branch not showing

Branch is only displayed for worktree sessions (sessions created with `aoe add --worktree`). Regular sessions don't have a fixed branch.

### Container not showing

Container name is only displayed for sandboxed sessions (sessions created with `aoe add --sandbox`). The container name follows the pattern `aoe_<session_id>`.
