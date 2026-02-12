# Tasks: Custom Sandbox Instruction

**Input**: Design documents from `/specs/003-custom-sandbox-instruction/`
**Prerequisites**: plan.md (required), spec.md (required for user stories), research.md, data-model.md, contracts/

**Tests**: Not explicitly requested in the feature specification. Test tasks are omitted.

**Organization**: Tasks are grouped by user story to enable independent implementation and testing of each story.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3)
- Include exact file paths in descriptions

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: No new project setup needed - this is a feature addition to an existing Rust project. Phase 1 is skipped.

---

## Phase 2: Foundational (Config Layer)

**Purpose**: Add the `custom_instruction` field to `SandboxConfig` and `SandboxConfigOverride`. These changes are required by ALL user stories and MUST be completed first.

- [ ] T001 [P] Add `custom_instruction: Option<String>` field with `#[serde(default, skip_serializing_if = "Option::is_none")]` to `SandboxConfig` struct and its `Default` impl in `src/session/config.rs`
- [ ] T002 [P] Add `custom_instruction: Option<String>` field with `#[serde(default, skip_serializing_if = "Option::is_none")]` to `SandboxConfigOverride` struct in `src/session/profile_config.rs`
- [ ] T003 Add merge logic for `custom_instruction` in `apply_sandbox_overrides()` in `src/session/profile_config.rs` -- follow the `cpu_limit` pattern: `if let Some(ref custom_instruction) = source.custom_instruction { target.custom_instruction = Some(custom_instruction.clone()); }`
- [ ] T004 Add `custom_instruction: Option<String>` field with `#[serde(default, skip_serializing_if = "Option::is_none")]` to `SandboxInfo` struct in `src/session/instance.rs` -- this makes the instruction available at launch time, following the `yolo_mode` pattern
- [ ] T005 Add `INSTRUCTION_SUPPORTED_TOOLS` constant (`&["claude", "codex"]`) near the existing `SUPPORTED_TOOLS` and `YOLO_SUPPORTED_TOOLS` constants in `src/session/instance.rs`
- [ ] T006 Populate `SandboxInfo.custom_instruction` from the resolved `SandboxConfig` when creating sandbox info -- find where `SandboxInfo` is constructed (likely in session creation/add flow) and copy `config.sandbox.custom_instruction` into it

**Checkpoint**: Config layer complete. `cargo check` should pass. The `custom_instruction` field is stored, serialized, and merged across global/profile scopes.

---

## Phase 3: User Story 1 - Set a Global Custom Sandbox Instruction (Priority: P1) MVP

**Goal**: Users can set a custom instruction in the Settings TUI and have it injected into the agent's launch command for sandboxed sessions (Claude and Codex).

**Independent Test**: Set a custom instruction in Settings TUI, launch a sandboxed Claude session, verify `--append-system-prompt "your text"` appears in the tmux command.

### Implementation for User Story 1

- [ ] T007 [US1] Add `CustomInstruction` variant to the `FieldKey` enum in `src/tui/settings/fields.rs`
- [ ] T008 [US1] Add the `CustomInstruction` setting field to `build_sandbox_fields()` in `src/tui/settings/fields.rs` -- use `resolve_optional()` with `global.sandbox.custom_instruction.clone()` and `sb.and_then(|s| s.custom_instruction.clone())`, create `SettingField` with `FieldValue::OptionalText`, label `"Custom Instruction"`, description `"Custom instruction text appended to the agent's system prompt in sandboxed sessions (Claude, Codex only)"`
- [ ] T009 [US1] Add match arm for `(FieldKey::CustomInstruction, FieldValue::OptionalText(v))` in `apply_field_to_global()` in `src/tui/settings/fields.rs` -- set `config.sandbox.custom_instruction = v.clone()`
- [ ] T010 [US1] Inject custom instruction CLI flags into `tool_cmd` in `start_with_size_opts()` in `src/session/instance.rs` -- after the YOLO mode tool_cmd match block, check `sandbox.custom_instruction`, if `Some` and non-empty: for `"claude"` append `--append-system-prompt {shell_escape(instruction)}`, for `"codex"` append `--config developer_instructions={shell_escape(instruction)}`, for other tools do nothing. Use existing `shell_escape()` function for safe escaping.
- [ ] T011 [US1] Verify the instruction is NOT injected for host-mode sessions -- confirm that the instruction injection code is only in the `if self.is_sandboxed()` branch of `start_with_size_opts()` in `src/session/instance.rs`

**Checkpoint**: User Story 1 complete. Users can set a global custom instruction and it gets injected into Claude/Codex commands. `cargo fmt && cargo clippy && cargo test` should pass.

---

## Phase 4: User Story 2 - Override Custom Instruction Per Profile (Priority: P2)

**Goal**: Users can set per-profile custom instruction overrides that replace the global instruction for sessions under that profile.

**Independent Test**: Set a global instruction, set a different profile override, launch sessions under each, verify the correct instruction is used.

### Implementation for User Story 2

- [ ] T012 [US2] Add match arm for `(FieldKey::CustomInstruction, FieldValue::OptionalText(v))` in `apply_field_to_profile()` in `src/tui/settings/fields.rs` -- follow the `CpuLimit` pattern: compare `v` with `global.sandbox.custom_instruction`, if equal clear the override (`s.custom_instruction = None`), if different set it (`s.custom_instruction = v.clone()`)
- [ ] T013 [US2] Add match arm for `FieldKey::CustomInstruction` in `clear_profile_override()` in `src/tui/settings/input.rs` -- set `config.sandbox.custom_instruction = None` when inside `if let Some(ref mut s) = config.sandbox`

