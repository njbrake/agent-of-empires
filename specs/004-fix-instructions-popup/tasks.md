# Tasks: Fix Custom Instructions Popup Editor

**Input**: Design documents from `/specs/004-fix-instructions-popup/`
**Prerequisites**: plan.md (required), spec.md (required), research.md, data-model.md, contracts/dialog-api.md

**Tests**: Included per constitution requirement (Principle II: "All features MUST have corresponding tests").

**Organization**: Tasks are grouped by user story to enable independent implementation and testing of each story.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3)
- Include exact file paths in descriptions

## Phase 1: Setup

**Purpose**: Register the new dialog module and prepare the module structure

- [X] T001 Add `mod custom_instruction;` declaration and `pub use custom_instruction::CustomInstructionDialog;` re-export in `src/tui/dialogs/mod.rs`

---

## Phase 2: Foundational (CustomInstructionDialog Core)

**Purpose**: Build the complete `CustomInstructionDialog` struct that all user stories depend on

**CRITICAL**: No user story work can begin until this phase is complete

- [X] T002 Create `CustomInstructionDialog` struct and `new()` constructor in `src/tui/dialogs/custom_instruction.rs`. Struct fields: `focused_zone: usize` (0=text area, 1=buttons), `focused_button: usize` (0=Save, 1=Cancel), `text_area: tui_textarea::TextArea<'static>`, `original_value: Option<String>`. Constructor takes `Option<String>`, initializes TextArea with the value (or empty), sets `focused_zone=0`, `focused_button=0`.
- [X] T003 Implement `handle_key(&mut self, key: KeyEvent) -> DialogResult<Option<String>>` in `src/tui/dialogs/custom_instruction.rs`. Key routing: when `focused_zone==0` (text area) -- Escape returns Cancel, Tab sets `focused_zone=1`, all other keys delegated to `self.text_area.input(key)`. When `focused_zone==1` (buttons) -- Escape returns Cancel, Tab sets `focused_zone=0`, Left/Right toggles `focused_button` between 0 and 1, Enter on Save (button 0) returns `Submit(text_or_none)`, Enter on Cancel (button 1) returns Cancel.
- [X] T004 Implement `render(&self, frame: &mut Frame, area: Rect, theme: &Theme)` in `src/tui/dialogs/custom_instruction.rs`. Use `centered_rect()` for dialog sizing (70% width, 60% height of area). Render `Clear` widget, bordered `Block` with title "Edit Custom Instruction". Layout: vertical split into text area region, button row (height 3), and hint bar (height 1). Render `TextArea` widget in text area region with focus-dependent border color. Render Save/Cancel buttons with highlight on focused button. Render hint line: "Tab: switch focus | Enter: edit/confirm | Esc: cancel".
- [X] T005 Add `pub(super) custom_instruction_dialog: Option<CustomInstructionDialog>` field to `SettingsView` struct in `src/tui/settings/mod.rs`, initialize to `None` in constructor.

**Checkpoint**: CustomInstructionDialog module compiles and is importable from settings. Run `cargo check`.

---

## Phase 3: User Story 1 - Edit Custom Instruction via Popup Dialog (Priority: P1) MVP

**Goal**: Replace inline single-line input with the popup dialog for editing custom instructions

**Independent Test**: Navigate to Settings > Sandbox > Custom Instruction, press Enter, type multi-line text in popup, Tab to Save, Enter to confirm, verify instruction saved.

### Implementation for User Story 1

- [X] T006 [US1] Intercept Enter key on `FieldKey::CustomInstruction` in `src/tui/settings/input.rs`. In the match arm where `FieldValue::OptionalText` currently creates `self.editing_input = Some(Input::new(...))`, add a check: if field key is `CustomInstruction`, instead create `self.custom_instruction_dialog = Some(CustomInstructionDialog::new(value.clone()))` and return early (skip inline edit mode).
- [X] T007 [US1] Add dialog key delegation at the top of `handle_key()` in `src/tui/settings/input.rs`. Before existing key handling: if `self.custom_instruction_dialog.is_some()`, delegate key to `dialog.handle_key(key)`. On `DialogResult::Submit(value)`: update the selected field's `FieldValue::OptionalText` with the returned value, call existing apply/persist logic, set `self.custom_instruction_dialog = None`. On `DialogResult::Cancel`: set `self.custom_instruction_dialog = None`. On `DialogResult::Continue`: return `SettingsAction::Continue`.
- [X] T008 [P] [US1] Add dialog overlay rendering in `src/tui/settings/render.rs`. At the end of the settings `render()` method: if `self.custom_instruction_dialog.is_some()`, call `dialog.render(frame, area, theme)` to overlay the dialog on top of the settings view.

**Checkpoint**: User Story 1 fully functional. Open popup, type multi-line text, Tab to Save, Enter confirms, value persists. Escape cancels without saving.

---

## Phase 4: User Story 2 - Clear Custom Instruction (Priority: P2)

**Goal**: Allow users to clear a custom instruction by saving empty text

**Independent Test**: Open popup with existing instruction, clear all text, Tab to Save, Enter, verify instruction removed (set to None).

