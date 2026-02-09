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
git remote add <contributor-login> https://github.com/<contributor-login>/<repo>.git 2>/dev/null || true
git fetch <contributor-login> <head-branch-name>
```

### 2. Analyze the Diff

Get the full diff between base and PR branch:

```bash
git diff <base-branch>...<contributor-login>/<head-branch-name>
```

### 3. Read Affected Files

For each file changed, read the surrounding context to understand:
- The existing code patterns
- How the changes integrate with the codebase
- Whether the changes follow project conventions

### 4. Check Project Conventions

Review CLAUDE.md and any other project guidelines. Verify the PR:
- Follows naming conventions
- Includes required wiring (e.g., for config fields: FieldKey, SettingField, apply functions)
- Has appropriate error handling
- Includes necessary documentation updates
- Uses conventional commit format

### 5. Identify Potential Issues

Look for:
- Logic errors or edge cases not handled
- Security vulnerabilities (injection, path traversal, etc.)
- Missing error handling
- Breaking changes without migration
- Code duplication that could be refactored
- Missing tests for new functionality

### 6. Provide Structured Review

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
