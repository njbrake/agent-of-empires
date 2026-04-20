---
name: docs-review
description: Review all project documentation against the current codebase and fix inaccuracies. Only modifies documentation files.
---

# Documentation Review

Review all project documentation for accuracy against the current codebase. Fix any outdated, incorrect, or missing information.

## Scope

**Documentation files to review:**
- `README.md`
- `CLAUDE.md`
- All files in `docs/` (guides, CLI reference, installation, quick start, etc.)
- Any other `.md` files at the repository root

**Source code to cross-reference:**
- CLI command definitions in `src/cli/` (clap structs, subcommands, flags, arguments)
- Configuration structs in `src/session/` (config fields, defaults, validation)
- Supported agents and detection logic in `src/process/`
- TUI key bindings and navigation in `src/tui/`
- Docker/sandbox configuration in `src/docker/`
- Git worktree operations in `src/git/`
- Feature flags and capabilities throughout `src/`

## Instructions

1. Read every documentation file listed above.

2. For each documentation file, cross-reference its claims against the actual source code:
   - CLI flags and arguments: compare against clap definitions in `src/cli/`
   - Configuration options: compare against config structs in `src/session/`
   - Supported agents: compare against agent detection in `src/process/`
   - Key bindings: compare against input handling in `src/tui/`
   - Installation steps: verify scripts in `scripts/` match documented commands
   - Feature descriptions: verify features exist and work as documented

3. For each discrepancy found:
   - Fix the documentation to match the source code (never the reverse)
   - Keep the fix minimal and precise
   - Preserve the existing writing style and tone of the document

4. Do NOT modify any non-documentation files. Never edit `.rs`, `.toml`, `.yml`, `.yaml`, `.sh`, `.js`, `.json`, or any other source/config files.

5. After reviewing all files, provide a summary of what was changed and why. If no changes were needed, report that all documentation is accurate.

## What to look for

- CLI commands, flags, or arguments that have been added, removed, or renamed
- Configuration options that have changed defaults, names, or behavior
- Features described in docs that no longer exist
- Features in the code that are not documented
- Incorrect file paths or directory references
- Outdated installation instructions or version requirements
- Key bindings that have changed
- Supported agent lists that are out of date
