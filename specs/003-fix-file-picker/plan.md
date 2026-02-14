# Implementation Plan: Fix File Picker Tab/Enter Behavior

**Branch**: `003-fix-file-picker` | **Date**: 2026-02-11 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/003-fix-file-picker/spec.md`

## Summary

Fix three bugs in the DirPicker component (`src/tui/components/dir_picker.rs`): (1) Tab should autocomplete based on filter matches rather than always following the highlight cursor, (2) Enter on an empty filtered list should select the current directory instead of silently doing nothing, and (3) the hint bar label "Tab cd" should be clearer. All changes are confined to a single file with minimal blast radius.

## Technical Context

**Language/Version**: Rust 2021 edition, MSRV 1.74
**Primary Dependencies**: ratatui 0.29, crossterm 0.28, tui-input 0.11
**Storage**: N/A (no persistence changes)
**Testing**: `cargo test` (in-module `#[cfg(test)]` unit tests)
**Target Platform**: Linux, macOS (terminal TUI)
**Project Type**: Single Rust crate with TUI binary
**Performance Goals**: N/A (keyboard input handling, no latency concerns)
**Constraints**: Must not break existing Backspace, Esc, Ctrl+H, or arrow key behavior
**Scale/Scope**: 3 localized changes in `dir_picker.rs` (lines 131-169, 342-358)

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

Constitution is not configured for this project (template placeholders only). No gates to enforce. Proceeding.

**Post-design re-check**: N/A (no constitution gates defined).

## Project Structure

### Documentation (this feature)

```text
specs/003-fix-file-picker/
├── plan.md              # This file
├── spec.md              # Feature specification
├── research.md          # Phase 0 research findings
├── quickstart.md        # Implementation quickstart guide
├── checklists/
│   └── requirements.md  # Spec quality checklist
└── tasks.md             # Phase 2 output (created by /speckit.tasks)
```

### Source Code (files to modify)

```text
src/tui/components/
└── dir_picker.rs        # All 3 fixes: Tab logic, Enter logic, hint bar text
```

**Structure Decision**: This is a bug fix in a single component. No new files, modules, or architectural changes needed. All modifications are within `dir_picker.rs`.

## Design

### Change 1: Tab Autocomplete (FR-001, FR-002, FR-003)

**Current code** (`dir_picker.rs:151-168`):
```rust
KeyCode::Tab => {
    if filtered_len > 0 && self.selected < filtered_len {
        let selected_name = &filtered[self.selected];
        // ... navigates into selected (highlighted) item
    }
    DirPickerResult::Continue
}
```

**New logic**:
```rust
KeyCode::Tab => {
    if filtered_len == 0 {
        // FR-003: No matches, Tab is a no-op
        return DirPickerResult::Continue;
    }

    // FR-001: Single match -> autocomplete into it (ignore highlight)
    // FR-002: Multiple matches -> use highlighted item from filtered list
    let target = if filtered_len == 1 {
        &filtered[0]
    } else {
        &filtered[self.selected.min(filtered_len - 1)]
    };

    if target == "../" {
        // FR-008: Tab on "../" navigates to parent
        if let Some(parent) = self.cwd.parent() {
            self.cwd = parent.to_path_buf();
            self.filter = Input::default();
            self.selected = 0;
            self.refresh_dirs();
        }
    } else {
        // FR-007: Navigate into directory, clear filter, refresh
        self.cwd = self.cwd.join(target);
        self.filter = Input::default();
        self.selected = 0;
        self.refresh_dirs();
    }
    DirPickerResult::Continue
}
```

**Key behavioral difference**: When `filtered_len == 1`, the code uses `filtered[0]` instead of `filtered[self.selected]`. Since `self.selected` is always 0 when there's only 1 item (typing resets selection to 0), this is mostly a semantic/intentional change. The real fix is for the case where the user types a filter, then arrows down (changing `self.selected`), and more results appear. But the single-match case makes the intent explicit.

### Change 2: Enter on Empty Filter (FR-004, FR-005)

**Current code** (`dir_picker.rs:131-134`):
```rust
KeyCode::Enter => {
    if filtered_len == 0 {
        return DirPickerResult::Continue; // BUG: silent no-op
    }
    // ...
}
```

**New logic**: Remove the early return. The existing fallthrough at line 148-149 already handles this correctly:
```rust
KeyCode::Enter => {
    if self.selected < filtered_len {
        let selected_name = &filtered[self.selected];
        if selected_name == "../" {
            if let Some(parent) = self.cwd.parent() {
                self.active = false;
                return DirPickerResult::Selected(parent.to_string_lossy().to_string());
            }
        } else {
            self.active = false;
            let path = self.cwd.join(selected_name);
            return DirPickerResult::Selected(path.to_string_lossy().to_string());
        }
    }
    // Falls through here when filtered_len == 0 OR selected >= filtered_len
    self.active = false;
    DirPickerResult::Selected(self.cwd.to_string_lossy().to_string())
}
```

### Change 3: Hint Bar Label (FR-006)

**Current** (`dir_picker.rs:346`): `Span::raw(" cd  ")`

**New**: `Span::raw(" enter dir  ")`

### Test Changes

1. **Update** `test_enter_on_empty_filtered_list_does_nothing` (line 672):
   - Rename to `test_enter_on_empty_filtered_list_selects_cwd`
   - Assert `DirPickerResult::Selected(cwd)` instead of `DirPickerResult::Continue`
   - Assert `!picker.is_active()`

2. **Add** `test_tab_single_match_autocompletes`:
   - Setup: tempdir with ["alpha", "beta", "gamma"]
   - Type "al" to filter to just "alpha"
   - Press Down to move highlight (to verify highlight is ignored)
   - Press Tab
   - Assert cwd changed to "alpha" subdirectory

3. **Add** `test_tab_no_match_does_nothing`:
   - Setup: tempdir with directories
   - Type "zzz" to match nothing
   - Press Tab
   - Assert cwd unchanged

## Complexity Tracking

No complexity violations. All changes are within a single file, modifying existing match arms with no new abstractions or patterns.
