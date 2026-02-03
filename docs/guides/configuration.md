# Configuration Reference

AoE uses a layered configuration system. Settings are resolved in this order:

1. **Global config** -- `~/.agent-of-empires/config.toml` (or `~/.config/agent-of-empires/config.toml` on Linux)
2. **Profile config** -- `~/.agent-of-empires/profiles/<name>/config.toml`
3. **Repo config** -- `.aoe/config.toml` in the project root

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
| `AGENT_OF_EMPIRES_DEBUG` | Enable debug logging (`1` to enable) |

## Session

```toml
[session]
default_tool = "claude"   # claude, opencode, vibe, codex, gemini
```

| Option | Default | Description |
|--------|---------|-------------|
| `default_tool` | (auto-detect) | Default agent for new sessions. Falls back to the first available tool if unset or unavailable. |

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
yolo_mode_default = false
default_image = "ghcr.io/njbrake/aoe-sandbox:latest"
cpu_limit = "4"
memory_limit = "8g"
environment = ["ANTHROPIC_API_KEY", "OPENAI_API_KEY"]
environment_values = { GH_TOKEN = "$AOE_GH_TOKEN" }
extra_volumes = []
volume_ignores = ["node_modules", "target"]
auto_cleanup = true
default_terminal_mode = "host"
```

| Option | Default | Description |
|--------|---------|-------------|
| `enabled_by_default` | `false` | Auto-enable sandbox for new sessions |
| `yolo_mode_default` | `false` | Skip agent permission prompts in sandbox |
| `default_image` | `ghcr.io/njbrake/aoe-sandbox:latest` | Docker image for containers |
| `cpu_limit` | (none) | CPU limit (e.g., `"4"`) |
| `memory_limit` | (none) | Memory limit (e.g., `"8g"`) |
| `environment` | `["TERM", "COLORTERM", "FORCE_COLOR", "NO_COLOR"]` | Host env var names to pass through |
| `environment_values` | `{}` | Env vars with explicit values (see below) |
| `extra_volumes` | `[]` | Additional Docker volume mounts |
| `volume_ignores` | `[]` | Directories to exclude from the project mount via anonymous volumes |
| `auto_cleanup` | `true` | Remove containers when sessions are deleted |
| `default_terminal_mode` | `"host"` | Paired terminal location: `"host"` or `"container"` |

### environment vs environment_values

- **`environment`** passes host env vars by name. The host value is read at container start.
- **`environment_values`** injects fixed values. Values starting with `$` reference a host env var (e.g., `"$AOE_GH_TOKEN"` reads `AOE_GH_TOKEN` from the host). Use `$$` for a literal `$`.

## tmux

```toml
[tmux]
status_bar = "auto"
mouse = "auto"
```

| Option | Default | Description |
|--------|---------|-------------|
| `status_bar` | `"auto"` | `"auto"`: apply if no `~/.tmux.conf`; `"enabled"`: always apply; `"disabled"`: never apply |
| `mouse` | `"auto"` | Same modes as `status_bar`. Controls mouse support in aoe tmux sessions. |

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

Per-repo settings go in `.aoe/config.toml` at your project root. Run `aoe init` to generate a template.

Repo config supports: `[hooks]`, `[session]`, `[sandbox]`, and `[worktree]` sections. It does not support `[tmux]`, `[updates]`, `[claude]`, or `[diff]` -- those are personal settings.

See [Repo Config & Hooks](repo-config.md) for details.
