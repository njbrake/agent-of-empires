# Implementation Plan: Automated Docs Updating

**Branch**: `001-auto-docs-update` | **Date**: 2026-02-03 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/001-auto-docs-update/spec.md`

## Summary

Add two components for keeping documentation up to date:

1. **A docs review skill** (`SKILL.md`) that any compatible AI coding
   agent can invoke to review documentation against the codebase and
   fix inaccuracies. Uses the Agent Skills open standard for cross-agent
   compatibility (Claude Code, OpenCode, Codex CLI, Mistral Vibe).

2. **A monthly GitHub Actions workflow** that creates a reminder issue
   nudging the maintainer to run the skill locally with their agent
   subscription.

No Rust source code changes are needed. No API keys or secrets are
required (the workflow uses the default `GITHUB_TOKEN` for issue
creation; the skill runs locally on the maintainer's machine).

## Technical Context

**Language/Version**: YAML (GitHub Actions workflow) + Markdown (SKILL.md prompt)
**Primary Dependencies**: GitHub Actions (issue creation only), Agent Skills standard
**Storage**: N/A (no persistent state)
**Testing**: Manual validation by invoking skill locally + `workflow_dispatch` for the reminder
**Target Platform**: Any local dev environment with a compatible AI agent; GitHub Actions for the reminder
**Project Type**: Two-file addition (skill + workflow)
**Performance Goals**: N/A (no runtime performance concerns)
**Constraints**: Skill MUST NOT modify source code; workflow needs only `GITHUB_TOKEN` (no API key secrets)
**Scale/Scope**: ~15 markdown files in `docs/` plus root-level `.md` files

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Code Quality | PASS | No Rust code changes. YAML and Markdown follow project conventions. |
| II. Testing Standards | N/A | No Rust tests needed. Skill validated manually; workflow validated via `workflow_dispatch`. |
| III. User Experience Consistency | PASS | No TUI/CLI changes. Skill uses standard agent invocation patterns. |
| IV. Performance Requirements | PASS | No TUI impact. Skill runs locally; workflow runs in CI. |
| V. Simplicity | PASS | Two files total. No abstractions, no over-engineering. |

No violations. Complexity Tracking section not needed.

## Project Structure

### Documentation (this feature)

```text
specs/001-auto-docs-update/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── spec.md              # Feature specification
├── checklists/
│   └── requirements.md  # Spec quality checklist
└── tasks.md             # Phase 2 output (/speckit.tasks command)
```

### Source Code (repository root)

```text
.claude/skills/docs-review/
└── SKILL.md             # NEW: Cross-agent docs review skill

.github/workflows/
└── docs-review-reminder.yml  # NEW: Monthly reminder issue workflow
```

**Structure Decision**: Two files added. The skill lives in
`.claude/skills/` which is the standard location for Claude Code and is
also read as a fallback by OpenCode. Codex CLI and Mistral Vibe support
the same SKILL.md format in their own skill directories, but placing it
in `.claude/skills/` keeps it version-controlled in the repo. The
workflow lives in `.github/workflows/` alongside existing workflows.

## Design Decisions

### Cross-Agent Skill Format

The skill uses the Agent Skills open standard (`SKILL.md` with YAML
frontmatter). This format is natively supported by:
- **Claude Code**: `.claude/skills/<name>/SKILL.md` (project skills)
- **OpenCode**: Falls back to `.claude/skills/` for compatibility
- **Codex CLI**: Supports SKILL.md format natively
- **Mistral Vibe**: Supports SKILL.md format natively

Gemini CLI uses TOML commands and does not support SKILL.md. Users of
Gemini CLI can read the prompt content manually. No TOML wrapper is
provided (per clarification).

### Skill Prompt Design

The skill prompt instructs the agent to:
1. Read all documentation files (`docs/**/*.md`, `README.md`, `CLAUDE.md`)
2. Read relevant source code to cross-reference accuracy (CLI flags
   from clap definitions, config structs, feature lists, etc.)
3. Identify discrepancies where docs don't match the code
4. Fix only the documentation -- never modify source code
5. Report what was changed and why
6. If everything is accurate, report that no changes are needed

### Monthly Reminder Workflow

The workflow:
- Triggers on `schedule` (monthly cron, 1st of each month at 09:00 UTC)
  and `workflow_dispatch` (manual)
- Uses `GITHUB_TOKEN` (no additional secrets needed)
- Checks for existing open issues with the `docs-review` label
- If none exist, creates a new issue with:
  - Title: `docs: monthly documentation review`
  - Body: checklist with steps to run the skill, review changes, and
    commit
  - Label: `docs-review`
- If an open issue already exists, skips creation (no duplicates)

### No API Keys Required

Unlike the previous CI-based approach, this design requires no API key
secrets. The workflow only creates GitHub issues using the built-in
`GITHUB_TOKEN`. The actual Claude Code / agent invocation happens
locally on the maintainer's machine using their subscription.

## Complexity Tracking

> No violations to justify. Two-file feature with no architectural complexity.
