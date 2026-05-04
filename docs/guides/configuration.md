# Configuration Reference

AoE uses a layered configuration system. Settings are resolved in this order:

1. **Global config** -- `~/.agent-of-empires/config.toml` (or `~/.config/agent-of-empires/config.toml` on Linux)
2. **Profile config** -- `~/.agent-of-empires/profiles/<name>/config.toml`
3. **Repo config** -- `.agent-of-empires/config.toml` in the project root

Later layers override earlier ones. Only explicitly set fields override; unset fields inherit from the previous layer.

All settings below can also be edited from the TUI settings screen (press `s` or access via the menu).

## File Locations

| Platform | Global Config |
|----------|--------------|
| Linux | `$XDG_CONFIG_HOME/agent-of-empires/config.toml` (defaults to `~/.config/agent-of-empires/`) |
| macOS | `~/.agent-of-empires/config.toml` |

```
~/.agent-of-empires/
  config.toml              # Global configuration
  trusted_repos.toml       # Hook trust decisions (auto-managed)
  .schema_version          # Migration tracking (auto-managed)
  profiles/
    default/
      sessions.json        # Session data
      groups.json          # Group hierarchy
      config.toml          # Profile-specific overrides
  logs/                    # Session execution logs
```

## Environment Variables

| Variable | Description |
|----------|-------------|
| `AGENT_OF_EMPIRES_PROFILE` | Default profile to use |
| `AGENT_OF_EMPIRES_DEBUG` | Enable debug logging to `debug.log` in app data dir (`1` to enable) |

## Theme

```toml
[theme]
name = "empire"   # empire, phosphor, tokyo-night-storm, catppuccin-latte, dracula
```

| Option | Default | Description |
|--------|---------|-------------|
| `name` | `"empire"` | TUI color theme. Available: `empire` (warm navy/amber), `phosphor` (green), `tokyo-night-storm` (dark blue/purple), `catppuccin-latte` (light pastel), `dracula` (dark purple/pink). |

## Session

```toml
[session]
default_tool = "claude"   # any supported agent name
yolo_mode_default = false
agent_status_hooks = true
```

| Option | Default | Description |
|--------|---------|-------------|
| `default_tool` | (auto-detect) | Default agent for new sessions. Falls back to the first available tool if unset or unavailable. Can be set to a custom agent name. |
| `yolo_mode_default` | `false` | Enable YOLO mode by default for new sessions (skip permission prompts). Works with or without sandbox. |
| `agent_status_hooks` | `true` | Install status-detection hooks into the agent's settings file. When disabled, status detection falls back to tmux pane content parsing. |
| `agent_extra_args` | `{}` | Per-agent extra arguments appended after the binary (e.g., `{ opencode = "--port 8080" }`). |
| `agent_command_override` | `{}` | Per-agent command override replacing the binary entirely (e.g., `{ claude = "my-claude-wrapper" }`). |
| `custom_agents` | `{}` | User-defined agents: name to command mapping. Custom agent names appear in the TUI agent picker alongside built-in agents. |
| `agent_detect_as` | `{}` | Status detection mapping: maps an agent name to a built-in agent whose status heuristics should be used. |

### Custom Agents

You can register additional agents (SSH wrappers to remote machines, custom workflows, etc.) that appear in the TUI agent picker alongside built-in agents like `claude`, `opencode`, and `codex`.

```toml
[session]
custom_agents = { "lenovo-claude" = "ssh -t lenovo claude" }
agent_detect_as = { "lenovo-claude" = "claude" }
```

- **`custom_agents`**: Maps a display name to the command to run. The name appears in the agent picker when creating a new session, and the command is auto-filled as the session's command override.
- **`agent_detect_as`** (optional): Maps a custom agent to a built-in agent's status detection. Without this, custom agents default to `Idle` status. Setting `"lenovo-claude" = "claude"` reuses Claude's Running/Waiting/Idle detection heuristics for the remote session.

Custom agents are always shown as available in the picker (no binary detection), since the command may target a remote host or a wrapper script.

You can also set `default_tool` to a custom agent name:

```toml
[session]
default_tool = "lenovo-claude"
custom_agents = { "lenovo-claude" = "ssh -t lenovo claude" }
agent_detect_as = { "lenovo-claude" = "claude" }
```

Both fields are editable from the TUI settings screen and support profile/repo-level overrides.

> **Note:** Profile and repo-level overrides fully replace the global value rather than merging with it. A profile that defines `custom_agents` replaces the entire global set, so you must redeclare any global agents you want to keep in that profile.

## Worktree

```toml
[worktree]
enabled = false
path_template = "../{repo-name}-worktrees/{branch}"
bare_repo_path_template = "./{branch}"
auto_cleanup = true
show_branch_in_tui = true
delete_branch_on_cleanup = false
```

