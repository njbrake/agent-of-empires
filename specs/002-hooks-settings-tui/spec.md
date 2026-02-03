# Feature Specification: Hooks at Global/Profile Level & Repo Settings in TUI

**Feature Branch**: `002-hooks-settings-tui`
**Created**: 2026-02-03
**Status**: Draft
**Input**: User description: "I would like the hooks feature to be able to be specified at a global and profile level. Also, I would like for the settings TUI to allow me to edit directory/project level settings"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Configure Global Default Hooks (Priority: P1)

As a user, I want to define default `on_create` and `on_launch` hooks at
the global config level so that every new session automatically runs my
preferred setup commands without requiring a per-repo `.aoe/config.toml`.

**Why this priority**: This is the foundational change. Hooks must exist
in the global config before profiles or TUI editing can layer on top.

**Independent Test**: Create a session for a repo that has no
`.aoe/config.toml`. Verify the global hooks execute during creation and
launch.

**Acceptance Scenarios**:

1. **Given** a user has `on_create = ["echo hello"]` in their global
   config, **When** they create a non-sandboxed session for a repo with
   no `.aoe/config.toml`, **Then** the `echo hello` command runs locally
   in the project directory during session creation.
2. **Given** a user has `on_create = ["echo hello"]` in their global
   config, **When** they create a sandboxed session for a repo with no
   `.aoe/config.toml`, **Then** the `echo hello` command runs inside the
   sandbox container in the container's working directory.
3. **Given** a user has `on_launch = ["echo launching"]` globally,
   **When** they launch any session, **Then** the command runs on every
   launch in the appropriate environment (local or container) matching
   the session's sandbox setting.
4. **Given** a user has global hooks and the repo also defines hooks in
   `.aoe/config.toml`, **When** the session is created/launched,
   **Then** only the repo-level hooks execute (the most specific level
   wins; global hooks are not appended).

---

### User Story 2 - Configure Profile-Level Hook Overrides (Priority: P2)

As a user, I want to define hooks at the profile level so that different
profiles (e.g., "work" vs "personal") can have different default hooks
that override or extend the global defaults.

**Why this priority**: Profile overrides follow the established
global-then-profile pattern already used for sandbox, worktree, and
other config sections.

**Independent Test**: Create two profiles with different `on_launch`
hooks. Switch between profiles and verify each profile's hooks execute
for its sessions.

**Acceptance Scenarios**:

1. **Given** a user has global hooks and a profile with its own hooks,
   **When** a session is created under that profile, **Then** the
   profile hooks replace the global hooks for that session.
2. **Given** a user clears the profile hook override, **When** a session
   is created, **Then** the global hooks apply again.
3. **Given** a profile defines only `on_create` hooks but not
   `on_launch`, **When** a session launches, **Then** `on_create` uses
   the profile value and `on_launch` falls back to the global value.

---

### User Story 3 - Edit Hooks in the Settings TUI (Priority: P3)

As a user, I want to view and edit `on_create` and `on_launch` hooks in
the settings TUI under a dedicated "Hooks" tab, for both global and
profile scopes, so I do not have to manually edit config files.

**Why this priority**: TUI editing builds on the config fields from US1
and US2. It uses the existing settings infrastructure (tabs, field
types, scope switching).

**Independent Test**: Open settings TUI, navigate to the Hooks tab, add
a hook command, save, and verify the config file is updated.

**Acceptance Scenarios**:

1. **Given** the user opens the settings TUI, **When** they navigate to
   the "Hooks" tab, **Then** they see `on_create` and `on_launch` as
   editable list fields.
2. **Given** the user is in Profile scope on the Hooks tab, **When**
   they add a hook, **Then** it is saved as a profile override and
   visually marked as overridden.
3. **Given** the user presses the clear-override key ('r') on a
   profile hook field, **When** the override is cleared, **Then** the
   field reverts to the global value.

---

### User Story 4 - Edit Repo-Level Settings in TUI (Priority: P4)

As a user, I want a new "Repo" tab (or scope) in the settings TUI that
lets me view and edit the `.aoe/config.toml` for the currently selected
session's project directory, so I can manage repo-level hooks without
manually editing files.

**Why this priority**: This introduces a new scope/tab to settings,
which is a larger UX change. It depends on the hooks fields being
defined (US3) and extends the pattern to repo-level config.

**Independent Test**: Select a session, open settings, navigate to the
Repo tab, edit hooks, save, and verify `.aoe/config.toml` is updated in
the project directory.

**Acceptance Scenarios**:

1. **Given** the user has a session selected on the home screen,
   **When** they open settings and navigate to the Repo tab, **Then**
   they see the repo-level hooks from `.aoe/config.toml` (or empty
   fields if no config exists).
2. **Given** the user edits repo-level hooks and saves, **When** they
   inspect `.aoe/config.toml` in the project directory, **Then** the
   file reflects the changes.
3. **Given** no session is selected (or the session has no project
   path), **When** the user tries to access the Repo tab, **Then** the
   tab is disabled or shows a message indicating no project is
   available.
4. **Given** the user adds hooks to a repo that had no `.aoe/config.toml`,
   **When** they save, **Then** the file and `.aoe/` directory are
   created automatically.

---

### Edge Cases

- What happens when the global config defines hooks but the user has
  never trusted them? Global/profile hooks are user-authored, so they
  are implicitly trusted (no trust dialog needed). Only repo-level hooks
  from `.aoe/config.toml` require trust approval.
