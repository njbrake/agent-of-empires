# Data Model: Hooks at Global/Profile Level & Repo Settings in TUI

**Date**: 2026-02-03
**Feature Branch**: `002-hooks-settings-tui`

## Entities

### HooksConfig (existing, reused at global level)

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| on_create | Vec<String> | [] | Commands run once on session creation |
| on_launch | Vec<String> | [] | Commands run on every session launch |

**Location**: `src/session/repo_config.rs` (existing)
**Used by**: Global config, repo config

### HooksConfigOverride (new)

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| on_create | Option<Vec<String>> | None | Profile override for on_create |
| on_launch | Option<Vec<String>> | None | Profile override for on_launch |

**Location**: `src/session/profile_config.rs` (new struct)
**Used by**: Profile config

### Config (modified)

Added field:

| Field | Type | Default | Serde |
|-------|------|---------|-------|
| hooks | HooksConfig | HooksConfig::default() | `#[serde(default)]` |

### ProfileConfig (modified)

Added field:

| Field | Type | Default | Serde |
|-------|------|---------|-------|
| hooks | Option<HooksConfigOverride> | None | `#[serde(default, skip_serializing_if = "Option::is_none")]` |

### SettingsCategory (modified enum)

New variants:

| Variant | Description |
|---------|-------------|
| Hooks | Tab for global/profile hook configuration |
| Repo | Tab for repo-level `.aoe/config.toml` editing |

### FieldKey (modified enum)

New variants:

| Variant | Category | FieldValue type |
|---------|----------|-----------------|
| HookOnCreate | Hooks | List(Vec<String>) |
| HookOnLaunch | Hooks | List(Vec<String>) |
| RepoHookOnCreate | Repo | List(Vec<String>) |
| RepoHookOnLaunch | Repo | List(Vec<String>) |

### SettingsView (modified struct)

New fields:

| Field | Type | Description |
|-------|------|-------------|
| project_path | Option<String> | Path to selected session's project dir |
| repo_config | Option<RepoConfig> | Loaded repo config for Repo tab |

## Relationships

```
Config (global)
  └── hooks: HooksConfig
        ├── on_create: Vec<String>
        └── on_launch: Vec<String>

ProfileConfig (profile override)
  └── hooks: Option<HooksConfigOverride>
        ├── on_create: Option<Vec<String>>
        └── on_launch: Option<Vec<String>>

RepoConfig (repo-level, existing)
  └── hooks: Option<HooksConfig>
        ├── on_create: Vec<String>
        └── on_launch: Vec<String>
```

## Resolution Chain

```
For each hook field (on_create, on_launch) independently:
  1. Start with global Config.hooks.{field}
  2. If ProfileConfig.hooks.{field} is Some, use it instead
  3. If RepoConfig.hooks.{field} is non-empty, use it instead
  4. Result: the most specific defined value wins
```

## State Transitions

### Settings TUI Scope Behavior

| Tab | Global scope | Profile scope | Notes |
|-----|-------------|---------------|-------|
| Hooks | Edits Config.hooks | Edits ProfileConfig.hooks | Standard scope toggle |
| Repo | N/A (scope ignored) | N/A (scope ignored) | Always edits project .aoe/config.toml |

### Repo Tab Availability

| Condition | Repo tab state |
|-----------|---------------|
| Session selected with project_path | Enabled, shows repo config fields |
| No session selected | Disabled, shows placeholder message |
| Session with no project_path | Disabled, shows placeholder message |

### Hook Execution Environment

| Session sandbox setting | Execution location | Working directory |
|------------------------|--------------------|-------------------|
| Sandbox enabled | Inside Docker container | Container workdir (e.g., `/workspace/{title}`) |
| Sandbox disabled | Local host | Project directory path |

This applies uniformly to hooks from ALL config levels (global, profile,
repo). The config level does not affect where hooks execute.

### Hook Failure Semantics

| Hook type | On failure | Applies to |
|-----------|-----------|------------|
| on_create | Fatal - abort session creation, cleanup | All config levels |
| on_launch | Non-fatal - log warning, session continues | All config levels |

### Duplicate Execution Prevention

| Event | on_launch behavior |
|-------|--------------------|
| Session creation (background) | Hooks run; `on_launch_hooks_ran` flag set |
| Subsequent attach to same session | Hooks skipped (flag checked) |
| Later re-attach (after initial) | Hooks run normally |
