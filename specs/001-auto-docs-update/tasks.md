# Tasks: Automated Docs Updating

**Input**: Design documents from `/specs/001-auto-docs-update/`
**Prerequisites**: plan.md (required), spec.md (required for user stories), research.md

**Tests**: Not requested in the feature specification. No test tasks included.

**Organization**: Tasks are grouped by user story to enable independent implementation and testing of each story.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2)
- Include exact file paths in descriptions

## Phase 1: Setup

**Purpose**: Create directory structure for the new files

- [x] T001 Create skill directory at .claude/skills/docs-review/

**Checkpoint**: Directory structure ready for implementation

---

## Phase 2: User Story 1 - Docs Review Skill (Priority: P1)

**Goal**: A cross-agent SKILL.md that instructs any compatible AI coding agent to review all project documentation against the current codebase and fix inaccuracies. Only documentation files are modified.

**Independent Test**: Invoke the skill locally in Claude Code via `/project:docs-review` and verify it reads docs and source code, identifies inaccuracies, and only modifies markdown files.

### Implementation for User Story 1

- [x] T002 [US1] Create SKILL.md with YAML frontmatter (name: docs-review, description) at .claude/skills/docs-review/SKILL.md. The prompt MUST instruct the agent to: (1) read all documentation files in docs/, README.md, CLAUDE.md, and other root-level .md files, (2) read relevant source code to cross-reference accuracy -- specifically clap CLI definitions in src/cli/, config structs in src/session/, feature lists, supported agents, and key bindings in src/tui/, (3) identify discrepancies where documentation does not match the code, (4) fix only the documentation files -- never modify .rs, .toml, .yml, or any non-documentation files, (5) for each change, explain what was wrong and what was corrected, (6) if all documentation is already accurate, report that no changes are needed without modifying any files
- [x] T003 [US1] Validate the skill by invoking it locally with Claude Code (`/project:docs-review`) and confirming it produces reasonable output -- review the diff and verify only .md files were touched

**Checkpoint**: User Story 1 complete. The docs review skill is functional and can be invoked from Claude Code, OpenCode, Codex CLI, or Mistral Vibe.

---

## Phase 3: User Story 2 - Monthly Reminder Issue (Priority: P2)

**Goal**: A GitHub Actions workflow that creates a monthly reminder issue nudging the maintainer to run the docs review skill locally.

**Independent Test**: Trigger the workflow via `workflow_dispatch` in the GitHub Actions UI and verify it creates a well-formatted issue with the `docs-review` label, a checklist, and skill invocation instructions. Trigger again and verify no duplicate issue is created.

### Implementation for User Story 2

- [x] T004 [US2] Create GitHub Actions workflow at .github/workflows/docs-review-reminder.yml with: (1) triggers: schedule cron `0 9 1 * *` (1st of month, 09:00 UTC) and workflow_dispatch, (2) permissions: issues write, (3) a step that uses `gh issue list --label docs-review --state open` to check for existing open issues, (4) if no open issue exists, create one using `gh issue create` with title `docs: monthly documentation review`, label `docs-review`, and a body containing: a brief explanation of the task, the skill invocation command for Claude Code (`/project:docs-review`), a checklist (run the skill, review the diff, commit changes, close this issue), and a note that the skill also works with OpenCode, Codex CLI, and Mistral Vibe, (5) if an open issue already exists, log "Skipping: open docs-review issue already exists" and exit successfully
- [ ] T005 [US2] Validate the workflow by triggering it manually via `workflow_dispatch` in GitHub Actions UI and confirming the issue is created correctly with the right label, title, and body content
- [ ] T006 [US2] Validate duplicate prevention by triggering the workflow again while the first issue is still open and confirming no second issue is created

**Checkpoint**: User Story 2 complete. Monthly reminders will be created automatically. Combined with US1, the full feature is operational.

---

## Phase 4: Polish & Cross-Cutting Concerns

**Purpose**: Final validation across both stories

- [x] T007 Verify .claude/skills/docs-review/ is not gitignored (check .gitignore does not re-add .claude/ exclusion)
- [x] T008 Verify the skill SKILL.md renders correctly as plain markdown (readable by agents that do not support the SKILL.md standard natively)

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies -- can start immediately
- **User Story 1 (Phase 2)**: Depends on Phase 1 (directory exists)
- **User Story 2 (Phase 3)**: No dependency on User Story 1 (independent file)
- **Polish (Phase 4)**: Depends on both user stories being complete

### User Story Dependencies

- **User Story 1 (P1)**: Independent. Can start after Phase 1.
- **User Story 2 (P2)**: Independent. Can start in parallel with US1 (different files, no shared state). The issue body references the skill invocation command, but the workflow file does not depend on the skill file existing.

### Parallel Opportunities

- T002 and T004 can run in parallel (different files: SKILL.md vs .yml)
- T003 depends on T002 (validates the skill output)
- T005 and T006 depend on T004 (validate the workflow)
- T007 and T008 can run in parallel

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup (T001)
2. Complete Phase 2: User Story 1 (T002, T003)
3. **STOP and VALIDATE**: Invoke `/project:docs-review` locally
4. The skill is immediately usable without the reminder workflow

### Full Delivery

1. T001: Create directory
2. T002 + T004 in parallel: Write SKILL.md and workflow YAML
3. T003: Validate skill locally
4. T005, T006: Validate workflow via `workflow_dispatch`
5. T007, T008: Polish checks

---

## Notes

- This feature creates 2 new files and 0 Rust code changes
- No `cargo fmt`, `cargo clippy`, or `cargo test` needed (no Rust changes)
- The skill file is plain markdown with YAML frontmatter -- keep it simple
- The workflow uses only `GITHUB_TOKEN` -- no additional secrets required
- Commit after each task or logical group
