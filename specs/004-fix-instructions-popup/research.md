# Research: Fix Custom Instructions Popup Editor

**Branch**: `004-fix-instructions-popup` | **Date**: 2026-02-12

## R1: Multi-line Text Editing Widget

**Decision**: Use `tui-textarea` (`TextArea`) for the multi-line text editing area inside the popup dialog.

**Rationale**: `tui-textarea 0.7` is already declared in `Cargo.toml` but currently unused in the codebase. It provides built-in multi-line editing with scrolling, cursor movement, word wrapping, and copy/paste support. This avoids reinventing multi-line editing on top of `tui_input::Input` (which is single-line only).

**Alternatives considered**:
- `tui_input::Input` with manual newline handling: Would require significant custom code for cursor positioning, line wrapping, scrolling, and selection. Not practical.
- Custom widget from scratch using raw ratatui: Excessive engineering for a well-solved problem.

## R2: Dialog Architecture Pattern

**Decision**: Create a new `CustomInstructionDialog` in `src/tui/dialogs/custom_instruction.rs` following the `RenameDialog` pattern.

**Rationale**: `RenameDialog` (`src/tui/dialogs/rename.rs`) is the closest existing pattern. It already demonstrates:
- Multi-field focus zones with `usize focused_field` and Tab/Shift+Tab cycling
- `DialogResult<T>` return type (`Continue | Cancel | Submit(T)`)
- Centered rendering via `centered_rect()` from `src/tui/dialogs/mod.rs`
- Themed styling with border, title, and `Clear` overlay
- Comprehensive test patterns

The new dialog adapts this pattern with two focus zones: text area (zone 0, using `TextArea`) and button row (zone 1, with Save/Cancel buttons).

**Alternatives considered**:
- Inline expansion within settings view: Would break the established modal dialog pattern and add complexity to the settings rendering pipeline.
- Reuse `RenameDialog` with modifications: Too many structural differences (multi-line vs. single-line, button row vs. field-based confirm).

## R3: Settings Integration Point

**Decision**: Add `custom_instruction_dialog: Option<CustomInstructionDialog>` to `SettingsView` struct and intercept `FieldKey::CustomInstruction` + Enter to launch the popup instead of entering inline edit mode.

**Rationale**: The settings view already manages modal state via `editing_input: Option<Input>` and `list_edit_state: Option<ListEditState>`. Adding a dialog state field follows the same pattern. When the dialog is active, key events are delegated to the dialog's `handle_key()` method, and the result is processed to update the field value.

**Alternatives considered**:
- Manage dialog at the `App` level (like HomeView dialogs): Would require threading the custom instruction value through additional layers. Settings-local state is simpler and more cohesive.

## R4: Field Value Display in Settings List

**Decision**: Change `OptionalText` rendering for `CustomInstruction` to show a truncated first-line preview with ellipsis when text is set, and "(not set)" when empty.

**Rationale**: The current `render_text_field()` already truncates to 50 characters. For multi-line custom instructions, we should additionally replace newlines with spaces (or show only the first line) for the list preview, and append "..." if truncated. This requires minimal changes to the rendering path.

**Alternatives considered**:
- Multi-line preview in list: Would break the consistent 3-line field height in the settings list.
- Show character count instead of preview: Less informative; users want to see what the instruction says.

## R5: Focus Zone Behavior in Button Row

**Decision**: The button row has two buttons (Save / Cancel) with Left/Right arrow keys to switch between them, and Enter to activate the focused button. Save is focused by default when Tab moves to the button row.

**Rationale**: This mirrors standard dialog button behavior. Left/Right navigation within the button row is intuitive and consistent with the `ConfirmDialog` pattern. Having Save focused by default optimizes the common case (Tab then Enter to save).

**Alternatives considered**:
- Single "Save" button with Escape for cancel: Would work but having explicit Cancel button provides discoverability and is consistent with the two-action dialog pattern.
