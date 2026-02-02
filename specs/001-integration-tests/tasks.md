# Tasks: Integration Test Coverage

**Input**: Design documents from `/specs/001-integration-tests/`
**Prerequisites**: plan.md (required), spec.md (required for user stories)

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3)

---

## Phase 1: User Story 1 - CLI Session Lifecycle (Priority: P1)

**Goal**: Verify the core session CRUD flow (create, persist, load, remove) works end-to-end through the library API with real file I/O.

**Independent Test**: Run `cargo test --test session_lifecycle` -- all tests pass in isolation.

- [ ] T001 [US1] Create `tests/session_lifecycle.rs` with `setup_temp_home()` helper and `use serial_test::serial`
- [ ] T002 [US1] Implement `test_create_session_persists`: create an `Instance`, save via `Storage::save_with_groups()`, reload via `load_with_groups()`, assert title/path match
- [ ] T003 [US1] Implement `test_create_multiple_sessions`: save 3 instances, reload, assert all 3 present with correct data
- [ ] T004 [US1] Implement `test_remove_session_by_id`: create 2 sessions, remove one by filtering on `id`, save, reload, assert only 1 remains
- [ ] T005 [US1] Implement `test_create_session_with_group`: create instance with `group_path = "work"`, build `GroupTree`, save, reload, assert group exists and session is in it
- [ ] T006 [US1] Implement `test_session_backup_created`: save sessions, assert `sessions.json.bak` file exists on disk
- [ ] T007 [US1] Implement `test_storage_defaults_to_default_profile`: `Storage::new("")` resolves to the `"default"` profile directory

**Checkpoint**: `cargo test --test session_lifecycle` passes. Core session persistence is verified.

---

## Phase 2: User Story 2 - Profile Management Lifecycle (Priority: P1)

**Goal**: Verify profile create/delete/list/default-switch works with real directories and that sessions are isolated between profiles.

**Independent Test**: Run `cargo test --test profile_management` -- all tests pass in isolation.

- [ ] T008 [P] [US2] Create `tests/profile_management.rs` with `setup_temp_home()` helper and `use serial_test::serial`
- [ ] T009 [US2] Implement `test_create_profile`: call `create_profile("work")`, assert `list_profiles()` contains `"work"`
- [ ] T010 [US2] Implement `test_list_profiles_includes_default`: on a fresh home dir, call `list_profiles()`, assert `"default"` is present
- [ ] T011 [US2] Implement `test_delete_profile`: create then delete `"work"`, assert `list_profiles()` no longer contains it
- [ ] T012 [US2] Implement `test_cannot_delete_default_profile`: call `delete_profile("default")`, assert it returns an error
- [ ] T013 [US2] Implement `test_set_default_profile`: create `"work"`, call `set_default_profile("work")`, load global config, assert `default_profile == "work"`
- [ ] T014 [US2] Implement `test_profile_session_isolation`: save a session in profile `"a"`, load storage for profile `"b"`, assert profile `"b"` has 0 sessions
- [ ] T015 [US2] Implement `test_profile_config_isolation`: save config in profile `"a"` with a distinct value, load config for profile `"b"`, assert it has the default value

**Checkpoint**: `cargo test --test profile_management` passes. Profile isolation is verified.

---

## Phase 3: User Story 3 - Group Management Lifecycle (Priority: P2)

**Goal**: Verify group create/delete/move operations persist correctly across save/load cycles.

**Independent Test**: Run `cargo test --test group_persistence` -- all tests pass in isolation.

- [ ] T016 [P] [US3] Create `tests/group_persistence.rs` with `setup_temp_home()` helper and `use serial_test::serial`
- [ ] T017 [US3] Implement `test_create_group_and_persist`: create a group via `GroupTree::create_group()`, save with `save_with_groups()`, reload, assert `group_exists()` returns true
- [ ] T018 [US3] Implement `test_nested_group_persistence`: create `"work/frontend"`, save, reload, assert both `"work"` and `"work/frontend"` exist
- [ ] T019 [US3] Implement `test_delete_group_persists`: create group, save, delete group, save again, reload, assert group is gone
- [ ] T020 [US3] Implement `test_move_session_between_groups`: create session in group `"a"`, change `group_path` to `"b"`, rebuild tree, save, reload, assert session is in group `"b"`
- [ ] T021 [US3] Implement `test_group_with_sessions_round_trip`: create 3 sessions across 2 groups, save, reload, assert each session is in its correct group
- [ ] T022 [US3] Implement `test_empty_groups_persist`: create group with no sessions via `create_group()`, save, reload, assert group still exists

**Checkpoint**: `cargo test --test group_persistence` passes. Group persistence is verified.

---

## Phase 4: User Story 4 - Profile Config Merge Pipeline (Priority: P2)

**Goal**: Verify that global config + profile overrides merge correctly with real TOML files on disk.

