# Contract: CustomInstructionDialog API

**Branch**: `004-fix-instructions-popup` | **Date**: 2026-02-12

This feature is a TUI-only change with no external API surface. The "contract" defines the internal dialog interface that integrates with the settings view.

## Dialog Interface

### Constructor
```
CustomInstructionDialog::new(current_value: Option<String>) -> Self
```
- Creates a new dialog pre-populated with `current_value` (or empty if None)
- Sets `focused_zone = 0` (text area focused by default)
- Sets `focused_button = 0` (Save button focused by default when button row gains focus)
- Stores `original_value` for cancel restoration

### Key Handler
```
CustomInstructionDialog::handle_key(key: KeyEvent) -> DialogResult<Option<String>>
```
- Returns `DialogResult::Continue` for most key events (text editing, focus changes)
- Returns `DialogResult::Cancel` on Escape from any zone
- Returns `DialogResult::Submit(Option<String>)` when Save is activated:
  - `Some(text)` if text is non-empty
  - `None` if text is empty (clears instruction)
- Returns `DialogResult::Cancel` when Cancel button is activated

### Renderer
```
CustomInstructionDialog::render(frame: &mut Frame, area: Rect, theme: &Theme)
```
- Renders centered overlay dialog using `centered_rect()`
- Clears background with `Clear` widget
- Draws bordered block with title "Edit Custom Instruction"
- Renders text area (zone 0) with `TextArea` widget
- Renders button row (zone 1) with Save/Cancel buttons
- Renders hint bar at bottom with key binding hints
- Visual focus indicators on the active zone

## Settings View Integration

### State Field
```
SettingsView.custom_instruction_dialog: Option<CustomInstructionDialog>
```

### Input Flow
1. When `editing_input` is None and user presses Enter on `FieldKey::CustomInstruction`:
   - Create `CustomInstructionDialog::new(current_value)`
   - Store in `self.custom_instruction_dialog`
   - Skip normal inline edit mode
2. When `custom_instruction_dialog.is_some()`:
   - Delegate all key events to `dialog.handle_key(key)`
   - On `Submit(value)`: update field value, close dialog
   - On `Cancel`: close dialog without updating

### Render Flow
1. Render normal settings view
2. If `custom_instruction_dialog.is_some()`: render dialog overlay on top