### Implementation for User Story 2

- [X] T009 [US2] Verify and ensure empty-text-to-None conversion in `src/tui/settings/input.rs` and `src/tui/dialogs/custom_instruction.rs`. In the dialog's Submit handler: when text area content is empty or whitespace-only, return `Submit(None)` instead of `Submit(Some(""))`. In the settings Submit handler (T007): when received value is `None`, update field to `FieldValue::OptionalText(None)` which existing apply logic already handles as clearing the instruction.

**Checkpoint**: User Story 2 functional. Clearing text and saving removes the custom instruction from config.

---

## Phase 5: User Story 3 - Preview Instruction in Settings List (Priority: P3)

**Goal**: Show a truncated preview of the custom instruction in the settings field list

**Independent Test**: Set a long multi-line custom instruction, view settings list, verify truncated preview with ellipsis is shown.

### Implementation for User Story 3

- [X] T010 [US3] Improve custom instruction display in settings field list in `src/tui/settings/render.rs`. In the `OptionalText` rendering path for the field list: when the field key is `CustomInstruction` and the value is `Some(text)`, replace newline characters with spaces, truncate to fit the available width minus 3 characters, and append "..." if truncated. When value is `None`, display "(not set)" placeholder.

**Checkpoint**: All user stories independently functional. Settings list shows meaningful preview of custom instruction content.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Testing, linting, and final validation

- [X] T011 Add unit tests for `CustomInstructionDialog` in `src/tui/dialogs/custom_instruction.rs`. Test cases: (1) `new()` with `Some(text)` pre-populates TextArea, (2) `new()` with `None` starts empty, (3) Tab toggles `focused_zone` between 0 and 1, (4) Shift+Tab toggles in reverse, (5) Escape returns `Cancel` from zone 0, (6) Escape returns `Cancel` from zone 1, (7) Enter in zone 0 does not return Submit (continues), (8) Enter on Save button returns `Submit`, (9) Enter on Cancel button returns `Cancel`, (10) Left/Right in button row toggles `focused_button`, (11) Submit with empty text returns `Submit(None)`, (12) Submit with non-empty text returns `Submit(Some(text))`.
- [X] T012 Run `cargo fmt`, `cargo clippy` (zero warnings), and `cargo test` (all pass) per constitution requirements

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies - start immediately
- **Foundational (Phase 2)**: Depends on Phase 1 completion - BLOCKS all user stories
- **User Story 1 (Phase 3)**: Depends on Phase 2 completion
- **User Story 2 (Phase 4)**: Depends on Phase 3 (builds on US1's Submit handler)
- **User Story 3 (Phase 5)**: Can start after Phase 2 (independent of US1/US2)
- **Polish (Phase 6)**: Depends on all user stories being complete

### User Story Dependencies

- **User Story 1 (P1)**: Depends on Foundational (Phase 2). Core popup dialog editing.
- **User Story 2 (P2)**: Depends on User Story 1 (extends the Submit handler to handle empty text).
- **User Story 3 (P3)**: Depends on Foundational (Phase 2) only. Independent rendering change.

### Parallel Opportunities

- **T001** (setup): Standalone, start immediately
- **T002, T003, T004**: Sequential within same file (`custom_instruction.rs`)
- **T005**: Can start after T001 (needs type to exist)
- **T008** (render overlay) and **T006/T007** (input handling): [P] - different files
- **T010** (preview rendering) can run in parallel with **T006/T007** (both depend on Phase 2 only)

---

## Parallel Example: User Story 1

```bash
# After Phase 2 completes, launch these in parallel:
Task: T006/T007 "Wire dialog into settings input handling in src/tui/settings/input.rs"
Task: T008 "Render dialog overlay in src/tui/settings/render.rs"
```

## Parallel Example: User Story 1 + User Story 3

```bash
# After Phase 2 completes, US3 can run alongside US1:
Task: T006/T007/T008 "User Story 1 - popup dialog editing"
Task: T010 "User Story 3 - preview rendering in settings list"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup (T001)
2. Complete Phase 2: Foundational (T002-T005)
3. Complete Phase 3: User Story 1 (T006-T008)
4. **STOP and VALIDATE**: Test popup editing end-to-end
5. The core bug fix is now deployed

### Incremental Delivery

1. Setup + Foundational -> Dialog module ready
2. Add User Story 1 -> Popup editing works -> Core fix delivered (MVP!)
3. Add User Story 2 -> Empty text clears instruction -> Completes editing experience
4. Add User Story 3 -> Preview in settings list -> Polished UX
5. Polish -> Tests, linting, final validation

---

## Notes

- [P] tasks = different files, no dependencies
- [Story] label maps task to specific user story for traceability
- `tui-textarea 0.7` is already in `Cargo.toml` but currently unused - T002 is the first use
- US2 is lightweight (mostly verification of existing conversion logic) but included as separate phase for traceability
- Follow `RenameDialog` patterns in `src/tui/dialogs/rename.rs` as primary reference for dialog implementation
- Commit after each phase checkpoint
