<!--
  Sync Impact Report
  ==================
  Version change: 0.0.0 (template) -> 1.0.0
  Modified principles: N/A (initial population from template)
  Added sections:
    - Principle 1: Code Quality
    - Principle 2: Testing Standards
    - Principle 3: User Experience Consistency
    - Principle 4: Performance Requirements
    - Principle 5: Simplicity and Maintainability
    - Section: Performance Standards
    - Section: Development Workflow
    - Governance rules
  Removed sections: None
  Templates requiring updates:
    - .specify/templates/plan-template.md - ✅ no updates needed
      (Constitution Check section already references constitution file dynamically)
    - .specify/templates/spec-template.md - ✅ no updates needed
      (Success Criteria section already supports measurable outcomes)
    - .specify/templates/tasks-template.md - ✅ no updates needed
      (Polish phase already includes performance optimization and testing)
  Follow-up TODOs:
    - RATIFICATION_DATE set to today (2026-02-03) as initial adoption
-->

# Agent of Empires Constitution

## Core Principles

### I. Code Quality

All code merged into the repository MUST pass `cargo fmt`, `cargo clippy`
(with zero warnings), and `cargo check` before being considered complete.

- Every module MUST follow Rust naming conventions: `snake_case` for
  functions/modules, `CamelCase` for types, `SCREAMING_SNAKE_CASE` for
  constants.
- OS-specific logic MUST be isolated in `src/process/{macos,linux}.rs`
  rather than scattered via `cfg` attributes.
- Comments MUST explain "why", not "what". Remove comments that restate
  the code. Keep comments that document non-obvious layout structure,
  formulas, or design decisions.
- No emdashes in documentation or comments.
- Breaking changes MUST be handled via the migration system in
  `src/migrations/` rather than inline compatibility shims.

### II. Testing Standards

All features MUST have corresponding tests. Tests MUST be deterministic
and clean up after themselves.

- Unit tests MUST be colocated in-module using `#[cfg(test)]` for pure
  logic validation.
- Integration tests MUST reside in `tests/*.rs` for end-to-end behavior
  verification.
- Tests MUST NOT read or write real user state; use `tempfile`-based
  temporary directories instead.
- Tmux-dependent tests MUST use unique session names prefixed with
  `aoe_test_` and gracefully skip when tmux is unavailable.
- `cargo test` MUST pass on both Linux and macOS before any PR is merged.
- New configurable fields MUST include test coverage for the settings TUI
  wiring (field key, field entry, apply logic, clear override).

### III. User Experience Consistency

The TUI and CLI MUST provide equivalent functionality. Every operation
available in one interface MUST be accessible from the other.

- Every configurable field added to `SandboxConfig`, `WorktreeConfig`, or
  similar structs MUST be editable in the settings TUI. This requires:
  a `FieldKey` variant, a `SettingField` entry, `apply_field_to_global()`
  and `apply_field_to_profile()` wiring, and a `clear_profile_override()`
  case.
- Profile overrides (`*ConfigOverride` structs) MUST include new fields
  with merge logic in `merge_configs()`.
- Key bindings and navigation patterns MUST be consistent across all TUI
  views. New views MUST reuse existing key conventions (e.g., `n` for new,
  `d` for delete, `Enter` for select, `?` for help).
- Error messages MUST be actionable: state what went wrong and what the
  user can do about it.
- Agent auto-detection MUST work without user configuration for all
  supported agents (Claude Code, OpenCode, Mistral Vibe, Codex CLI,
  Gemini CLI).

### IV. Performance Requirements

The TUI MUST remain responsive under normal operating conditions.
Operations that block the main thread MUST be performed asynchronously.

- TUI render loop MUST maintain a minimum of 30 fps with up to 50
  concurrent sessions displayed.
- Session creation (non-Docker) MUST complete within 2 seconds on a
  standard workstation.
- Status detection polling MUST NOT consume more than 5% CPU when idle
  with 20 active sessions.
- Docker container startup MUST NOT block the TUI; progress MUST be
  displayed to the user.
- Git worktree operations MUST handle repositories with 1000+ branches
  without degrading list/search performance.
- Release builds MUST use `cargo build --release`. The `dev-release`
  profile (skipping LTO) is acceptable for local development only.

### V. Simplicity and Maintainability

Prefer the simplest solution that meets requirements. Avoid speculative
generalization and premature abstraction.

- YAGNI: do not implement features or abstractions for hypothetical
  future requirements.
- Three similar lines of code are preferable to a premature abstraction.
- Only validate at system boundaries (user input, external APIs, file I/O).
  Trust internal code and framework guarantees.
- Do not add error handling for scenarios that cannot occur in practice.
- Removed code MUST be deleted completely. No backwards-compatibility
  shims, re-exports of unused items, or `// removed` comments.

## Performance Standards

Quantitative baselines for regression detection:

| Metric | Target | Measurement |
|--------|--------|-------------|
| TUI startup time | < 500ms | Time from `aoe` invocation to first render |
| Session list refresh | < 100ms | Time to poll and update all session statuses |
| Memory usage (idle) | < 50 MB RSS | With 10 sessions, no active agents |
| Binary size (release) | < 20 MB | Stripped release binary |
| CI pipeline | < 10 min | Full `cargo test` + `cargo clippy` + `cargo fmt --check` |

These targets serve as regression indicators. Exceeding a target MUST be
investigated and justified before merging.

## Development Workflow

All contributions MUST follow this workflow:

1. **Branch**: Create from latest main using convention
   `feature/...`, `fix/...`, `docs/...`, or `refactor/...`.
2. **Implement**: Write code following all Core Principles.
3. **Verify locally**: Run `cargo fmt`, `cargo clippy`, and `cargo test`
   before pushing. All three MUST pass.
4. **PR**: Include a clear "what/why" description, testing methodology,
   and screenshots/recordings for UI changes.
5. **Commit messages**: Use conventional commit prefixes
   (`feat:`, `fix:`, `docs:`, `refactor:`).
6. **Review**: PRs MUST verify compliance with this constitution.
   Complexity beyond what the task requires MUST be justified.

Debug logging is available via `RUST_LOG=agent_of_empires=debug` or
`AGENT_OF_EMPIRES_DEBUG=1`.

## Governance

This constitution is the authoritative source of project standards.
It supersedes all other informal practices or conventions.

- **Amendments** require: (1) a documented rationale, (2) approval from
  a project maintainer, and (3) a migration plan if the change affects
  existing code or data.
- **Versioning** follows semantic versioning: MAJOR for principle
  removals or incompatible redefinitions, MINOR for new principles or
  materially expanded guidance, PATCH for clarifications and wording
  fixes.
- **Compliance review**: all PRs and code reviews MUST verify adherence
  to the principles defined here. Non-compliance MUST be flagged and
  resolved before merge.
- **Runtime guidance**: refer to `CLAUDE.md` for development-time
  conventions and tooling details that supplement this constitution.

**Version**: 1.0.0 | **Ratified**: 2026-02-03 | **Last Amended**: 2026-02-03
