# Tasks: Hooks at Global/Profile Level & Repo Settings in TUI

**Input**: Design documents from `/specs/002-hooks-settings-tui/`
**Prerequisites**: plan.md (required), spec.md (required for user stories), research.md, data-model.md, contracts/

**Tests**: Included where they validate core resolution logic, sandbox execution, and failure semantics.

**Organization**: Tasks are grouped by user story to enable independent implementation and testing of each story.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3)
- Include exact file paths in descriptions

---

## Phase 1: Setup

**Purpose**: Ensure the project compiles and existing tests pass before making changes.

- [ ] T001 Run `cargo check` and `cargo test` to confirm clean baseline before changes

**Checkpoint**: Baseline verified. All changes build on a known-good state.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Add HooksConfig to global and profile config structs. These changes are required before any user story can be implemented.

**CRITICAL**: No user story work can begin until this phase is complete.

- [ ] T002 Add `pub hooks: HooksConfig` field with `#[serde(default)]` to the `Config` struct in `src/session/config.rs`. Import `HooksConfig` from `repo_config`. Ensure `HooksConfig` derives `Default` (empty vecs) if it does not already.
- [ ] T003 Create `HooksConfigOverride` struct in `src/session/profile_config.rs` with `pub on_create: Option<Vec<String>>` and `pub on_launch: Option<Vec<String>>`. Derive `Default`, `Serialize`, `Deserialize`, `Clone`. Add `#[serde(default, skip_serializing_if = "Option::is_none")]` on each field.
- [ ] T004 Add `pub hooks: Option<HooksConfigOverride>` field to `ProfileConfig` in `src/session/profile_config.rs`. Add `#[serde(default, skip_serializing_if = "Option::is_none")]`.
- [ ] T005 Create `apply_hooks_overrides()` function in `src/session/profile_config.rs` following the existing pattern (e.g., `apply_worktree_overrides`). For each field (`on_create`, `on_launch`): if override is `Some`, replace the global value.
- [ ] T006 Add hooks merge call to `merge_configs()` in `src/session/profile_config.rs`: `if let Some(ref hooks_override) = profile.hooks { apply_hooks_overrides(&mut global.hooks, hooks_override); }`
- [ ] T007 Update `profile_has_overrides()` in `src/session/profile_config.rs` to include hooks check.
- [ ] T008 Update `merge_repo_config()` in `src/session/repo_config.rs` to apply per-field hook overrides from `RepoConfig.hooks` onto `Config.hooks`. If repo defines `on_create`, it replaces `config.hooks.on_create`. Same for `on_launch`. This integrates hooks into the existing global > profile > repo resolution chain.
- [ ] T009 Run `cargo check` and `cargo test` to verify foundational changes compile and existing tests pass.

**Checkpoint**: Foundation ready. Config structs support hooks at all three levels with per-field override resolution.

---

## Phase 3: User Story 1 - Configure Global Default Hooks (Priority: P1)

**Goal**: Global hooks execute for sessions when no repo-level hooks exist. Hooks follow the session's sandbox setting for execution location (FR-011). `on_create` failures abort creation; `on_launch` failures are non-fatal (FR-012). Duplicate `on_launch` execution is prevented (FR-013).

**Independent Test**: Create a session for a repo without `.aoe/config.toml` and verify global hooks run in the correct environment (local or container).

### Implementation for User Story 1

