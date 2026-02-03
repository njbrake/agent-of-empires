# Research: Hooks at Global/Profile Level & Repo Settings in TUI

**Date**: 2026-02-03
**Feature Branch**: `002-hooks-settings-tui`

## R1: Where to Place HooksConfig in Global Config

**Decision**: Add `pub hooks: HooksConfig` (with `#[serde(default)]`) to
the `Config` struct in `src/session/config.rs`. Default is empty lists
for both `on_create` and `on_launch`.

**Rationale**: The existing `Config` struct already contains all
user-preference sections (worktree, sandbox, tmux, session, sound). Hooks
at the global level are user preferences - not security-sensitive repo
artifacts. The `HooksConfig` struct already exists in
`src/session/repo_config.rs` and can be reused directly.

**Alternatives considered**:
- Separate global hooks config file: Rejected. Would break the
  single-file convention for global config and add unnecessary I/O.
- Nested within an existing section: Rejected. Hooks are orthogonal to
  sandbox/session/worktree concerns.

## R2: Profile Override Pattern for Hooks

**Decision**: Create `HooksConfigOverride` in `profile_config.rs` with
`pub on_create: Option<Vec<String>>` and
`pub on_launch: Option<Vec<String>>`. Add
`pub hooks: Option<HooksConfigOverride>` to `ProfileConfig`. Follow the
existing per-field `Option<T>` override pattern.

**Rationale**: This matches every other override struct in the codebase
(e.g., `SandboxConfigOverride`, `WorktreeConfigOverride`). Per-field
granularity was confirmed during clarification.

**Alternatives considered**:
- Whole-section override (single `Option<HooksConfig>`): Rejected per
  clarification. Users expect `on_create` and `on_launch` to be
  independently overridable.

## R3: Hook Resolution Chain Integration

**Decision**: Modify `resolve_config_with_repo()` in `repo_config.rs` to
apply repo-level hook overrides per-field on top of the already-merged
global+profile config. The existing chain (global -> profile -> repo)
remains; hooks now participate at all three levels.

**Rationale**: `resolve_config_with_repo()` already calls
`merge_repo_config()`. That function already merges sandbox, session,
and worktree overrides from `RepoConfig`. Adding hooks follows the same
pattern. Since `Config` will now carry hooks, repo hooks simply override
the resolved global+profile hooks.

**Alternatives considered**:
- Separate resolution function for hooks: Rejected. Would duplicate the
  resolution chain and create maintenance burden.

## R4: Trust Semantics for Global/Profile Hooks

**Decision**: Global and profile hooks are implicitly trusted. The trust
check in `check_hook_trust()` only applies to repo-level hooks from
`.aoe/config.toml`. No changes to the trust system are needed.

**Rationale**: Global/profile hooks are written by the user in their own
config directory (which they control). Repo hooks come from cloned
repositories and may contain untrusted commands. This distinction already
exists - the trust system only reads `.aoe/config.toml`.

**Alternatives considered**:
- Trust all hooks: Rejected. Repo hooks from untrusted repos must still
  require approval.
- Trust nothing: Rejected. Asking users to trust their own global config
  would be a poor UX.

## R5: Repo Tab Architecture in Settings TUI

**Decision**: Add `SettingsCategory::Repo` as a new tab. This tab
operates differently from other tabs: it does NOT use the Global/Profile
scope toggle. Instead, it loads/saves `.aoe/config.toml` from the
currently selected session's project path. The tab is only available when
a session with a project path is selected.

**Rationale**: Repo settings are fundamentally different from
global/profile settings. They belong to a specific directory, not to the
user's app config. Using a separate tab (rather than a third scope)
makes this distinction clear in the UI.

**Alternatives considered**:
- Third scope ("Repo") alongside Global/Profile: Rejected. Would
  conflate two different storage backends (app config dir vs project
  dir) under the same scope mechanism.
- Separate dialog outside settings: Rejected. Settings TUI is the
  natural home for all configuration editing.

## R6: Repo Tab Data Flow

**Decision**: The Repo tab will:
1. Accept a `project_path: Option<String>` in `SettingsView::new()`.
2. Load `RepoConfig` from `.aoe/config.toml` at that path (or empty
   defaults if file doesn't exist).
3. Store it as `repo_config: Option<RepoConfig>` on `SettingsView`.
4. On save, serialize and write to `.aoe/config.toml`, creating the
   directory if needed.
5. If `project_path` is None, the Repo tab shows a disabled placeholder.

**Rationale**: Keeps repo config loading/saving isolated from the
global/profile save path. The `SettingsView` already tracks `has_changes`
which can cover repo changes too.

**Alternatives considered**:
- Separate save button for repo: Rejected. A single Ctrl-s save is
  simpler. The save function can dispatch to the appropriate backend
  based on which fields changed.

## R7: Sandbox Execution Semantics for Global/Profile Hooks

**Decision**: Global and profile hooks follow the same execution model
as repo hooks. The session's sandbox setting determines where hooks
run - not which config level they came from. Sandboxed sessions run all
hooks inside the container; non-sandboxed sessions run hooks locally.
Failure semantics (`on_create` = fatal, `on_launch` = non-fatal) apply
uniformly regardless of config level.

**Rationale**: The existing hook execution code in `creation_poller.rs`
and `instance.rs` already branches on `data.sandbox` / sandbox presence.
Since global/profile hooks are resolved into the same `HooksConfig`
struct as repo hooks (via the merge chain), they naturally flow through
the same execution path. No special-casing is needed.

**Alternatives considered**:
- Per-hook sandbox override (run some hooks locally, others in
  container): Rejected. Adds complexity with minimal benefit. Users who
  need host-side commands can use repo-level hooks or run them outside
  aoe.
- Always run global/profile hooks locally: Rejected. Would break
  the principle that hooks prepare the session's working environment.
  In sandboxed sessions, the working environment IS the container.

## R8: Duplicate on_launch Prevention for Global/Profile Hooks

**Decision**: The existing `on_launch_hooks_ran` flag in
`CreationResult::Success` already prevents duplicate execution. Since
global/profile hooks resolve into the same `HooksConfig` and flow
through the same creation poller code path, the skip logic in
`app.rs` (`take_on_launch_hooks_ran()`) works without modification.

**Rationale**: No new mechanism is needed. The creation poller runs
`on_launch` hooks during creation, sets the flag, and `attach_session()`
in `app.rs` checks it before re-running hooks. This works regardless of
which config level the hooks originated from.
