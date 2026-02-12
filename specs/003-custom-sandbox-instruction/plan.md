# Implementation Plan: Custom Sandbox Instruction

**Branch**: `003-custom-sandbox-instruction` | **Date**: 2026-02-11 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/003-custom-sandbox-instruction/spec.md`

## Summary

Add a configurable `custom_instruction` text field to `SandboxConfig` that injects a custom system prompt into the agent's launch command for sandboxed sessions. Uses CLI flags for supported agents (Claude via `--append-system-prompt`, Codex via `--config developer_instructions=`) and displays a warning popup for unsupported agents (Gemini, Vibe, OpenCode, custom commands). Includes full Settings TUI support with profile overrides.

## Technical Context

**Language/Version**: Rust (stable, workspace edition 2021)
**Primary Dependencies**: ratatui (TUI), clap (CLI), serde/toml (config), tmux (session management), Docker (sandboxing)
**Storage**: TOML config files (`~/.config/agent-of-empires/config.toml`, per-profile overrides)
**Testing**: `cargo test` (unit tests in-module, integration tests in `tests/`)
**Target Platform**: Linux, macOS
**Project Type**: Single Rust project (CLI/TUI application)
**Performance Goals**: N/A (config field, no performance-sensitive path)
**Constraints**: Shell command-line length limits for very large instructions; must not break existing agent launch commands
**Scale/Scope**: 6 files modified, ~150 lines of code added

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

Constitution is not configured for this project (template placeholders only). No gates to check. Proceeding with project-level guidelines from CLAUDE.md:

- Settings TUI field requirement: Will be satisfied (FieldKey + SettingField + apply logic + clear override)
- Profile override requirement: Will be satisfied (SandboxConfigOverride field + merge logic)
- cargo fmt/clippy/test: Will be run before finishing

## Project Structure

### Documentation (this feature)

```text
specs/003-custom-sandbox-instruction/
├── plan.md              # This file
├── spec.md              # Feature specification
├── research.md          # Phase 0: delivery mechanism research
├── data-model.md        # Phase 1: entity definitions
├── quickstart.md        # Phase 1: implementation guide
├── checklists/
│   └── requirements.md  # Spec quality checklist
└── tasks.md             # Phase 2 output (created by /speckit.tasks)
```

### Source Code (files to modify)

```text
src/
├── session/
│   ├── config.rs              # Add custom_instruction to SandboxConfig
│   ├── profile_config.rs      # Add override + merge logic
│   └── instance.rs            # Add INSTRUCTION_SUPPORTED_TOOLS, inject CLI flags
├── tui/
│   ├── app.rs                 # Add pre-launch warning for unsupported agents
│   ├── settings/
│   │   ├── fields.rs          # Add FieldKey, build field, apply logic
│   │   └── input.rs           # Add clear_profile_override arm
│   └── dialogs/               # Existing InfoDialog (no changes needed)
└── ...

tests/
└── (unit tests in-module)
```

**Structure Decision**: Existing single-project Rust layout. All changes are modifications to existing files - no new files created in `src/`.

## Implementation Details

### Phase 1: Config Layer

**File: `src/session/config.rs`**
- Add to `SandboxConfig` struct:
  ```rust
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub custom_instruction: Option<String>,
  ```
- Default impl: `custom_instruction: None`

**File: `src/session/profile_config.rs`**
- Add to `SandboxConfigOverride` struct:
  ```rust
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub custom_instruction: Option<String>,
  ```
- Add to `apply_sandbox_overrides()`:
  ```rust
  if let Some(ref custom_instruction) = source.custom_instruction {
      target.custom_instruction = Some(custom_instruction.clone());
  }
  ```

### Phase 2: Settings TUI

**File: `src/tui/settings/fields.rs`**

1. Add `FieldKey::CustomInstruction` variant to the enum

2. Add to `build_sandbox_fields()` using `resolve_optional()`:
   ```rust
   let (custom_instruction, o_ci) = resolve_optional(
       scope,
       global.sandbox.custom_instruction.clone(),
       sb.and_then(|s| s.custom_instruction.clone()),
       sb.map(|s| s.custom_instruction.is_some()).unwrap_or(false),
   );
   // Create SettingField with FieldValue::OptionalText(custom_instruction)
   ```

3. Add to `apply_field_to_global()`:
   ```rust
   (FieldKey::CustomInstruction, FieldValue::OptionalText(v)) => {
       config.sandbox.custom_instruction = v.clone();
   }
   ```

4. Add to `apply_field_to_profile()`:
   Follow the `CpuLimit` pattern - compare with global, set or clear override accordingly.

**File: `src/tui/settings/input.rs`**

5. Add to `clear_profile_override()`:
   ```rust
   FieldKey::CustomInstruction => {
       if let Some(ref mut s) = config.sandbox {
           s.custom_instruction = None;
       }
   }
   ```

### Phase 3: Command Injection

**File: `src/session/instance.rs`**

1. Add constant:
   ```rust
   pub const INSTRUCTION_SUPPORTED_TOOLS: &[&str] = &["claude", "codex"];
   ```

2. In `start_with_size_opts()`, after the YOLO mode tool_cmd construction, append the instruction flag:
   ```rust
   // After tool_cmd is built (with or without YOLO flags)
   if let Some(ref instruction) = sandbox.custom_instruction {
       if !instruction.is_empty() {
           let escaped = shell_escape(instruction);
           tool_cmd = match self.tool.as_str() {
               "claude" => format!("{} --append-system-prompt {}", tool_cmd, escaped),
               "codex" => format!("{} --config developer_instructions={}", tool_cmd, escaped),
               _ => tool_cmd, // unsupported agents: no flag appended
           };
       }
   }
   ```

3. The `custom_instruction` value needs to be available on `SandboxInfo` or resolved from config at launch time. Research shows `SandboxInfo` is the data available in `start_with_size_opts()`. Either:
   - Add `custom_instruction: Option<String>` to `SandboxInfo` (populated at session creation from resolved config), or
   - Pass the resolved config through to the launch method

   The `SandboxInfo` approach is preferred since it follows the pattern of `yolo_mode` being stored on `SandboxInfo`.

### Phase 4: Warning Popup

**File: `src/tui/app.rs`**

In `attach_session()`, after getting the instance but before launching:

```rust
if instance.is_sandboxed() {
    if let Some(ref instruction) = instance.sandbox_info.as_ref()
        .and_then(|s| s.custom_instruction.as_ref())
    {
        if !instruction.is_empty()
            && !INSTRUCTION_SUPPORTED_TOOLS.contains(&instance.tool.as_str())
        {
            // Show InfoDialog warning
            // "Custom instruction is configured but {tool} does not support instruction injection.
            //  The session will launch without the custom instruction."
        }
    }
}
```

The warning is non-blocking - the session launches after the user dismisses the dialog.

## Complexity Tracking

No complexity violations. This feature follows established patterns exactly:
- Config field: mirrors `cpu_limit` / `memory_limit` pattern
- Settings TUI: mirrors `OptionalText` field pattern
- Command injection: mirrors YOLO mode flag injection pattern
- Warning popup: uses existing `InfoDialog`
