# Feature Specification: Fix Custom Instructions Popup Editor

**Feature Branch**: `004-fix-instructions-popup`
**Created**: 2026-02-12
**Status**: Draft
**Input**: User description: "Please fix the bugs with adding custom sandbox instructions in the TUI: I think it should be a popup instead of an inline single line input..."

## Clarifications

### Session 2026-02-12

- Q: What keybinding should confirm/save the popup editor, given Enter conflicts with newline insertion in multi-line text? → A: Use a focus-based two-zone design (text area + button row) with Tab to switch focus, so Enter is always correct -- newline when editing text, confirm when focused on the Save button. Follows the existing RenameDialog Tab/Enter/Esc pattern.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Edit Custom Instruction via Popup Dialog (Priority: P1)

A user navigates to Sandbox settings in the TUI and selects the "Custom Instruction" field. Instead of an inline single-line text input that is difficult to use for multi-line or lengthy instructions, a dedicated popup dialog appears. The popup has two focus zones: a multi-line text area for composing the instruction, and a button row (Save / Cancel) at the bottom. The user uses Tab to switch focus between the text area and the buttons. Enter inserts newlines when the text area is focused, and activates the selected button when the button row is focused. Escape always cancels.

**Why this priority**: This is the core fix. The current inline single-line input is the primary usability problem. Users cannot see their full instruction text, cannot easily write multi-line prompts, and the inline editor is error-prone for longer content.

**Independent Test**: Can be fully tested by navigating to Settings > Sandbox > Custom Instruction, pressing Enter, typing multi-line text in the popup, pressing Tab to move to the Save button, pressing Enter to confirm, and verifying the instruction is saved correctly.

**Acceptance Scenarios**:

1. **Given** the user is in Settings view on the Sandbox section, **When** they select the "Custom Instruction" field and press Enter, **Then** a centered popup dialog appears with a multi-line text editor (focused by default) pre-populated with the current instruction value (or empty if none set), and a Save/Cancel button row at the bottom.
2. **Given** the popup is open with focus on the text area, **When** the user presses Enter, **Then** a newline is inserted into the text (Enter does not confirm the dialog).
3. **Given** the popup is open, **When** the user presses Tab, **Then** focus moves from the text area to the button row (or vice versa).
4. **Given** the popup is open with focus on the Save button, **When** the user presses Enter, **Then** the popup closes and the field value is updated to the entered text.
5. **Given** the popup is open with text entered, **When** the user presses Escape (from either focus zone), **Then** the popup closes and the original field value is preserved unchanged.
6. **Given** the popup is open with focus on the text area, **When** the user types text including newlines, **Then** the text area properly handles multi-line input with visible line wrapping.

---

### User Story 2 - Clear Custom Instruction (Priority: P2)

A user wants to remove a previously set custom instruction. From the popup dialog, the user can clear all text, Tab to the Save button, and confirm, which removes the instruction entirely (sets it to None/empty).

**Why this priority**: Users need a clear way to remove instructions they no longer want. This is a natural complement to editing.

**Independent Test**: Can be tested by opening the custom instruction popup, clearing all text, pressing Tab to Save, pressing Enter, and verifying the instruction is removed from the config.

**Acceptance Scenarios**:

1. **Given** a custom instruction is currently set, **When** the user opens the popup, clears all text, and confirms via the Save button, **Then** the custom instruction is removed (set to None).
2. **Given** no custom instruction is set, **When** the user opens the popup and confirms with empty text via the Save button, **Then** the field remains unset.

---

### User Story 3 - Preview Instruction in Settings List (Priority: P3)

When viewing the settings list, the custom instruction field shows a truncated preview of the current instruction text so users can quickly see whether an instruction is configured without opening the popup.

**Why this priority**: Improves discoverability and quick-glance understanding of current configuration state.

**Independent Test**: Can be tested by setting a long custom instruction and verifying the settings list shows a truncated preview with an indicator that more text exists.

**Acceptance Scenarios**:

1. **Given** a custom instruction longer than one line is set, **When** the user views the settings list, **Then** the field displays the first portion of the text followed by an ellipsis indicator.
2. **Given** no custom instruction is set, **When** the user views the settings list, **Then** the field displays an appropriate empty/unset indicator (e.g., "None" or empty).

---

### Edge Cases

- What happens when the user pastes extremely long text (thousands of characters) into the popup? The popup should accept the text and handle scrolling within the text area. No artificial character limit is imposed by the UI.
- What happens when the popup is opened while a profile override exists for the custom instruction? The popup should display the effective (merged) value and save appropriately to the correct config layer (global or profile override).
- What happens if the terminal window is very small when the popup opens? The popup should scale down gracefully, maintaining at minimum a usable text area size.
- What happens if the user presses Enter while the text area is focused? Enter inserts a newline. The user must Tab to the button row and press Enter on Save to confirm.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST display a centered popup dialog when the user activates the Custom Instruction field in settings, replacing the current inline single-line text input.
- **FR-002**: The popup MUST provide a multi-line text editing area that supports typing, deleting, and navigating within the text.
- **FR-003**: The popup MUST pre-populate with the current custom instruction value when opened, or be empty if no instruction is set. Focus MUST default to the text area.
- **FR-004**: The popup MUST have two focus zones: a text area and a button row (Save / Cancel). Tab switches focus between zones. Enter inserts a newline when the text area is focused, and activates the selected button when the button row is focused.
- **FR-005**: The popup MUST support canceling (discarding changes) via Escape key from either focus zone, restoring the original value.
- **FR-006**: The popup MUST display visible key binding hints (e.g., "Tab: switch focus | Enter: edit/confirm | Esc: cancel") so users know how to interact with it.
- **FR-007**: System MUST correctly persist the edited instruction to the appropriate config layer (global config or profile override) consistent with existing settings behavior.
- **FR-008**: System MUST handle empty/cleared text as removing the custom instruction (setting to None).
- **FR-009**: The popup MUST follow the existing dialog visual style (centered overlay, border, title, themed colors) consistent with other dialogs in the application.
- **FR-010**: The settings list MUST show a truncated preview of the custom instruction value for quick-glance identification.

### Key Entities

- **Custom Instruction**: A user-provided text string appended to an AI agent's system prompt in sandboxed sessions. Can be set globally, per-profile, or per-session. Stored as an optional string in the configuration.
- **Popup Dialog**: A modal overlay in the TUI with two focus zones (text area + button row) that captures user input independently from the underlying settings view, following the existing dialog pattern used by rename, new session, and confirmation dialogs.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Users can compose and edit multi-line custom instructions without character or line limitations imposed by the UI.
- **SC-002**: The popup editor opens and closes without visual glitches or leftover artifacts on the underlying settings view.
- **SC-003**: All existing custom instruction functionality (saving to global config, profile overrides, per-session injection into supported agents) continues to work correctly after the UI change.
- **SC-004**: Users can complete the full edit-save cycle (open popup, type instruction, Tab to Save, Enter to confirm, verify in settings) in a smooth, uninterrupted flow.

## Assumptions

- The existing dialog system provides sufficient patterns and utilities (centered rect, clear overlay, themed rendering) to implement the new popup without introducing new infrastructure.
- A multi-line text editing widget is available or can be integrated for the text area within the popup.
- The Custom Instruction field type in the settings view will change from inline-editable to popup-activated, meaning pressing Enter on the field opens the popup rather than entering inline edit mode.
- No changes to the underlying data model are needed; only the TUI editing experience changes.
- The focus-based two-zone design (text area + button row with Tab switching) follows the established RenameDialog pattern and requires no new interaction paradigms.