- What happens when a hook command contains special characters (quotes,
  pipes, semicolons)? The list editor accepts freeform text; commands
  are passed to the shell as-is, matching current repo-hook behavior.
- What happens when the user saves an empty hooks list? An empty list
  means "no hooks". The `[hooks]` section is omitted from the config
  file entirely.
- What happens when a repo config defines fields beyond hooks (future
  fields)? The Repo tab should display all repo-configurable fields, not
  just hooks. For now, hooks are the only repo-level fields, but the tab
  design should accommodate future additions.
- Where do hooks execute when sandbox is enabled? Hooks ALWAYS follow
  the session's sandbox setting. If the session uses a sandbox (Docker
  container), all hooks - regardless of whether they come from global,
  profile, or repo config - execute inside the container in the
  container's working directory. If the session is not sandboxed, hooks
  execute locally in the project directory. There is no per-hook
  override for execution location.
- What happens if the container is not yet running when hooks need to
  execute? The system ensures the container is running before executing
  hooks inside it. If the container fails to start, `on_create` hooks
  abort session creation; `on_launch` hooks are skipped with a warning.
- What are the failure semantics? `on_create` hooks that fail are fatal
  and abort session creation (cleanup occurs). `on_launch` hooks that
  fail are non-fatal - a warning is logged but the session starts
  normally. This applies regardless of whether hooks come from global,
  profile, or repo config.
- What about duplicate execution of `on_launch` hooks? When a session is
  first created, `on_launch` hooks run in the background during
  creation. When the user then attaches to that session, the system
  skips `on_launch` hooks to prevent double execution.

## Clarifications

### Session 2026-02-03

- Q: When multiple levels define hooks, should they merge/append or override? → A: Override/replace. The most specific level wins entirely (repo > profile > global). No merging across layers.
- Q: Should override granularity be per-field or whole-section? → A: Per-field. Each hook type (`on_create`, `on_launch`) is independently resolved. Unset fields fall back to the next level.
- Q: Where do global/profile hooks execute when sandbox is enabled? → A: All hooks follow the session's sandbox setting. Sandboxed sessions run hooks inside the container; non-sandboxed sessions run hooks locally. No per-hook location override. Failure semantics (on_create=fatal, on_launch=non-fatal) apply uniformly regardless of config level.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST support `on_create` and `on_launch` hook lists
  in the global config file (`config.toml`).
- **FR-002**: System MUST support `on_create` and `on_launch` hook lists
  as profile overrides, following the existing `Option<T>` override
  pattern.
- **FR-003**: Hook resolution MUST be per-field (`on_create` and
  `on_launch` resolved independently) with override order: repo >
  profile > global. For each field, the most specific level that defines
  it wins. Unset fields fall back to the next level.
- **FR-004**: Global and profile hooks MUST NOT require trust approval
  (they are user-authored in the app's own config directory). Only
  repo-level hooks require trust.
- **FR-005**: The settings TUI MUST include a "Hooks" tab with
  `on_create` and `on_launch` fields displayed as editable lists.
- **FR-006**: The settings TUI MUST include a "Repo" tab that loads and
  saves `.aoe/config.toml` from the currently selected session's project
  directory.
- **FR-007**: The Repo tab MUST be disabled or show a placeholder when
  no session is selected or the session has no associated project path.
- **FR-008**: Saving repo settings MUST create the `.aoe/` directory and
  `config.toml` file if they do not exist.
- **FR-009**: The Hooks tab MUST support both Global and Profile scopes,
  using the existing scope-switching mechanism (Tab key).
- **FR-010**: Clearing a profile override on a hook field MUST revert to
  the global value, following the existing clear-override pattern.
- **FR-011**: Hooks from all config levels (global, profile, repo) MUST
  execute in the session's sandbox container when the session has sandbox
  enabled. When sandbox is disabled, hooks MUST execute locally in the
  project directory. The execution location is determined solely by the
  session's sandbox setting, not by which config level the hooks came
  from.
- **FR-012**: `on_create` hook failures MUST abort session creation
  regardless of which config level the hooks originated from. `on_launch`
  hook failures MUST be non-fatal (logged as warnings) regardless of
  config level origin.
- **FR-013**: When a session is first created and `on_launch` hooks run
  during the creation process, the system MUST skip `on_launch` hooks
  when the user subsequently attaches to that session, preventing
  duplicate execution.

### Key Entities

- **HooksConfig**: Contains `on_create` and `on_launch` command lists.
  Currently exists for repo config; will be reused at global and profile
  levels.
- **RepoConfig**: The per-repository configuration loaded from
  `.aoe/config.toml`. Currently contains hooks; the Repo tab exposes
  this for TUI editing.
- **SettingsCategory::Hooks**: New tab in the settings TUI for hook
  management at global/profile scope.
- **SettingsCategory::Repo**: New tab in the settings TUI for
  repo-level configuration editing.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Users can define hooks at global, profile, and repo levels
  and the correct hooks execute based on the override hierarchy.
- **SC-002**: Users can add, edit, and remove hook commands entirely
  within the settings TUI without manually editing config files.
- **SC-003**: All hook-related settings fields are fully wired (FieldKey,
  SettingField, apply functions, clear override) following the existing
  pattern.
- **SC-004**: Repo-level settings are viewable and editable from the TUI
  when a session with a project path is selected.
- **SC-005**: Existing repo-level hook behavior (trust system, execution
  semantics, Docker support) remains unchanged.