| Option | Default | Description |
|--------|---------|-------------|
| `enabled` | `false` | Enable worktree support for new sessions |
| `path_template` | `../{repo-name}-worktrees/{branch}` | Path template for worktrees in regular repos |
| `bare_repo_path_template` | `./{branch}` | Path template for worktrees in bare repos |
| `auto_cleanup` | `true` | Prompt to remove worktree when deleting a session |
| `show_branch_in_tui` | `true` | Display branch name in the TUI session list |
| `delete_branch_on_cleanup` | `false` | Also delete the git branch when removing a worktree |

**Template variables:**

| Variable | Description |
|----------|-------------|
| `{repo-name}` | Repository folder name |
| `{branch}` | Branch name (slashes converted to hyphens) |
| `{session-id}` | First 8 characters of session UUID |

## Sandbox (Docker)

```toml
[sandbox]
enabled_by_default = false
default_image = "ghcr.io/njbrake/aoe-sandbox:latest"
cpu_limit = "4"
memory_limit = "8g"
port_mappings = ["3000:3000", "5432:5432"]
environment = ["ANTHROPIC_API_KEY", "OPENAI_API_KEY", "GH_TOKEN=$AOE_GH_TOKEN"]
extra_volumes = []
volume_ignores = ["node_modules", "target"]
auto_cleanup = true
default_terminal_mode = "host"
```

| Option | Default | Description |
|--------|---------|-------------|
| `enabled_by_default` | `false` | Auto-enable sandbox for new sessions |
| `default_image` | `ghcr.io/njbrake/aoe-sandbox:latest` | Docker image for containers |
| `cpu_limit` | (none) | CPU limit (e.g., `"4"`) |
| `memory_limit` | (none) | Memory limit (e.g., `"8g"`) |
| `port_mappings` | `[]` | Host-to-container port mappings (e.g., `["3000:3000"]`) |
| `environment` | `["TERM", "COLORTERM", "FORCE_COLOR", "NO_COLOR"]` | Env vars for containers (see below) |
| `extra_volumes` | `[]` | Additional Docker volume mounts |
| `volume_ignores` | `[]` | Directories to exclude from the project mount via anonymous volumes |
| `auto_cleanup` | `true` | Remove containers when sessions are deleted |
| `default_terminal_mode` | `"host"` | Paired terminal location: `"host"` or `"container"` |

### environment entries

Each entry in the `environment` list can be:
- **`KEY`** (bare name) -- passes the host env var value into the container
- **`KEY=VALUE`** -- sets an explicit value; if VALUE starts with `$`, it reads from a host env var (e.g., `GH_TOKEN=$AOE_GH_TOKEN`). Use `$$` for a literal `$`.

## tmux

```toml
[tmux]
status_bar = "auto"
mouse = "auto"
clipboard = "auto"
```

| Option | Default | Description |
|--------|---------|-------------|
| `status_bar` | `"auto"` | `"auto"`: apply if no `~/.tmux.conf`; `"enabled"`: always apply; `"disabled"`: never apply |
| `mouse` | `"auto"` | Same modes as `status_bar`. Controls mouse support in aoe tmux sessions. |
| `clipboard` | `"auto"` | Same modes. Forwards OSC 52 clipboard escape sequences from the wrapped agent (Claude Code, OpenCode, Codex, etc.) through tmux to your terminal. Without this, "select to copy" inside the agent silently fails. Sets `set-clipboard on` and `allow-passthrough on` on the aoe tmux session. |

## Diff

```toml
[diff]
default_branch = "main"
context_lines = 3
```

| Option | Default | Description |
|--------|---------|-------------|
| `default_branch` | (auto-detect) | Base branch for diffs |
| `context_lines` | `3` | Lines of context around changes |

## Updates

```toml
[updates]
check_enabled = true
auto_update = false
check_interval_hours = 24
notify_in_cli = true
```

| Option | Default | Description |
|--------|---------|-------------|
| `check_enabled` | `true` | Check for new versions |
| `auto_update` | `false` | Automatically install updates |
| `check_interval_hours` | `24` | Hours between update checks |
| `notify_in_cli` | `true` | Show update notifications in CLI output |

## Claude

```toml
[claude]
config_dir = "~/.claude"
```

| Option | Default | Description |
|--------|---------|-------------|
| `config_dir` | (none) | Custom Claude Code config directory. Supports `~/` prefix. |

## Profiles

Profiles provide separate workspaces with their own sessions and groups. Each profile can override any of the settings above.

```bash
aoe                 # Uses "default" profile
aoe -p work         # Uses "work" profile
aoe profile create client-xyz
aoe profile list
aoe profile default work   # Set "work" as default
```

Profile overrides go in `~/.agent-of-empires/profiles/<name>/config.toml` and use the same format as the global config.

## Repo Config

Per-repo settings go in `.agent-of-empires/config.toml` at your project root. Run `aoe init` to generate a template.

Repo config supports: `[hooks]`, `[session]`, `[sandbox]`, and `[worktree]` sections. It does not support `[tmux]`, `[updates]`, `[claude]`, or `[diff]` -- those are personal settings.

See [Repo Config & Hooks](repo-config.md) for details.
