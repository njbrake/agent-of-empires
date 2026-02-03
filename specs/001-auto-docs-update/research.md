# Research: Automated Docs Updating

## R1: Execution Model -- Local Skill vs CI Automation

**Decision**: Use a local agent skill (SKILL.md) invoked by the
maintainer, with a monthly GitHub Actions workflow creating a reminder
issue.

**Rationale**: The maintainer uses a Claude Code subscription (not
per-token API billing). The `anthropics/claude-code-action` in CI
requires an `ANTHROPIC_API_KEY` which uses token-based billing, not
the subscription. Running locally avoids this billing mismatch and
keeps the maintainer in the loop for reviewing changes before they're
committed. The monthly issue serves as the automation nudge.

**Alternatives considered**:
- Fully automated CI with `claude-code-action@v1`: Requires API key
  (token-based billing), not compatible with subscription model.
- Weekly PR automation: Over-engineered for the use case; maintainer
  prefers human-in-the-loop review.

## R2: Cross-Agent Skill Format

**Decision**: Use the Agent Skills open standard (`SKILL.md` with YAML
frontmatter) placed in `.claude/skills/docs-review/SKILL.md`.

**Rationale**: The Agent Skills standard was published by Anthropic in
December 2025 and adopted by OpenAI (Codex CLI), OpenCode, Mistral
Vibe, Cursor, and others. A single SKILL.md file works across 4 of 5
supported agents natively:
- Claude Code: native support (`.claude/skills/`)
- OpenCode: reads `.claude/skills/` as a fallback
- Codex CLI: supports SKILL.md format natively
- Mistral Vibe: supports SKILL.md format natively
- Gemini CLI: does NOT support SKILL.md (uses TOML); user can read
  the prompt manually

**Alternatives considered**:
- Agent-specific command files for each tool: Fragmented, hard to
  maintain, defeats the purpose of a standard.
- Plain markdown prompt in `docs/` or `scripts/`: Works universally
  (copy-paste) but not auto-discoverable as a slash command.
- SKILL.md + Gemini TOML wrapper: Maintainer decided Gemini support
  is not needed.

## R3: Reminder Mechanism

**Decision**: Monthly GitHub Actions workflow that creates an issue
with a `docs-review` label. Duplicate detection via label query.

**Rationale**: GitHub Issues are the simplest notification mechanism
that doesn't require additional tooling. The `GITHUB_TOKEN` provided
by Actions is sufficient for issue creation (no additional secrets).
Label-based duplicate detection is straightforward with `gh issue list
--label docs-review --state open`.

**Alternatives considered**:
- Slack/Discord notification: Requires webhook setup and additional
  secrets. GitHub issues are already the project's issue tracker.
- Email notification: GitHub already sends email notifications for
  new issues if the maintainer has notifications enabled.
- PR instead of issue: Over-complicated; the PR would be empty since
  the actual changes happen locally.

## R4: Schedule Frequency

**Decision**: Monthly (1st of each month at 09:00 UTC).

**Rationale**: Monthly is a reasonable cadence for documentation
review. It's frequent enough to catch drift before it accumulates
significantly, but infrequent enough to not be noisy. The cron
expression `0 9 1 * *` runs on the 1st of each month.

**Alternatives considered**:
- Weekly: Too frequent; creates issue fatigue. The maintainer would
  likely start ignoring weekly reminders.
- Quarterly: Too infrequent; significant doc drift could accumulate.
- On every release: Would require release event detection; adds
  complexity. The maintainer can always run the skill manually before
  a release.

## R5: Skill Placement

**Decision**: Place skill at `.claude/skills/docs-review/SKILL.md` in
the repository root.

**Rationale**: This is the standard project-level skill location for
Claude Code. OpenCode reads this path as a fallback. The skill is
version-controlled with the repo and available to anyone who clones it.
Users of other agents (Codex CLI, Mistral Vibe) can symlink or copy
the skill to their agent's skill directory if needed, or invoke it
by referencing the file path.

**Alternatives considered**:
- `~/.claude/skills/` (personal): Not version-controlled, not shared
  with other contributors.
- Separate skills repo: Over-engineering for a single skill.

## Sources

- [Agent Skills open standard (Anthropic blog)](https://www.anthropic.com/engineering/equipping-agents-for-the-real-world-with-agent-skills)
- [Claude Code GitHub Actions docs](https://code.claude.com/docs/en/github-actions)
- [Claude Code Skills docs](https://code.claude.com/docs/en/skills)
- [Codex CLI Skills](https://developers.openai.com/codex/skills/)
- [AGENTS.md standard](https://agents.md/)
