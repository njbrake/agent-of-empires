# Quickstart: Fix Custom Instructions Popup Editor

**Branch**: `004-fix-instructions-popup` | **Date**: 2026-02-12

## Overview

Replace the inline single-line text input for custom sandbox instructions in the TUI settings with a popup dialog featuring a multi-line text editor and Save/Cancel buttons.

## Key Files to Modify

| File | Change |
|------|--------|
| `src/tui/dialogs/mod.rs` | Add `mod custom_instruction;` and re-export |
| `src/tui/dialogs/custom_instruction.rs` | **New file**: CustomInstructionDialog implementation |
| `src/tui/settings/mod.rs` | Add `custom_instruction_dialog: Option<CustomInstructionDialog>` state |
| `src/tui/settings/input.rs` | Intercept Enter on CustomInstruction field to launch popup; handle dialog results |
| `src/tui/settings/render.rs` | Render dialog overlay when active; improve OptionalText preview for custom instruction |

## Implementation Steps

1. **Create `CustomInstructionDialog`** in `src/tui/dialogs/custom_instruction.rs`:
   - Two focus zones: TextArea (zone 0) + button row (zone 1)
   - Tab/Shift+Tab to switch zones
   - Enter = newline in text area, activate in button row
   - Escape = cancel from either zone
   - Returns `DialogResult<Option<String>>`

2. **Wire into settings view**:
   - Add dialog state to `SettingsView`
   - In `input.rs`: when Enter on `FieldKey::CustomInstruction`, open popup instead of inline edit
   - When dialog is active, delegate keys to dialog and process results
   - On `Submit`: update field value and persist
   - On `Cancel`: discard changes

3. **Update rendering**:
   - In `render.rs`: render dialog overlay on top of settings when active
   - Improve custom instruction preview in field list (truncated first line + ellipsis)

4. **Tests**:
   - Unit tests for dialog key handling (Tab, Enter, Escape, text input)
   - Unit tests for focus zone transitions
   - Unit tests for Submit/Cancel result values

## Build & Test

```bash
cargo check                    # Quick type-check
cargo build                    # Full build
cargo test                     # Run all tests
cargo fmt && cargo clippy      # Format and lint
```

## Dependencies

- `tui-textarea 0.7` (already in Cargo.toml, currently unused)
- `ratatui 0.29` (existing)
- No new dependencies needed
