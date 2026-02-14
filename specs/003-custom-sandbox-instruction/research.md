# Research: Custom Sandbox Instruction

**Feature Branch**: `003-custom-sandbox-instruction`
**Date**: 2026-02-11

## Decision 1: Instruction Delivery Mechanism

**Decision**: CLI flags only, per-agent.

**Rationale**: Only two of the five supported agents (Claude, Codex) have CLI flags for injecting custom instructions. The remaining agents (Gemini, Vibe, OpenCode) and custom commands do not. File-based approaches (writing CLAUDE.md/AGENTS.md/GEMINI.md) were considered but rejected by the user in favor of simplicity - CLI flags for supported agents, warning popup for unsupported ones.

**Agent-specific flags**:
- **Claude**: `--append-system-prompt "text"` - Appends to the default system prompt without replacing it. Works in both interactive and print mode.
- **Codex**: `--config developer_instructions="text"` - Overrides developer instructions config value inline.
- **Gemini**: No CLI flag. Has `GEMINI_SYSTEM_MD` env var but it fully replaces (not appends) the system prompt - too destructive.
- **Vibe**: No CLI flag. System prompt is file-based (`~/.vibe/prompts/<name>.md`) and requires pre-creation.
- **OpenCode**: No CLI flag. No known instruction injection mechanism.

**Alternatives considered**:
- **Hybrid (CLI + file fallback)**: Write agent-specific instruction files for unsupported agents. Rejected - adds complexity and file management burden.
- **File-based only**: Write instruction files for all agents. Rejected - less reliable than CLI flags for agents that support them, and still doesn't cover all agents.
- **Environment variable**: Set a universal env var. Rejected - no agent reads a common env var for instructions.

## Decision 2: Configuration Field Type

**Decision**: `Option<String>` field on `SandboxConfig`, rendered as `OptionalText` in the Settings TUI.

**Rationale**: Follows the exact pattern of existing optional string fields like `cpu_limit` and `memory_limit`. The `OptionalText` field type supports empty/None state (no instruction) and free-form text entry. Profile overrides use the standard `SandboxConfigOverride` pattern with `Option<String>`.

**Alternatives considered**:
- **Required String with empty default**: Would always serialize to config file even when unused.
- **File path reference**: User suggested plain text, not file paths. Existing hook mechanisms serve file-based use cases.

## Decision 3: Warning Mechanism for Unsupported Agents

**Decision**: Show an `InfoDialog` warning popup in the TUI when launching a sandboxed session with a custom instruction configured for an unsupported agent.

**Rationale**: The codebase already has `InfoDialog` (title + message + OK button) used for similar informational warnings. The warning appears at launch time in `app.rs::attach_session()`, after resolving the instance but before calling `start_with_size_opts()`. The session still launches after the user dismisses the warning - the instruction is simply not injected.

**Alternatives considered**:
- **Silent skip**: No warning, just don't inject. Rejected - user should know their instruction isn't being applied.
- **Block launch**: Prevent launching unsupported agents entirely. Rejected - too restrictive. The agent is still useful without the instruction.
- **Settings-time warning**: Show warning when configuring the instruction. Rejected - warning should be contextual at launch time when the tool is known.

## Decision 4: Scope Limitation

**Decision**: Sandbox sessions only. Host-mode sessions are out of scope.

**Rationale**: The user explicitly requested sandbox-only scope. The feature name is "Custom Sandbox Instruction" and the use case is informing the agent about its sandbox environment.

## Decision 5: Supported Tools Constant

**Decision**: Add `INSTRUCTION_SUPPORTED_TOOLS: &[&str] = &["claude", "codex"]` constant in `instance.rs`, following the pattern of `SUPPORTED_TOOLS` and `YOLO_SUPPORTED_TOOLS`.

**Rationale**: Centralizes the list of agents that support instruction injection. Makes it easy to extend when new agents add support for instruction flags. Used both for command construction and for warning logic.

## Decision 6: Shell Escaping

**Decision**: Use the existing `shell_escape()` function for escaping instruction text in CLI arguments.

**Rationale**: The function already handles double quotes, backslashes, `$`, and backticks - the exact characters that could break shell commands. It wraps the result in double quotes. Already proven in the codebase for environment variable escaping in docker exec commands.

## Decision 7: Command Construction Integration Point

**Decision**: Inject the instruction flag into the `tool_cmd` string inside `start_with_size_opts()`, after the YOLO mode flag logic and before the docker exec assembly.

**Rationale**: The existing pattern for YOLO mode already demonstrates per-tool flag injection at this exact location. The custom instruction flag is appended to `tool_cmd` using the same match-on-tool-name pattern. This ensures the instruction is included in the final docker exec command.
