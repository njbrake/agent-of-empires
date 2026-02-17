---
name: review-pr
description: Review a GitHub pull request, fetching contributor forks if needed and analyzing changes against project conventions.
---

# Pull Request Review

Review a GitHub pull request thoroughly, checking code quality, project conventions, and providing actionable feedback.

## Arguments

- `<pr-number-or-url>`: PR number (e.g., `237`) or full GitHub URL (e.g., `https://github.com/owner/repo/pull/237`)

## Instructions

### 1. Fetch PR Information

Use `gh pr view` to get PR details:

```bash
gh pr view <pr-number> --json title,body,author,baseRefName,headRefName,headRepository,headRepositoryOwner,commits,files,additions,deletions,reviews,comments
```

If the PR is from a fork, add the contributor's remote and fetch:

```bash
git remote add <contributor-login> git@github.com:<contributor-login>/<repo>.git 2>/dev/null || true
git fetch <contributor-login> <head-branch-name>
```

### 2. Fetch Latest Base Branch

**Important**: Before analyzing the diff, fetch the latest base branch so the three-dot diff has an accurate merge-base:

```bash
git fetch origin <base-branch>
```

### 3. Analyze the Diff

Get the diff showing only what the PR changes relative to the base branch:

```bash
git diff origin/<base-branch>...<contributor-login>/<head-branch-name>
```

**Note**: Use the three-dot (`...`) syntax which shows changes in the PR branch since it diverged from the base. This ensures you're reviewing only the PR's changes, not unrelated commits that may have been merged into main since the branch was created.

**Important**: The PR branch may not be up to date with the base branch. This is normal and should NOT be flagged as an issue. Focus your review exclusively on the changes introduced by the PR. Do not comment on:
- The branch being "behind main" or needing a rebase
- Features or fixes in main that haven't been merged into the PR branch yet
- Diff noise from the branch not including recent base branch changes

Only mention merge status if there are actual merge conflicts that prevent the PR from being merged.

### 4. Read Affected Files

For each file changed, read the surrounding context to understand:
- The existing code patterns
- How the changes integrate with the codebase
- Whether the changes follow project conventions

### 5. Check Project Conventions

Review CLAUDE.md and any other project guidelines. Verify the PR:
- Follows naming conventions
- Includes required wiring (e.g., for config fields: FieldKey, SettingField, apply functions)
- Has appropriate error handling
- Includes necessary documentation updates
- Uses conventional commit format

### 6. Identify Potential Issues

Look for:
- Logic errors or edge cases not handled
- Security vulnerabilities (injection, path traversal, etc.)
- Missing error handling
- Breaking changes without migration
- Code duplication that could be refactored
- Missing tests for new functionality

### 7. Provide Structured Review

Format your review as:

```markdown
## PR #<number> Review: <title>

### Summary
<Brief description of what the PR does>

### Code Review
<Table or list analyzing each changed file>

### Compliance with Project Guidelines
<How well does this follow CLAUDE.md and other conventions>

### Issues Found
<Any bugs, security issues, or convention violations>

### Suggestions
<Optional improvements that aren't blocking>

### Verdict
<Approve / Request Changes / Needs Discussion>
<Summary reasoning>
```

## What Makes a Good PR

- **Clear scope**: Does one thing well
- **Complete implementation**: All required pieces are present (e.g., for config: struct field, TUI field, apply functions)
- **Appropriate testing**: New functionality has tests, bug fixes have regression tests
- **Clean commits**: Conventional commit format, logical commit history
- **Good documentation**: Updates docs if behavior changes

## Common Issues to Flag

- Config fields added without TUI settings integration
- Error handling that silently swallows errors
- Hard-coded values that should be configurable
- Platform-specific code outside the designated modules
- Breaking changes without migration scripts
- Missing documentation for user-facing changes
