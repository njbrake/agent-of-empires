# Data Model: Custom Sandbox Instruction

**Feature Branch**: `003-custom-sandbox-instruction`
**Date**: 2026-02-11

## Entities

### SandboxConfig (modified)

**File**: `src/session/config.rs`

| Field | Type | Default | Serde Attributes | Description |
|-------|------|---------|------------------|-------------|
| `custom_instruction` | `Option<String>` | `None` | `#[serde(default, skip_serializing_if = "Option::is_none")]` | User-defined text to pass to the agent as a system prompt instruction when launching sandboxed sessions |

**Validation rules**:
- No maximum length enforced (practical limit is OS command-line length)
- All characters accepted; shell escaping is handled at command construction time
- `None` and `Some("")` are treated equivalently (no instruction injected)

### SandboxConfigOverride (modified)

**File**: `src/session/profile_config.rs`

| Field | Type | Serde Attributes | Description |
|-------|------|------------------|-------------|
| `custom_instruction` | `Option<String>` | `#[serde(default, skip_serializing_if = "Option::is_none")]` | Profile-level override for the custom instruction. `None` means inherit from global. |

**Merge behavior**: In `apply_sandbox_overrides()`, if the profile override has `Some(value)`, it replaces the global `custom_instruction` entirely. If `None`, the global value is used.

### FieldKey (modified)

**File**: `src/tui/settings/fields.rs`

| Variant | Description |
|---------|-------------|
| `CustomInstruction` | New enum variant for the settings TUI field |

### SettingField (new entry in build_sandbox_fields)

| Property | Value |
|----------|-------|
| `key` | `FieldKey::CustomInstruction` |
| `label` | `"Custom Instruction"` |
| `description` | `"Custom instruction text appended to the agent's system prompt in sandboxed sessions (Claude, Codex only)"` |
| `value` | `FieldValue::OptionalText(Option<String>)` |
| `category` | `SettingsCategory::Sandbox` |
| `has_override` | Resolved via `resolve_optional()` |

## Constants

### INSTRUCTION_SUPPORTED_TOOLS

**File**: `src/session/instance.rs`

```
pub const INSTRUCTION_SUPPORTED_TOOLS: &[&str] = &["claude", "codex"];
```

Tools in this list have their instruction flags appended to `tool_cmd` in `start_with_size_opts()`.

## State Transitions

```
None (default)
  -> Some("instruction text")  [user enters text in Settings TUI]
  -> None                       [user clears field in Settings TUI]

Profile Override:
  None (inherit global)
  -> Some("override text")     [user sets profile-specific instruction]
  -> None (inherit global)     [user clears profile override with 'r' key]
```

## Configuration File Format (TOML)

### Global config
```toml
[sandbox]
custom_instruction = "You are running in a Docker sandbox. You have full permissions."
```

### Profile override
```toml
[profiles.work.sandbox]
custom_instruction = "You are in a corporate sandbox. Use the proxy at proxy.corp.com:8080."
```

### No instruction (field absent or omitted)
```toml
[sandbox]
# custom_instruction not present - no instruction injected
```
