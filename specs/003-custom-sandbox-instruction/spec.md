# Feature Specification: Custom Sandbox Instruction

**Feature Branch**: `003-custom-sandbox-instruction`
**Created**: 2026-02-08
**Status**: Draft
**Input**: User description: "Please add a configurable setting to add a custom instruction to the agent when it's started inside a sandbox. For example, I want to be able to have a custom instruction that tells the agent it's running in a sandbox."

## Clarifications

### Session 2026-02-11

- Q: How should the custom instruction be delivered to agents -- CLI flags only, file-based (CLAUDE.md/AGENTS.md/GEMINI.md), or hybrid? → A: CLI flags only. Use `--append-system-prompt` for Claude and `--config developer_instructions=` for Codex. Agents that lack a CLI flag (Gemini, Vibe, OpenCode, custom commands) receive a warning popup informing the user that custom instructions are not supported for that agent.
- Q: Should the custom instruction apply to both sandboxed and host-mode sessions? → A: Sandbox only. Host-mode sessions are out of scope.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Set a Global Custom Sandbox Instruction (Priority: P1)

A user wants all agents launched inside sandboxed sessions to receive a custom instruction automatically. For example, they want every sandboxed agent to be told: "You are running inside a Docker sandbox. You have full permissions to install packages and modify the filesystem freely." The user navigates to the Settings TUI, finds the sandbox section, and enters their custom instruction text. From that point forward, every new sandboxed session passes that instruction to the agent at launch time.

**Why this priority**: This is the core feature. Without the ability to set a custom instruction, no other stories matter.

**Independent Test**: Can be fully tested by setting a custom instruction in settings, launching a sandboxed session, and verifying the instruction is passed to the agent command.

**Acceptance Scenarios**:

1. **Given** a user has no custom sandbox instruction configured, **When** they launch a sandboxed session, **Then** no additional instruction text is passed to the agent.
2. **Given** a user sets a custom sandbox instruction in the settings TUI, **When** they launch a new sandboxed session, **Then** the configured instruction text is included in the agent's launch command as a system prompt/instruction argument.
3. **Given** a user updates their custom sandbox instruction, **When** they launch a new sandboxed session, **Then** the updated instruction is used (not the old one).

---

### User Story 2 - Override Custom Instruction Per Profile (Priority: P2)

A user has multiple profiles (e.g., "work" and "personal") and wants different custom instructions for each. Their work profile should tell the agent about corporate proxy settings, while their personal profile uses a simpler instruction. The user sets a global default instruction, then overrides it on specific profiles.

**Why this priority**: Profile overrides are the standard pattern for all sandbox settings and users expect per-profile customization.

**Independent Test**: Can be tested by setting a global instruction, creating a profile override with a different instruction, and verifying each profile's sessions use the correct instruction.

**Acceptance Scenarios**:

1. **Given** a global custom instruction is set and no profile override exists, **When** a sandboxed session is launched under any profile, **Then** the global instruction is used.
2. **Given** a profile-specific custom instruction override is set, **When** a sandboxed session is launched under that profile, **Then** the profile's instruction is used instead of the global one.
3. **Given** a profile override is cleared, **When** a sandboxed session is launched under that profile, **Then** it falls back to the global instruction.

---

### User Story 3 - Clear or Disable Custom Instruction (Priority: P3)

A user previously set a custom sandbox instruction but now wants to remove it entirely. They navigate to the settings TUI, clear the instruction field, and save. Future sandboxed sessions launch without any custom instruction.

**Why this priority**: Users need the ability to undo configuration changes. This completes the CRUD lifecycle for the setting.

**Independent Test**: Can be tested by setting an instruction, clearing it, and verifying no instruction is passed on the next sandboxed session launch.

**Acceptance Scenarios**:

1. **Given** a custom sandbox instruction is set, **When** the user clears the field in settings, **Then** the instruction is removed from the configuration.
2. **Given** the instruction field is empty, **When** a sandboxed session is launched, **Then** no additional instruction argument is passed to the agent.

---

### Edge Cases

- What happens when the custom instruction contains special characters (quotes, newlines, shell metacharacters)? The system must handle these safely without breaking the agent launch command.
- What happens when the custom instruction is extremely long? The system should accept reasonably long instructions (multi-paragraph) without truncation.
- What happens when the custom instruction is set but the session is not sandboxed (host mode)? The instruction should only apply to sandboxed sessions, not host sessions.
- What happens with non-Claude agents (codex, gemini, vibe)? Codex supports injection via `--config developer_instructions=`. Gemini, Vibe, and OpenCode do not support CLI-based instruction injection; the system displays a warning popup and launches without the instruction. Custom commands are also skipped with a warning.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST provide a configurable text field in the sandbox settings for entering a custom instruction string.
- **FR-002**: System MUST pass the custom instruction to the agent's launch command via CLI flags for supported agents: `--append-system-prompt` for Claude, `--config developer_instructions=` for Codex. Agents without a supported flag are skipped.
- **FR-009**: System MUST display a warning popup when launching a sandboxed session with a custom instruction configured for an agent that does not support instruction injection (Gemini, Vibe, OpenCode, custom commands).
- **FR-003**: System MUST NOT pass any custom instruction when the field is empty or not configured.
- **FR-004**: System MUST support profile-level overrides for the custom instruction, following the existing override pattern (global default with per-profile overrides).
- **FR-005**: System MUST allow the custom instruction to be edited, cleared, and saved through the Settings TUI.
- **FR-006**: System MUST persist the custom instruction in the configuration file so it survives application restarts.
- **FR-007**: System MUST safely handle special characters in the instruction text (quotes, newlines, shell metacharacters) without breaking the agent launch command.
- **FR-008**: System MUST only apply the custom instruction to sandboxed sessions, not to host-mode sessions.

### Key Entities

- **Custom Instruction**: A user-defined text string stored in the sandbox configuration. It is optional (can be empty/absent). It has a global value and optional per-profile overrides.
- **Sandbox Session**: An agent session running inside a Docker container. The custom instruction is injected into its launch command.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Users can configure a custom sandbox instruction in under 1 minute through the Settings TUI.
- **SC-002**: 100% of new sandboxed sessions with supported agents (Claude, Codex) include the custom instruction when one is configured. Unsupported agents display a warning instead.
- **SC-003**: Profile-specific overrides correctly replace the global instruction with zero leakage between profiles.
- **SC-004**: The custom instruction survives application restart without data loss.
- **SC-005**: Special characters in the instruction text do not cause agent launch failures.

## Assumptions

- Only Claude (`--append-system-prompt`) and Codex (`--config developer_instructions=`) support CLI-based instruction injection. Gemini, Vibe, OpenCode, and custom commands do not. Unsupported agents receive a warning popup and launch without the instruction.
- The custom instruction is plain text (not a file path). Users who want file-based instructions can use existing hook mechanisms.
- There is no maximum length enforced on the instruction text, though extremely large values (>10KB) may be impractical for command-line arguments and could be passed via alternative mechanisms if needed.
