# Quickstart: Custom Sandbox Instruction

**Feature Branch**: `003-custom-sandbox-instruction`

## What This Feature Does

Adds a configurable text field to the sandbox settings that injects a custom instruction into the agent's system prompt when launching sandboxed sessions. Currently supports Claude (`--append-system-prompt`) and Codex (`--config developer_instructions=`). Unsupported agents (Gemini, Vibe, OpenCode, custom commands) show a warning popup and launch without the instruction.

## Files to Modify

### 1. Config Layer (data storage)
- `src/session/config.rs` - Add `custom_instruction: Option<String>` to `SandboxConfig`
- `src/session/profile_config.rs` - Add `custom_instruction: Option<String>` to `SandboxConfigOverride` + merge logic in `apply_sandbox_overrides()`

### 2. Settings TUI (user interface)
- `src/tui/settings/fields.rs` - Add `FieldKey::CustomInstruction`, add field to `build_sandbox_fields()`, wire `apply_field_to_global()` and `apply_field_to_profile()`
- `src/tui/settings/input.rs` - Add `clear_profile_override()` match arm

### 3. Session Launch (command construction)
- `src/session/instance.rs` - Add `INSTRUCTION_SUPPORTED_TOOLS` constant, inject CLI flag into `tool_cmd` in `start_with_size_opts()`

### 4. Warning Popup (unsupported agents)
- `src/tui/app.rs` - Add pre-launch check in `attach_session()` to show `InfoDialog` for unsupported agents

## Implementation Order

1. Config + profile override (data layer)
2. Settings TUI field (so users can configure it)
3. Command injection in instance.rs (core functionality)
4. Warning popup in app.rs (UX for unsupported agents)
5. Tests

## Key Patterns to Follow

- **OptionalText field**: Follow the `cpu_limit` / `memory_limit` pattern exactly
- **Shell escaping**: Use `shell_escape()` for instruction text in CLI arguments
- **Tool matching**: Follow the YOLO mode `match self.tool.as_str()` pattern
- **Dialog**: Use `InfoDialog::new(title, message)` for the warning popup
- **Profile override**: Use `set_or_clear_override()` helper in `apply_field_to_profile()`

## Verification

```bash
cargo fmt
cargo clippy
cargo test
```

Manual testing: Set a custom instruction in Settings TUI, launch a sandboxed Claude session, verify `--append-system-prompt "your text"` appears in the tmux command.