**Checkpoint**: User Story 2 complete. Profile overrides work for the custom instruction field. `cargo fmt && cargo clippy && cargo test` should pass.

---

## Phase 5: User Story 3 - Clear or Disable Custom Instruction (Priority: P3)

**Goal**: Users can clear a previously set custom instruction so that no instruction is injected.

**Independent Test**: Set an instruction, clear it in Settings, launch a sandboxed session, verify no instruction flag in the command.

### Implementation for User Story 3

- [ ] T014 [US3] Verify that `apply_field_to_global()` correctly handles `FieldValue::OptionalText(None)` for `CustomInstruction` -- setting the field to empty/None in the TUI should result in `config.sandbox.custom_instruction = None`, and the TOML file should omit the field entirely (via `skip_serializing_if = "Option::is_none"`)
- [ ] T015 [US3] Verify that `start_with_size_opts()` correctly skips injection when `custom_instruction` is `None` or `Some("")` -- the check `if let Some(ref instruction) = sandbox.custom_instruction { if !instruction.is_empty() { ... } }` handles both cases

**Checkpoint**: User Story 3 complete. Full CRUD lifecycle for the custom instruction setting works. `cargo fmt && cargo clippy && cargo test` should pass.

---

## Phase 6: Warning Popup for Unsupported Agents (Cross-Cutting)

**Purpose**: Show an InfoDialog warning when launching a sandboxed session with a custom instruction configured for an agent that doesn't support instruction injection.

- [ ] T016 Add pre-launch check in `attach_session()` in `src/tui/app.rs` -- after getting the instance but before calling `start_with_size_opts()`, check if `instance.is_sandboxed()` AND `custom_instruction` is configured (non-empty) AND `instance.tool` is NOT in `INSTRUCTION_SUPPORTED_TOOLS`. If all true, show an `InfoDialog` with title `"Custom Instruction Not Supported"` and message explaining that the agent does not support instruction injection and the session will launch without it. The dialog is informational (non-blocking after dismissal) -- the session proceeds after the user acknowledges.

**Checkpoint**: Warning popup works for Gemini, Vibe, OpenCode, and custom commands. `cargo fmt && cargo clippy && cargo test` should pass.

---

## Phase 7: Polish & Cross-Cutting Concerns

**Purpose**: Final validation and cleanup

- [ ] T017 Run `cargo fmt` to ensure code formatting compliance
- [ ] T018 Run `cargo clippy` and fix any warnings
- [ ] T019 Run `cargo test` and verify all existing tests still pass
- [ ] T020 Manual smoke test: set a custom instruction with special characters (quotes, newlines, `$`, backticks), launch a sandboxed Claude session, verify the command is correctly escaped and the agent receives the instruction

---

## Dependencies & Execution Order

### Phase Dependencies

- **Foundational (Phase 2)**: No dependencies -- can start immediately
- **User Story 1 (Phase 3)**: Depends on Foundational (Phase 2) completion
- **User Story 2 (Phase 4)**: Depends on User Story 1 (Phase 3) -- needs the FieldKey and build_sandbox_fields entry
- **User Story 3 (Phase 5)**: Depends on User Story 1 (Phase 3) -- verification of existing behavior
- **Warning Popup (Phase 6)**: Depends on Foundational (Phase 2) -- needs SandboxInfo.custom_instruction populated
- **Polish (Phase 7)**: Depends on all previous phases

### User Story Dependencies

- **User Story 1 (P1)**: Depends on Foundational phase only. Core MVP.
- **User Story 2 (P2)**: Depends on US1 (needs the FieldKey and settings field to exist). Adds profile override wiring.
- **User Story 3 (P3)**: Depends on US1 (verification of clear behavior). Minimal new code -- mostly verification.

### Within Each User Story

- Settings field definition before apply logic
- Apply logic before command injection
- Command injection before manual verification

### Parallel Opportunities

- **Phase 2**: T001 and T002 can run in parallel (different files)
- **Phase 3**: T007, T008, T009 are sequential (same file), but T010 (instance.rs) can start once T004-T006 are done
- **Phase 4 + Phase 6**: Can potentially run in parallel (different files: fields.rs/input.rs vs app.rs)

---

## Parallel Example: Foundational Phase

```bash
# These can run in parallel (different files):
Task T001: "Add custom_instruction to SandboxConfig in src/session/config.rs"
Task T002: "Add custom_instruction to SandboxConfigOverride in src/session/profile_config.rs"
```

## Parallel Example: User Story 2 + Warning Popup

```bash
# These can run in parallel after US1 is complete (different files):
Task T012: "Add apply_field_to_profile() match arm in src/tui/settings/fields.rs"
Task T016: "Add pre-launch warning check in src/tui/app.rs"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 2: Foundational (config layer)
2. Complete Phase 3: User Story 1 (settings TUI + command injection)
3. **STOP and VALIDATE**: Set a custom instruction, launch a sandboxed Claude session, verify the flag appears
4. This alone delivers the core value of the feature

### Incremental Delivery

1. Foundational → Config layer ready
2. User Story 1 → Global instruction works → MVP!
3. User Story 2 → Profile overrides work
4. User Story 3 → Clear/disable verified
5. Warning Popup → UX polish for unsupported agents
6. Polish → fmt, clippy, test, smoke test

---

## Notes

- [P] tasks = different files, no dependencies
- [Story] label maps task to specific user story for traceability
- Each user story should be independently completable and testable
- Commit after each task or logical group
- Stop at any checkpoint to validate story independently
- Total: 20 tasks across 7 phases
- No new files created in src/ -- all modifications to existing files