- [ ] T010 [US1] Update hook resolution in `src/tui/home/input.rs` to use hooks from the merged `Config` (global+profile) when repo-level hooks are absent. Currently hooks are only sourced from `check_hook_trust()` on the repo config. After this change: (1) Resolve the full config via `resolve_config()` for the active profile. (2) Call `check_hook_trust()` for repo hooks. (3) If repo has trusted hooks, merge them per-field onto the resolved config hooks. (4) If repo has no hooks (`NoHooks`), use global+profile hooks directly. (5) If repo hooks need trust and user skips, fall back to global+profile hooks. Global/profile hooks skip the trust dialog entirely (FR-004).
- [ ] T011 [US1] Update `src/tui/creation_poller.rs` to accept the resolved `HooksConfig` (which now may come from global+profile instead of only repo). No structural change should be needed - the `CreationRequest.hooks` field already carries the hooks, and the sandbox-aware execution paths (`execute_hooks_in_container_streamed` vs `execute_hooks_streamed`) already branch on `data.sandbox`. Verify the existing sandbox branching works correctly for global/profile-sourced hooks (FR-011).
- [ ] T012 [US1] Update `src/session/instance.rs` `start_with_size_opts()` to resolve hooks from the merged config (global+profile) when `check_hook_trust()` returns `NoHooks`. Currently on_launch hooks for existing sessions only come from repo config. After this change, if no repo hooks exist, use `Config.hooks.on_launch` from the resolved config. Ensure sandbox-aware execution applies (FR-011).
- [ ] T013 [US1] Verify that the existing `on_launch_hooks_ran` flag in `CreationResult::Success` and the skip logic in `src/tui/app.rs` `attach_session()` correctly prevents duplicate `on_launch` execution for global/profile-sourced hooks (FR-013). This should work without modification since hooks flow through the same `CreationRequest.hooks` path.
- [ ] T014 [P] [US1] Add unit test in `tests/hooks_config.rs`: global hooks resolve when no repo config exists. Create a global config with hooks via `tempfile`, verify `resolve_config()` returns the hooks.
- [ ] T015 [P] [US1] Add unit test in `tests/hooks_config.rs`: repo hooks override global hooks per-field. Set global `on_create=["global"]` and `on_launch=["global"]`, repo `on_create=["repo"]` only. Verify resolved `on_create=["repo"]` and `on_launch=["global"]`.
- [ ] T016 [P] [US1] Add unit test in `tests/hooks_config.rs`: verify that global/profile hooks are NOT subject to trust checking. Only repo hooks should go through `check_hook_trust()`.
- [ ] T017 [US1] Run `cargo fmt`, `cargo clippy`, and `cargo test` to validate US1.

**Checkpoint**: Global default hooks work. Sessions without `.aoe/config.toml` use global hooks. Hooks execute in the correct environment (sandbox or local). Failure semantics and duplicate prevention work correctly.

---

## Phase 4: User Story 2 - Configure Profile-Level Hook Overrides (Priority: P2)

**Goal**: Profile-level hooks override global hooks with per-field granularity.

**Independent Test**: Two profiles with different hooks produce different behavior.

### Implementation for User Story 2

- [ ] T018 [P] [US2] Add unit test in `tests/hooks_config.rs`: profile `on_create` override replaces global `on_create`, while `on_launch` falls back to global when not overridden in profile.
- [ ] T019 [P] [US2] Add unit test in `tests/hooks_config.rs`: clearing a profile hooks override (setting fields back to None) restores global hooks.
- [ ] T020 [P] [US2] Add unit test in `tests/hooks_config.rs`: full three-level resolution (global + profile + repo) with per-field semantics. Verify repo > profile > global for each field independently. Include case where repo explicitly sets empty list `on_launch=[]` to override a non-empty profile/global value.
- [ ] T021 [US2] Run `cargo fmt`, `cargo clippy`, and `cargo test` to validate US2.

**Checkpoint**: Profile hooks override global hooks per-field. Three-level resolution chain works.

---

## Phase 5: User Story 3 - Edit Hooks in the Settings TUI (Priority: P3)

**Goal**: Users can view and edit `on_create` and `on_launch` hooks in a dedicated "Hooks" tab in the settings TUI.

**Independent Test**: Open settings, navigate to Hooks tab, add a hook, save, verify config file updated.

### Implementation for User Story 3

