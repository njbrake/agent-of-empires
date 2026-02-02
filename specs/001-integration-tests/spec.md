# Feature Specification: Integration Test Coverage

**Feature Branch**: `001-integration-tests`
**Created**: 2026-02-02
**Status**: Draft
**Input**: User description: "Add comprehensive integration test coverage"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - CLI Session Lifecycle (Priority: P1)

A developer adds a session via `aoe add`, verifies it persists in storage, lists it with `aoe list`, checks status with `aoe status`, and removes it with `aoe remove`. The full CRUD lifecycle works end-to-end through the CLI layer.

**Why this priority**: The add/remove/list/status commands are the core user workflow. They're the most-used commands and currently have zero integration test coverage despite touching 8+ subsystems (storage, groups, config, git, Docker, tmux, hooks, worktrees).

**Independent Test**: Can be tested by calling library functions directly in a temp directory with mocked HOME, creating sessions, and asserting on storage state and output.

**Acceptance Scenarios**:

1. **Given** a clean temp home directory, **When** a session is added with a path and title, **Then** the session is persisted in `sessions.json` with the correct path and title
2. **Given** a session exists in storage, **When** sessions are listed, **Then** the session appears with its title, path, and status
3. **Given** a session exists, **When** it is removed by ID, **Then** the session is removed from storage and `sessions.json` reflects the deletion
4. **Given** multiple sessions exist, **When** status is checked, **Then** output shows a summary of all sessions

---

### User Story 2 - Profile Management Lifecycle (Priority: P1)

A developer creates profiles to isolate session workspaces, switches between them, and deletes unused profiles. Sessions in one profile don't appear in another.

**Why this priority**: Profile management is completely untested (0 tests) yet it's the foundation for multi-workspace isolation. It's also simple to test since it's mostly file I/O.

**Independent Test**: Can be tested by calling profile functions from `src/session/` against a temp directory and verifying directory creation, default profile tracking, and session isolation.

**Acceptance Scenarios**:

1. **Given** a clean state, **When** a profile is created, **Then** a profile directory exists and `list_profiles()` includes it
2. **Given** two profiles exist with sessions, **When** loading storage for profile A, **Then** only profile A's sessions are returned
3. **Given** a non-default profile, **When** it's set as default, **Then** `load_config()` reflects the new default
4. **Given** a profile with sessions, **When** the profile is deleted, **Then** the profile directory is removed and it no longer appears in `list_profiles()`

---

### User Story 3 - Group Management Lifecycle (Priority: P2)

A developer creates groups to organize sessions, moves sessions between groups, and deletes groups. Group state persists across app restarts.

**Why this priority**: Groups have 16 unit tests for tree logic but zero integration tests for the full create/delete/move lifecycle with persistence.

**Independent Test**: Can be tested by creating sessions and groups via library functions, saving to disk, reloading, and verifying tree structure matches expectations.

**Acceptance Scenarios**:

1. **Given** sessions exist, **When** a group is created and sessions are assigned to it, **Then** `save_with_groups()` persists both and `load_with_groups()` restores the tree
2. **Given** nested groups A/B, **When** group A is deleted with force, **Then** child sessions are reassigned correctly and storage is updated
3. **Given** sessions in multiple groups, **When** a session is moved from group X to group Y, **Then** both the session's `group_path` and the group tree are updated in storage

---

### User Story 4 - Profile Config Merge Pipeline (Priority: P2)

A developer has global settings and profile-specific overrides. When loading config for a profile, overrides take precedence over global defaults. All config fields round-trip through TOML correctly.

**Why this priority**: Config merging has 16 unit tests but the full pipeline (load global TOML, load profile override TOML, merge, use resolved config) hasn't been integration-tested with real files.

**Independent Test**: Can be tested by writing global and profile config TOML files to a temp directory, calling `merge_configs()`, and asserting on the resolved values.

**Acceptance Scenarios**:

1. **Given** global config has `sandbox.auto_cleanup = true` and profile override has `sandbox.auto_cleanup = false`, **When** configs are merged, **Then** the resolved value is `false`
2. **Given** a profile override with only `theme` set, **When** merged with global config, **Then** all non-overridden fields inherit from global
3. **Given** a config with all fields set, **When** saved to TOML and reloaded, **Then** all values round-trip correctly

---

### User Story 5 - Migration Pipeline (Priority: P2)

When the app starts with an old schema version, pending migrations run in order, the schema version file is updated, and running migrations again is a no-op (idempotency).

**Why this priority**: Migrations protect against data corruption during upgrades. Only 2 unit tests exist (sequential ordering and version match). The actual migration execution pipeline is untested.

**Independent Test**: Can be tested by creating a temp directory with old-format data, running `run_migrations()`, and verifying the data was transformed and `.schema_version` was updated.

**Acceptance Scenarios**:

1. **Given** a fresh directory with no `.schema_version`, **When** `run_migrations()` runs, **Then** all migrations execute and `.schema_version` is set to CURRENT_VERSION
2. **Given** `.schema_version` is at version N, **When** `run_migrations()` runs, **Then** only migrations > N execute
3. **Given** all migrations have already run, **When** `run_migrations()` runs again, **Then** nothing happens (idempotent)

---

### User Story 6 - Repo Config and Hook Trust (Priority: P3)

When a developer initializes `aoe` in a repo with `.aoe/config.toml`, hooks are detected. Hooks require trust verification before execution. Changing hooks invalidates trust.

**Why this priority**: Already partially tested in `repo_config.rs` but missing the full trust lifecycle: detect hooks, trust, execute, detect change, re-verify.

**Independent Test**: Can be tested by creating a temp git repo with `.aoe/config.toml` containing hooks, and verifying trust/untrust/execute behavior.

**Acceptance Scenarios**:

1. **Given** a repo with untrusted hooks, **When** hooks are trusted, **Then** `check_hook_trust()` returns trusted and hooks execute
2. **Given** trusted hooks, **When** hook content changes, **Then** `check_hook_trust()` detects the change and returns untrusted

---

### Edge Cases

- What happens when storage JSON is corrupted mid-write? (backup recovery)
- What happens when a session references a deleted worktree path?
- What happens when two profiles have the same session ID? (should be impossible due to isolation)
- What happens when HOME is unset or points to a read-only directory?
- What happens when group tree references a non-existent session?

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: Integration tests MUST cover the CLI session lifecycle (add, list, status, remove)
- **FR-002**: Integration tests MUST verify profile isolation (sessions don't leak between profiles)
- **FR-003**: Integration tests MUST verify group persistence (create, delete with force, move sessions)
- **FR-004**: Integration tests MUST verify config merge precedence (profile overrides > global)
- **FR-005**: Integration tests MUST verify migration pipeline ordering and idempotency
- **FR-006**: All new integration tests MUST use temp directories and not affect real user state
- **FR-007**: Tests requiring external tools (tmux, Docker) MUST skip gracefully or use `#[ignore]`
- **FR-008**: Tests modifying environment variables MUST use `#[serial]`

### Key Entities

- **Session/Instance**: Core data type with path, title, tool, group_path, worktree_info, sandbox_info
- **Storage**: JSON persistence layer for sessions, with backup and group tree support
- **Config/ProfileConfig**: TOML-based global and per-profile configuration with merge logic
- **GroupTree**: Tree structure for organizing sessions into named groups
- **Migration**: Versioned data transformation from old schema to new

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: All 6 user stories have passing integration tests in `tests/`
- **SC-002**: `cargo test` passes on both Linux and macOS CI
- **SC-003**: No new `#[ignore]` tests that could reasonably run without external tools
- **SC-004**: Tests are deterministic (no flaky failures on repeated runs)
