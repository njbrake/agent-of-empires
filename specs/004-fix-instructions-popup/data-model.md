# Data Model: Fix Custom Instructions Popup Editor

**Branch**: `004-fix-instructions-popup` | **Date**: 2026-02-12

## Entities

No data model changes are required for this feature. The underlying storage entities remain unchanged.

### Existing Entities (unchanged)

#### Custom Instruction (in SandboxConfig)
- **Field**: `custom_instruction: Option<String>`
- **Location**: `src/session/config.rs` (SandboxConfig struct)
- **Storage**: TOML config file at app data directory
- **Behavior**: When `Some(text)`, the text is appended to the agent's system prompt via CLI flags during sandbox session launch. When `None`, no custom instruction is injected.

#### Custom Instruction Override (in SandboxConfigOverride)
- **Field**: `custom_instruction: Option<String>`
- **Location**: `src/session/profile_config.rs` (SandboxConfigOverride struct)
- **Behavior**: When set, overrides the global custom instruction for that profile. Merged via `merge_configs()`.

#### Custom Instruction in Session (in SandboxInfo)
- **Field**: `custom_instruction: Option<String>`
- **Location**: `src/session/instance.rs` (SandboxInfo struct)
- **Behavior**: Captured at session creation time from the effective (merged) config. Injected into CLI commands for Claude (`--append-system-prompt`) and Codex (`--config developer_instructions=`).

## New TUI State (in-memory only, not persisted)

### CustomInstructionDialog
- **focused_zone**: `usize` (0 = text area, 1 = button row)
- **focused_button**: `usize` (0 = Save, 1 = Cancel)
- **text_area**: `tui_textarea::TextArea` (multi-line editor state)
- **original_value**: `Option<String>` (value when dialog opened, for cancel restoration)

### State Transitions

```
Settings List (CustomInstruction field selected)
    |
    | [Enter pressed]
    v
CustomInstructionDialog (text area focused, pre-populated)
    |
    |-- [Tab] --> toggle focus between text area and button row
    |-- [Enter in text area] --> insert newline
    |-- [Enter on Save button] --> Submit(edited_text) --> Settings List (value updated)
    |-- [Enter on Cancel button] --> Cancel --> Settings List (value unchanged)
    |-- [Escape from any zone] --> Cancel --> Settings List (value unchanged)
```