- [ ] T022 [US3] Add `Hooks` variant to `SettingsCategory` enum in `src/tui/settings/fields.rs`. Add display name mapping.
- [ ] T023 [US3] Add `HookOnCreate` and `HookOnLaunch` variants to `FieldKey` enum in `src/tui/settings/fields.rs`.
- [ ] T024 [US3] Implement `build_hooks_fields()` function in `src/tui/settings/fields.rs`. Create two `SettingField` entries with `FieldValue::List` type. Use `resolve_value()` pattern for global/profile resolution. Labels: "On Create" and "On Launch". Descriptions: "Commands run once when a session is first created" and "Commands run every time a session starts". Note in description that hooks run inside the sandbox container when sandbox is enabled.
- [ ] T025 [US3] Add `SettingsCategory::Hooks` case to `build_fields_for_category()` dispatcher in `src/tui/settings/fields.rs`.
- [ ] T026 [US3] Add `FieldKey::HookOnCreate` and `FieldKey::HookOnLaunch` cases to `apply_field_to_global()` in `src/tui/settings/fields.rs`. Map `FieldValue::List` to `config.hooks.on_create` / `config.hooks.on_launch`.
- [ ] T027 [US3] Add `FieldKey::HookOnCreate` and `FieldKey::HookOnLaunch` cases to `apply_field_to_profile()` in `src/tui/settings/fields.rs`. Use `set_or_clear_override()` pattern with `config.hooks` section and `HooksConfigOverride` fields.
- [ ] T028 [US3] Add `FieldKey::HookOnCreate` and `FieldKey::HookOnLaunch` cases to `clear_profile_override()` in `src/tui/settings/input.rs`. Pattern: `if let Some(ref mut h) = self.profile_config.hooks { h.on_create = None; }`.
- [ ] T029 [US3] Add `SettingsCategory::Hooks` to the categories list in `SettingsView::new()` in `src/tui/settings/mod.rs`.
- [ ] T030 [US3] Run `cargo fmt`, `cargo clippy`, and `cargo test` to validate US3.

**Checkpoint**: Hooks tab appears in settings. Users can edit global/profile hooks via TUI.

---

## Phase 6: User Story 4 - Edit Repo-Level Settings in TUI (Priority: P4)

**Goal**: Users can view and edit `.aoe/config.toml` from a "Repo" tab in settings TUI.

**Independent Test**: Select session, open settings, edit repo hooks in Repo tab, save, verify `.aoe/config.toml` updated.

### Implementation for User Story 4

- [ ] T031 [US4] Add `Repo` variant to `SettingsCategory` enum in `src/tui/settings/fields.rs`. Add display name mapping.
- [ ] T032 [US4] Add `RepoHookOnCreate` and `RepoHookOnLaunch` variants to `FieldKey` enum in `src/tui/settings/fields.rs`.
- [ ] T033 [US4] Add `project_path: Option<String>` and `repo_config: Option<RepoConfig>` fields to `SettingsView` struct in `src/tui/settings/mod.rs`.
- [ ] T034 [US4] Update `SettingsView::new()` in `src/tui/settings/mod.rs` to accept `project_path: Option<String>` parameter. If Some, load `RepoConfig` from `.aoe/config.toml` at that path (use `load_repo_config()`). Add `SettingsCategory::Repo` to categories list.
- [ ] T035 [US4] Implement `build_repo_fields()` function in `src/tui/settings/fields.rs`. Create `SettingField` entries for repo-level `on_create` and `on_launch` using `FieldValue::List`. Source values from `RepoConfig.hooks` (or empty defaults). These fields do NOT use the global/profile scope toggle.
- [ ] T036 [US4] Add `SettingsCategory::Repo` case to `build_fields_for_category()` dispatcher in `src/tui/settings/fields.rs`. When `repo_config` is None (no project path), return empty fields.
- [ ] T037 [US4] Add `FieldKey::RepoHookOnCreate` and `FieldKey::RepoHookOnLaunch` cases to field apply logic. These should update `self.repo_config` rather than global/profile config. Add a dedicated `apply_field_to_repo()` function or handle in the existing dispatcher.
- [ ] T038 [US4] Update `save()` in `src/tui/settings/mod.rs` to also save `repo_config` changes. If `repo_config` has changes, serialize to `.aoe/config.toml` at `project_path`. Create `.aoe/` directory if it does not exist. Implement `save_repo_config()` function in `src/session/repo_config.rs` if one does not already exist.
- [ ] T039 [US4] Handle Repo tab disabled state in `src/tui/settings/render.rs`. When `project_path` is None, render a placeholder message in the fields area (e.g., "No session selected - select a session to edit repo settings"). Prevent field editing when disabled.
- [ ] T040 [US4] Update the call site that opens SettingsView (in `src/tui/home/input.rs` or wherever settings are opened) to pass the currently selected session's `project_path` to `SettingsView::new()`.
- [ ] T041 [US4] Handle scope toggle behavior: when Repo tab is selected, hide or disable the Global/Profile scope tabs since repo settings are scope-independent. Restore normal scope toggle when switching back to other tabs.
- [ ] T042 [US4] Run `cargo fmt`, `cargo clippy`, and `cargo test` to validate US4.

