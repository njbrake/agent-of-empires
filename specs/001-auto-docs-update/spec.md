# Feature Specification: Automated Docs Updating

**Feature Branch**: `001-auto-docs-update`
**Created**: 2026-02-03
**Status**: Draft
**Input**: User description: "Automated weekly task that triggers Claude Code to review project documentation and suggest improvements or fixes for outdated content" (GitHub Issue #202)

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Docs Review Skill (Priority: P1)

As a project maintainer, I want a reusable docs review skill that I
can invoke from any supported AI coding agent (Claude Code, OpenCode,
Codex CLI, Mistral Vibe) to review all project documentation against
the current codebase and suggest fixes for outdated content.

**Why this priority**: The skill is the core deliverable. Without a
well-crafted prompt that produces useful documentation fixes, the
reminder workflow has nothing to point to.

**Independent Test**: Can be fully tested by invoking the skill locally
(e.g., `/project:docs-review` in Claude Code) and verifying it
identifies known documentation inaccuracies and proposes corrections
limited to markdown files only.

**Acceptance Scenarios**:

1. **Given** the skill is invoked, **When** documentation references
   outdated CLI flags, removed features, or incorrect instructions,
   **Then** the agent modifies documentation files with corrected
   content.
2. **Given** the skill is invoked, **When** all documentation is
   already accurate, **Then** the agent reports that no changes are
   needed without modifying any files.
3. **Given** the skill is invoked, **When** it encounters source code
   that contradicts documentation, **Then** it updates the
   documentation to match the source code (not the reverse).
4. **Given** the skill is invoked, **When** it makes changes, **Then**
   only documentation files are modified (files in `docs/`, `README.md`,
   `CLAUDE.md`, and similar). Source code is never modified.

---

### User Story 2 - Monthly Reminder Issue (Priority: P2)

As a project maintainer, I want a monthly GitHub issue created
automatically that reminds me to run the docs review skill, so that
documentation reviews happen on a regular cadence without me having
to remember.

**Why this priority**: The reminder is the automation layer that
ensures the skill gets used regularly. Without it, the skill exists
but may be forgotten.

**Independent Test**: Can be tested by triggering the workflow manually
via `workflow_dispatch` and verifying it creates a well-formatted
GitHub issue with instructions for running the skill.

**Acceptance Scenarios**:

1. **Given** the workflow runs on its monthly schedule, **When** no
   open docs review issue exists, **Then** a new issue is created with
   a clear title, instructions for running the skill, and a checklist.
2. **Given** the workflow runs, **When** an open docs review issue from
   a previous month already exists, **Then** no duplicate issue is
   created.
3. **Given** a maintainer triggers the workflow manually via
   `workflow_dispatch`, **Then** it behaves identically to the
   scheduled run.

---

### Edge Cases

- What happens when the skill is run but the agent lacks access to
  source code? The skill prompt MUST instruct the agent to read source
  code for cross-referencing, so the agent needs normal repo file
  access.
- What happens when a previous docs review issue is still open? The
  workflow MUST check for existing open issues with the same label and
  skip creation if one exists.
- What happens when the workflow fails (e.g., GitHub API error)? The
  workflow MUST exit with a non-zero status and surface the error in
  the Actions log. No partial issue is created.
- What happens when the skill is invoked from an agent that does not
  support SKILL.md (e.g., Gemini CLI)? The user can copy the prompt
  content manually. The skill file is plain markdown and readable by
  any agent.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The system MUST provide an Agent Skills standard
  (`SKILL.md`) docs review skill that can be invoked from any
  compatible AI coding agent.
- **FR-002**: The skill prompt MUST instruct the agent to review all
  documentation files (`docs/`, `README.md`, `CLAUDE.md`, and other
  root-level `.md` files) against the current codebase for accuracy.
- **FR-003**: The skill prompt MUST instruct the agent to only modify
  documentation files. Source code MUST NOT be modified.
- **FR-004**: The skill prompt MUST instruct the agent to compare
  documentation against current source code, CLI help output, config
  structs, and other authoritative sources in the codebase.
- **FR-005**: The system MUST provide a GitHub Actions workflow that
  creates a reminder issue on a monthly schedule (cron).
- **FR-006**: The workflow MUST support manual triggering via
  `workflow_dispatch`.
- **FR-007**: The reminder issue MUST include a clear title, a
  checklist of steps (run skill, review changes, commit, close issue),
  and the skill invocation command for at least Claude Code.
- **FR-008**: The workflow MUST check for existing open docs review
  issues (by label) and skip creation if one already exists, preventing
  duplicates.
- **FR-009**: The reminder issue MUST be labeled to enable duplicate
  detection (e.g., `docs-review` label).

### Key Entities

- **Docs Review Skill**: An Agent Skills standard (`SKILL.md`) file
  containing the prompt and instructions for reviewing documentation.
  Invocable as a slash command in compatible agents.
- **Docs Review Reminder Issue**: A GitHub issue created monthly by a
  scheduled workflow, reminding the maintainer to run the docs review
  skill locally.
- **Docs Review Workflow**: The GitHub Actions workflow that creates
  the monthly reminder issue.

## Clarifications

### Session 2026-02-03

- Q: Should the workflow run against the default branch or a configurable branch? → A: Always use the repository's default branch (auto-detected).
- Q: Should there be API usage budget/cost controls per run? → A: No. Claude Code runs under a subscription, not per-token API billing. No cost cap needed.
- Q: Should auto-docs PRs be auto-assigned to a reviewer? → A: No. Maintainers review via normal PR notifications.
- Q: Should the feature run Claude Code in CI or locally? → A: Locally. The workflow creates a reminder issue; the maintainer runs the skill locally using their agent subscription.
- Q: Should the skill be agent-specific or cross-agent? → A: Cross-agent via the SKILL.md open standard. No Gemini CLI TOML wrapper needed.

## Assumptions

- The repository uses GitHub Actions for CI/CD (confirmed by existing
  workflows in `.github/workflows/`).
- The maintainer runs their preferred AI coding agent locally with a
  subscription (not per-token API billing).
- The `docs/` directory and root-level Markdown files are the primary
  documentation sources (confirmed by project structure).
- The SKILL.md format is supported by Claude Code, OpenCode, Codex CLI,
  and Mistral Vibe. Gemini CLI users can read the prompt manually.
- The `GITHUB_TOKEN` provided by GitHub Actions is sufficient for
  creating issues (no additional secrets needed).

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A reminder issue is created monthly without manual
  intervention, with a success rate of 95% or higher over any 6-month
  period.
- **SC-002**: The docs review skill, when invoked locally, identifies
  and proposes fixes for known documentation inaccuracies.
- **SC-003**: The skill produces changes scoped to documentation files
  only -- zero source code files are modified across all invocations.
- **SC-004**: Duplicate reminder issues are never created; at most one
  open docs review issue exists at any time.
- **SC-005**: The skill is invocable from at least 3 different AI
  coding agents (Claude Code, OpenCode, Codex CLI) without
  modification.
