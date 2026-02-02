# Implementation Plan: Integration Test Coverage

**Branch**: `001-integration-tests` | **Date**: 2026-02-02 | **Spec**: `specs/001-integration-tests/spec.md`
**Input**: Feature specification from `/specs/001-integration-tests/spec.md`

## Summary

Add integration tests for the 6 highest-priority untested areas: CLI session lifecycle, profile management, group persistence, config merge pipeline, migration execution, and hook trust lifecycle. Tests call library functions directly (not spawning the binary) against temp directories with `HOME` overridden, following the existing patterns in the codebase.

## Technical Context

**Language/Version**: Rust (stable toolchain)
**Primary Dependencies**: `tempfile`, `serial_test`, `anyhow`
**Storage**: JSON (sessions/groups) + TOML (config) on local filesystem
**Testing**: `cargo test` with `#[cfg(test)]` unit tests + `tests/*.rs` integration tests
**Target Platform**: Linux + macOS (CI matrix)
**Constraints**: Tests must be deterministic, isolated (temp dirs), and not require tmux/Docker unless `#[ignore]`d

## Constitution Check

| Gate | Status |
|------|--------|
| Tests use temp directories, not real user state | Pass |
| Tests with env var mutations use `#[serial]` | Pass |
| External tool tests skip/ignore gracefully | Pass |
| `cargo fmt` + `cargo clippy` clean | Will verify |

## Project Structure

### Documentation (this feature)

```text
specs/001-integration-tests/
├── spec.md
├── plan.md              # This file
└── tasks.md             # Created by /speckit.tasks
```

### Source Code (new test files)

```text
tests/
├── session_lifecycle.rs     # User Story 1: add/list/remove via library API
├── profile_management.rs    # User Story 2: create/delete/switch/isolate profiles
├── group_persistence.rs     # User Story 3: group CRUD with disk round-trip
├── config_merge.rs          # User Story 4: global + profile override merge pipeline
├── migration_pipeline.rs    # User Story 5: migration execution + idempotency
├── repo_config.rs           # User Story 6: already exists, extend with trust lifecycle
```

**Structure Decision**: One test file per user story, matching the existing pattern (e.g., `worktree_integration.rs`, `config_wiring.rs`). Tests live at `tests/` root alongside the 7 existing integration test files.

## Implementation Approach

All tests follow the same isolation pattern already used in the codebase:

```rust
fn setup_temp_home() -> tempfile::TempDir {
    let temp = tempfile::TempDir::new().unwrap();
    std::env::set_var("HOME", temp.path());
    #[cfg(target_os = "linux")]
    std::env::set_var("XDG_CONFIG_HOME", temp.path().join(".config"));
    temp
}
```

Tests call library functions from `agent_of_empires::session::*` directly rather than spawning the `aoe` binary. This avoids tmux dependencies and tests the actual logic paths that the CLI commands use.

---

## Phase 1: Session Lifecycle Tests (`tests/session_lifecycle.rs`)

**Maps to**: User Story 1 (P1)

Tests the core CRUD flow that `aoe add`, `aoe list`, and `aoe remove` use internally.

| Test | What it verifies |
|------|-----------------|
| `test_create_session_persists` | `Instance::new()` + `Storage::save_with_groups()` writes to `sessions.json`; `load_with_groups()` returns it |
| `test_create_multiple_sessions` | Multiple instances persist and load correctly |
| `test_remove_session_by_id` | Remove an instance from the vec, save, reload, verify it's gone |
| `test_create_session_with_group` | Session with `group_path` set; `GroupTree::new_with_groups()` reflects it |
| `test_create_session_with_command` | Session with custom command; `detect_tool()` resolves the tool correctly |
| `test_session_backup_created` | After save, `sessions.json.bak` exists |
| `test_storage_handles_empty_profile` | `Storage::new("")` defaults to `"default"` profile |

**API calls**: `Storage::new()`, `Instance::new()`, `GroupTree::new_with_groups()`, `save_with_groups()`, `load_with_groups()`

---

## Phase 2: Profile Management Tests (`tests/profile_management.rs`)

**Maps to**: User Story 2 (P1)

Tests the profile lifecycle that `aoe profile create/delete/list/default` uses.

| Test | What it verifies |
|------|-----------------|
| `test_create_profile` | `create_profile("work")` creates the profile dir; `list_profiles()` includes it |
| `test_list_profiles_includes_default` | Even with no explicit create, `"default"` appears in `list_profiles()` |
| `test_delete_profile` | `delete_profile("work")` removes it from `list_profiles()` |
| `test_cannot_delete_default_profile` | `delete_profile("default")` returns an error |
| `test_set_default_profile` | `set_default_profile("work")` persists in global config; `load_config()` reflects it |
| `test_profile_session_isolation` | Sessions saved in profile A are not visible when loading profile B |
| `test_profile_config_isolation` | Config saved in profile A does not affect profile B's config |