**Checkpoint**: Repo tab works. Users can edit `.aoe/config.toml` from the TUI.

---

## Phase 7: Polish & Cross-Cutting Concerns

**Purpose**: Final validation and cleanup across all user stories.

- [ ] T043 Run full `cargo fmt`, `cargo clippy`, and `cargo test` suite.
- [ ] T044 Manual verification: follow `quickstart.md` scenarios end-to-end (global hooks, profile override, repo tab, override hierarchy, disabled state).
- [ ] T045 Manual verification: sandbox execution scenarios from `quickstart.md` - verify hooks run in container for sandboxed sessions and locally for non-sandboxed sessions.
- [ ] T046 Manual verification: failure semantics from `quickstart.md` - verify `on_create` failure aborts creation and `on_launch` failure allows session to start.
- [ ] T047 Manual verification: duplicate `on_launch` prevention from `quickstart.md` - verify hooks do not run twice when attaching to a newly created session.
- [ ] T048 Verify no regressions in existing repo hook behavior (trust system, Docker execution, streamed output).

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 1 (Setup)**: No dependencies - start immediately
- **Phase 2 (Foundational)**: Depends on Phase 1 - BLOCKS all user stories
- **Phase 3 (US1)**: Depends on Phase 2
- **Phase 4 (US2)**: Depends on Phase 2 (can run in parallel with US1 if desired, but sequential is recommended since US2 tests build on US1 resolution logic)
- **Phase 5 (US3)**: Depends on Phase 2 (US1/US2 config changes must exist for TUI fields)
- **Phase 6 (US4)**: Depends on Phase 5 (reuses SettingsCategory patterns from US3)
- **Phase 7 (Polish)**: Depends on all user stories

### Within Each User Story

- Config struct changes before TUI wiring
- Hook resolution logic before execution integration
- TUI field definitions before apply/save logic
- Apply/save logic before clear-override logic
- All implementation before validation

### Parallel Opportunities

- T002, T003 can run in parallel (different files: config.rs vs profile_config.rs)
- T014, T015, T016 can run in parallel (independent test cases in same file)
- T018, T019, T020 can run in parallel (independent test cases)
- T031, T032, T033 involve different files and can partially overlap

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational (config struct changes)
3. Complete Phase 3: User Story 1 (global hooks execute with sandbox awareness)
4. **STOP and VALIDATE**: Test with `cargo test` and manual verification
5. Global hooks work in both sandboxed and non-sandboxed sessions

### Incremental Delivery

1. Setup + Foundational -> Config structs ready
2. US1: Global hooks resolve, execute in correct environment, respect failure semantics -> MVP
3. US2: Profile overrides work -> Three-level resolution
4. US3: Hooks tab in settings TUI -> GUI editing
5. US4: Repo tab in settings TUI -> Full feature
6. Each story adds value without breaking previous stories

---

## Notes

- [P] tasks = different files, no dependencies
- [Story] label maps task to specific user story for traceability
- Each user story should be independently completable and testable
- Commit after each task or logical group
- Stop at any checkpoint to validate story independently
- FR-011 (sandbox execution) is primarily validated in US1 since the existing execution code paths already handle sandbox branching
- FR-012 (failure semantics) and FR-013 (duplicate prevention) leverage existing mechanisms and are validated via tests and manual verification
