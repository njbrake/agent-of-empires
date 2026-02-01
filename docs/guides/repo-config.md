# Repository Configuration & Hooks

AoE supports per-repo configuration via a `.aoe/config.toml` file in your project root. This lets you define project-specific defaults and hooks that apply to every team member using AoE on that repo.

## Getting Started

Generate a template config:

```bash
aoe init
```

This creates `.aoe/config.toml` with commented-out examples. Edit the file to enable the settings you need.

## Configuration Sections

### Hooks

Hooks run shell commands at specific points in the session lifecycle.

```toml
[hooks]
# Run once when a session is first created (failures abort creation)
on_create = ["npm install", "cp .env.example .env"]

# Run every time a session starts (failures are logged but non-fatal)
on_launch = ["npm install"]
```

**`on_create`** runs only once, when the session is first created. If any command fails, session creation is aborted. Use this for one-time setup like installing dependencies or generating config files.

**`on_launch`** runs every time a session starts (including the first time, and every restart). Failures are logged as warnings but don't prevent the session from starting. Use this for things like ensuring dependencies are up to date.

For sandboxed sessions, hooks run inside the Docker container.

### Session

```toml
[session]
default_tool = "opencode"   # Override the default agent for this repo
```

Available tools: `claude`, `opencode`, `vibe`, `codex`, `gemini`.

### Sandbox

Override sandbox settings for this repo:

```toml
[sandbox]
enabled_by_default = true
default_image = "ghcr.io/njbrake/aoe-dev-sandbox:latest"
environment = ["NODE_ENV", "DATABASE_URL"]
environment_values = { CUSTOM_KEY = "value" }
volume_ignores = ["node_modules", ".next", "target"]
extra_volumes = ["/data:/data:ro"]
cpu_limit = "8"
memory_limit = "16g"
auto_cleanup = true
default_terminal_mode = "host"   # "host" or "container"
```

### Worktree

Override worktree settings for this repo:

```toml
[worktree]
enabled = true
path_template = "../{repo-name}-worktrees/{branch}"
bare_repo_path_template = "./{branch}"
auto_cleanup = true
show_branch_in_tui = true
delete_branch_on_cleanup = false
```

## Hook Trust System

When AoE encounters hooks in a repo for the first time, it prompts you to review and approve them before execution. This prevents untrusted repos from running arbitrary commands.

- Trust decisions are stored globally (shared across all profiles)
- If hook commands change (e.g., someone updates `.aoe/config.toml`), AoE prompts for re-approval
- Use `--trust-hooks` with `aoe add` to skip the trust prompt (useful for CI or repos you control)

```bash
# Trust hooks automatically
aoe add --trust-hooks .
```

## Config Precedence

Settings are resolved in this order (later overrides earlier):

1. **Global config** (`~/.agent-of-empires/config.toml`)
2. **Profile config** (`~/.agent-of-empires/profiles/<name>/config.toml`)
3. **Repo config** (`.aoe/config.toml`)

Only settings that are explicitly set in the repo config override the global/profile values. Unset fields inherit from the higher-level config.

## Example: Full Repo Config

```toml
[hooks]
on_create = ["npm install", "npx prisma generate"]
on_launch = ["npm install"]

[session]
default_tool = "claude"

[sandbox]
enabled_by_default = true
default_image = "ghcr.io/njbrake/aoe-dev-sandbox:latest"
environment = ["DATABASE_URL", "REDIS_URL"]
environment_values = { NODE_ENV = "development" }
volume_ignores = ["node_modules", ".next"]

[worktree]
enabled = true
```

## Checking Into Version Control

The `.aoe/config.toml` file is meant to be committed to your repo so the entire team shares the same configuration. The hook trust system ensures that each developer explicitly approves hook commands before they run.