**Independent Test**: Run `cargo test --test config_merge` -- all tests pass in isolation.

- [ ] T023 [P] [US4] Create `tests/config_merge.rs` with `setup_temp_home()` helper and `use serial_test::serial`
- [ ] T024 [US4] Implement `test_merge_overrides_global`: save global config with `sandbox.auto_cleanup = true`, save profile override with `sandbox.auto_cleanup = false`, call `merge_configs()`, assert result is `false`
- [ ] T025 [US4] Implement `test_merge_inherits_unset_fields`: save profile override with only `theme` set, merge with global, assert all other fields match global defaults
- [ ] T026 [US4] Implement `test_config_toml_round_trip`: create a `Config` with non-default values, `save_config()`, `load_config()`, assert all fields preserved
- [ ] T027 [US4] Implement `test_profile_config_toml_round_trip`: create a `ProfileConfig` with several overrides, `save_profile_config()`, `load_profile_config()`, assert all override fields preserved
- [ ] T028 [US4] Implement `test_empty_profile_config_returns_global`: load profile config for a profile with no override file, merge with global, assert result equals global

**Checkpoint**: `cargo test --test config_merge` passes. Config merge precedence is verified.

---

## Phase 5: User Story 5 - Migration Pipeline (Priority: P2)

**Goal**: Verify that migrations run in order, update `.schema_version`, and are idempotent.

**Independent Test**: Run `cargo test --test migration_pipeline` -- all tests pass in isolation.

- [ ] T029 [P] [US5] Create `tests/migration_pipeline.rs` with `setup_temp_home()` helper and `use serial_test::serial`
- [ ] T030 [US5] Implement `test_fresh_dir_runs_all_migrations`: set up temp home with no `.schema_version`, call `run_migrations()`, assert `.schema_version` file contains `CURRENT_VERSION`
- [ ] T031 [US5] Implement `test_up_to_date_is_noop`: write `CURRENT_VERSION` to `.schema_version`, call `run_migrations()`, assert no errors and version unchanged
- [ ] T032 [US5] Implement `test_idempotent_double_run`: call `run_migrations()` twice on a fresh dir, assert no errors on second run and version file correct
- [ ] T033 [US5] Implement `test_partial_version_runs_remaining`: write version `0` to `.schema_version`, call `run_migrations()`, assert version bumps to `CURRENT_VERSION`

**Checkpoint**: `cargo test --test migration_pipeline` passes. Migration safety is verified.

---

## Phase 6: User Story 6 - Hook Trust Lifecycle (Priority: P3)

**Goal**: Extend existing `tests/repo_config.rs` with tests verifying that modifying hooks invalidates trust and re-trusting works.

**Independent Test**: Run `cargo test --test repo_config` -- all tests (old and new) pass.

- [ ] T034 [US6] Implement `test_hook_trust_invalidated_on_change` in `tests/repo_config.rs`: trust hooks, modify `.aoe/config.toml` hook content, call `check_hook_trust()`, assert untrusted
- [ ] T035 [US6] Implement `test_hook_re_trust_after_change` in `tests/repo_config.rs`: after invalidation, re-trust, assert `check_hook_trust()` returns trusted and `execute_hooks()` succeeds

**Checkpoint**: `cargo test --test repo_config` passes. Full hook trust lifecycle verified.

---

## Phase 7: Validation

**Purpose**: Verify all tests pass together and CI quality gates are met.

- [ ] T036 Run `cargo fmt --all -- --check` and fix any formatting issues
- [ ] T037 Run `cargo clippy -- -D warnings` and fix any lint warnings
- [ ] T038 Run `cargo test` (full suite) and verify all new + existing tests pass
- [ ] T039 Verify no test uses real user HOME (grep for hardcoded paths)

**Checkpoint**: All quality gates pass. Ready for PR.

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phases 1-6**: All independent of each other (different files, no shared state). Can run in parallel.
- **Phase 7**: Depends on all previous phases being complete.

### Within Each Phase

Tasks are sequential within a phase (each test builds on the file created by the first task).

### Parallel Opportunities

All 6 test files can be written in parallel since they:
- Live in separate files (`tests/session_lifecycle.rs`, `tests/profile_management.rs`, etc.)
- Use independent temp directories
- Have no shared mutable state (each test creates its own `TempDir`)

### Recommended Execution Order (sequential)

P1 stories first, then P2, then P3:

1. Phase 1 (US1: session lifecycle) -- most foundational
2. Phase 2 (US2: profiles) -- builds confidence in isolation
3. Phase 3 (US3: groups) -- depends on understanding storage patterns from Phase 1
4. Phase 4 (US4: config merge) -- independent but benefits from Phase 2 profile knowledge
5. Phase 5 (US5: migrations) -- independent
6. Phase 6 (US6: hooks) -- extends existing file, lowest priority
7. Phase 7 (validation) -- final gate
