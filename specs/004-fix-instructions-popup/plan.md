# Implementation Plan: Fix Custom Instructions Popup Editor

**Branch**: `004-fix-instructions-popup` | **Date**: 2026-02-12 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/004-fix-instructions-popup/spec.md`

## Summary

Replace the inline single-line text input for custom sandbox instructions in the TUI settings with a modal popup dialog. The popup uses `tui-textarea::TextArea` for multi-line editing and follows the existing `RenameDialog` pattern with two focus zones (text area + Save/Cancel button row) connected by Tab. No data model changes needed; only the TUI editing experience changes.

## Technical Context

**Language/Version**: Rust (edition 2021, per Cargo.toml)
**Primary Dependencies**: ratatui 0.29, crossterm 0.28, tui-textarea 0.7 (already in Cargo.toml, unused), tui-input 0.11
**Storage**: N/A (no storage changes; existing TOML config persistence unchanged)
**Testing**: cargo test (unit tests in-module `#[cfg(test)]`, integration tests in `tests/`)
**Target Platform**: Linux and macOS terminals
**Project Type**: Single Rust project (binary + library)
**Performance Goals**: TUI render loop >= 30 fps (constitution requirement); popup open/close must be instantaneous
**Constraints**: No new dependencies; tui-textarea 0.7 already declared
**Scale/Scope**: 1 new dialog module, 3 existing files modified, ~300-400 lines of new code

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Code Quality | PASS | New code will follow `cargo fmt`, `cargo clippy` zero-warnings. No OS-specific logic needed. |
| II. Testing Standards | PASS | Unit tests for dialog key handling and focus management will be colocated. No tempfile needed (pure UI logic). |
| III. User Experience Consistency | PASS | Dialog follows existing Tab/Enter/Esc conventions (RenameDialog pattern). No new config fields added; existing `FieldKey::CustomInstruction` unchanged. |
| IV. Performance Requirements | PASS | Dialog is lightweight; no blocking operations. TextArea rendering is immediate. |
| V. Simplicity and Maintainability | PASS | Reuses existing dialog infrastructure (DialogResult, centered_rect, Clear overlay). Single new file. No speculative abstractions. |

**Post-Phase 1 Re-check**: All gates still PASS. No new dependencies introduced. `tui-textarea` was already a declared dependency. Design reuses existing patterns without new abstractions.

## Project Structure

### Documentation (this feature)

```text
specs/004-fix-instructions-popup/
├── spec.md
├── plan.md              # This file
├── research.md          # Phase 0: technology decisions
├── data-model.md        # Phase 1: entity/state documentation
├── quickstart.md        # Phase 1: implementation guide
├── contracts/
│   └── dialog-api.md    # Phase 1: dialog interface contract
├── checklists/
│   └── requirements.md  # Spec quality validation
└── tasks.md             # Phase 2 output (/speckit.tasks)
```

### Source Code (repository root)

```text
src/tui/
├── dialogs/
│   ├── mod.rs                    # Add mod custom_instruction + re-export
│   └── custom_instruction.rs     # NEW: CustomInstructionDialog
├── settings/
│   ├── mod.rs                    # Add dialog state field
│   ├── input.rs                  # Intercept CustomInstruction Enter, handle dialog results
│   └── render.rs                 # Render dialog overlay, improve preview text
└── components/
    └── text_input.rs             # (reference only, no changes)
```

**Structure Decision**: Single Rust project. All changes are within the existing `src/tui/` module tree. One new file (`custom_instruction.rs`) added to the existing `src/tui/dialogs/` directory.

## Complexity Tracking

No constitution violations. No complexity justification needed.