**API calls**: `create_profile()`, `delete_profile()`, `list_profiles()`, `set_default_profile()`, `load_config()`, `save_config()`, `Storage::new(profile)`

---

## Phase 3: Group Persistence Tests (`tests/group_persistence.rs`)

**Maps to**: User Story 3 (P2)

Tests group CRUD with disk round-trip, beyond the existing unit tests for in-memory tree logic.

| Test | What it verifies |
|------|-----------------|
| `test_create_group_and_persist` | Create group, save, reload; group exists in loaded tree |
| `test_nested_group_persistence` | Create `"work/frontend"`, save, reload; parent and child groups present |
| `test_delete_group_persists` | Delete a group, save, reload; group is gone |
| `test_move_session_between_groups` | Change a session's `group_path`, rebuild tree, save, reload; session appears in new group |
| `test_group_with_sessions_round_trip` | Sessions assigned to groups; full save/load cycle preserves assignments |
| `test_empty_groups_persist` | Groups with no sessions still persist (they're explicitly created) |

**API calls**: `GroupTree::new_with_groups()`, `create_group()`, `delete_group()`, `get_all_groups()`, `save_with_groups()`, `load_with_groups()`

---

## Phase 4: Config Merge Tests (`tests/config_merge.rs`)

**Maps to**: User Story 4 (P2)

Tests the real TOML file pipeline: write global config, write profile override, load and merge, verify precedence.

| Test | What it verifies |
|------|-----------------|
| `test_merge_overrides_global` | Profile sets `sandbox.auto_cleanup = false`; merged result is `false` |
| `test_merge_inherits_unset_fields` | Profile only sets `theme`; all other fields come from global |
| `test_config_toml_round_trip` | `save_config()` then `load_config()` preserves all fields |
| `test_profile_config_toml_round_trip` | `save_profile_config()` then `load_profile_config()` preserves all override fields |
| `test_resolve_config_combines_both` | `resolve_config(profile)` loads both files and returns the merged result |
| `test_empty_profile_config_returns_global` | Profile with no overrides returns global config unchanged |

**API calls**: `save_config()`, `load_config()`, `save_profile_config()`, `load_profile_config()`, `merge_configs()`, `resolve_config()`

---

## Phase 5: Migration Pipeline Tests (`tests/migration_pipeline.rs`)

**Maps to**: User Story 5 (P2)

Tests the migration runner's behavior with the `.schema_version` file.

| Test | What it verifies |
|------|-----------------|
| `test_fresh_dir_runs_all_migrations` | No `.schema_version` file; after `run_migrations()`, version file is set to `CURRENT_VERSION` |
| `test_up_to_date_is_noop` | `.schema_version` already at current; `run_migrations()` does nothing |
| `test_idempotent_double_run` | Run migrations twice; second run is a no-op, no errors |
| `test_partial_version_runs_remaining` | `.schema_version` at version 0; only migrations > 0 execute |

**API calls**: `migrations::run_migrations()`, file I/O for `.schema_version`

**Note**: The migration tests need HOME set to a temp dir so `get_app_dir()` resolves there. On Linux, also set `XDG_CONFIG_HOME`. The v001 migration moves data from `~/.agent-of-empires` to `$XDG_CONFIG_HOME/agent-of-empires` on Linux, so Linux-specific tests should verify this path change.

---

## Phase 6: Hook Trust Lifecycle (extend `tests/repo_config.rs`)

**Maps to**: User Story 6 (P3)

The existing `repo_config.rs` already tests loading, trust/untrust, and hook execution. Add tests for the full lifecycle: trust, modify, detect invalidation.

| Test | What it verifies |
|------|-----------------|
| `test_hook_trust_invalidated_on_change` | Trust hooks, modify hook content, `check_hook_trust()` returns untrusted |
| `test_hook_re_trust_after_change` | After invalidation, re-trusting works and hooks execute again |

**API calls**: `load_repo_config()`, `check_hook_trust()`, `trust_hooks()`, `execute_hooks()`

---

## Complexity Tracking

No constitution violations. All tests follow existing patterns and require no new dependencies.

## Risk Assessment

| Risk | Mitigation |
|------|-----------|
| `env::set_var` is unsafe in Rust 2024+ | Already mitigated by `#[serial]`; same pattern as all existing tests |
| Migration tests may conflict with real app dir | Tests set HOME to temp dir; no real user state touched |
| Profile/config API may have internal side effects | Tests verify via file system state, not just return values |
| Tests may be flaky if file system ops are slow | Use `#[serial]` for all env-mutating tests; no timing dependencies |
