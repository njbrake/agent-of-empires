# Tasks: Fix File Picker Tab/Enter Behavior

**Input**: Design documents from `/specs/003-fix-file-picker/`
**Prerequisites**: plan.md (required), spec.md (required for user stories), research.md, quickstart.md

**Tests**: Test updates are included as they are part of the bug fix (updating incorrect test assertions and adding coverage for new behavior).

**Organization**: Tasks are grouped by user story to enable independent implementation and testing of each story.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3)
- Include exact file paths in descriptions

## Path Conventions

- **Single project**: `src/`, `tests/` at repository root
- All changes in this feature are within `src/tui/components/dir_picker.rs`

---

## Phase 1: Setup

**Purpose**: No setup needed. This is a bug fix in an existing file with no new dependencies, files, or infrastructure.

(No tasks)

---

## Phase 2: Foundational

**Purpose**: No foundational/blocking work needed. All changes are independent modifications within a single match expression in `dir_picker.rs`.

(No tasks)

---

## Phase 3: User Story 1 - Tab Autocompletes Filter into Directory Navigation (Priority: P1) MVP

**Goal**: When a single directory matches the filter, Tab navigates into it regardless of highlight position. When multiple directories match, Tab navigates into the highlighted one from the filtered list. When none match, Tab is a no-op.

**Independent Test**: Open the file picker, type a partial directory name that uniquely matches one directory, press Tab, verify the picker navigates into the matching directory with the path updated in the title bar.

### Implementation for User Story 1

- [x] T001 [US1] Replace Tab key handler (lines 151-168) to check filtered match count: single match uses `filtered[0]`, multiple matches uses `filtered[self.selected.min(filtered_len - 1)]`, zero matches returns early in `src/tui/components/dir_picker.rs`
- [x] T002 [US1] Add `test_tab_single_match_autocompletes` test: setup tempdir with ["alpha", "beta", "gamma"], type "al", press Down, press Tab, assert cwd is "alpha" subdirectory in `src/tui/components/dir_picker.rs`
- [x] T003 [US1] Add `test_tab_no_match_does_nothing` test: setup tempdir, type "zzz", press Tab, assert cwd unchanged in `src/tui/components/dir_picker.rs`

**Checkpoint**: Tab autocomplete works correctly. Existing Tab tests (`test_tab_navigates_into_directory`, `test_tab_on_parent_goes_up`) still pass.

---

## Phase 4: User Story 2 - Enter Confirms Selection in All States (Priority: P2)

**Goal**: Enter always closes the picker with a meaningful selection. When the filtered list is empty, Enter selects the current working directory instead of silently doing nothing.

**Independent Test**: Open the file picker, type a filter that matches nothing, press Enter, verify the picker closes and returns the current working directory.

### Implementation for User Story 2

- [x] T004 [US2] Remove the `if filtered_len == 0 { return DirPickerResult::Continue; }` guard (lines 132-134) from the Enter handler in `src/tui/components/dir_picker.rs`
- [x] T005 [US2] Update `test_enter_on_empty_filtered_list_does_nothing` (line 672): rename to `test_enter_on_empty_filtered_list_selects_cwd`, assert `Selected(cwd)` and `!is_active()` in `src/tui/components/dir_picker.rs`

**Checkpoint**: Enter always closes the picker. Existing Enter tests (`test_enter_selects_highlighted_item`, `test_enter_selects_highlighted_subdir`) still pass.

---

## Phase 5: User Story 3 - Clearer Hint Bar Labels (Priority: P3)

**Goal**: The hint bar label for Tab communicates "navigate into directory" rather than using shell jargon "cd".

**Independent Test**: Open the file picker and verify the hint bar shows "Tab enter dir" instead of "Tab cd".

### Implementation for User Story 3

- [x] T006 [US3] Change hint bar Tab label from `Span::raw(" cd  ")` to `Span::raw(" enter dir  ")` on line 346 in `src/tui/components/dir_picker.rs`

**Checkpoint**: Hint bar displays updated labels.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Final validation across all changes.

- [x] T007 Run `cargo fmt` to ensure formatting compliance
- [x] T008 Run `cargo clippy` to ensure no lint warnings
- [x] T009 Run `cargo test` to verify all tests pass (including existing tests for regressions)

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: N/A
- **Foundational (Phase 2)**: N/A
- **User Story 1 (Phase 3)**: No dependencies, can start immediately
- **User Story 2 (Phase 4)**: No dependencies on US1, can start immediately
- **User Story 3 (Phase 5)**: No dependencies on US1 or US2, can start immediately
- **Polish (Phase 6)**: Depends on all user stories being complete

### User Story Dependencies

- **User Story 1 (P1)**: Independent. Modifies the `KeyCode::Tab` match arm.
- **User Story 2 (P2)**: Independent. Modifies the `KeyCode::Enter` match arm.
- **User Story 3 (P3)**: Independent. Modifies the hint bar rendering.

All three stories modify different sections of the same file, so they can be implemented sequentially without conflicts.

### Within Each User Story

- Implementation before test updates (for US1 and US2)
- US3 has no test changes

### Parallel Opportunities

- US1, US2, and US3 modify non-overlapping sections of `dir_picker.rs`, but since they're in the same file, parallel execution is not recommended. Sequential execution in priority order (P1 -> P2 -> P3) is the recommended approach.

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 3: User Story 1 (Tab autocomplete fix)
2. **STOP and VALIDATE**: Run `cargo test`, verify Tab behavior manually
3. This alone resolves the primary user complaint

### Incremental Delivery

1. US1 (Tab fix) -> Test independently -> Core complaint resolved
2. US2 (Enter fix) -> Test independently -> No more silent no-ops
3. US3 (Hint bar) -> Test independently -> Clearer UX
4. Polish -> `cargo fmt` + `cargo clippy` + full `cargo test`

---

## Notes

- All 9 tasks modify a single file: `src/tui/components/dir_picker.rs`
- No new files, modules, or dependencies needed
- 31 existing tests in `dir_picker.rs` serve as regression protection
- Commit after each user story checkpoint for clean git history
