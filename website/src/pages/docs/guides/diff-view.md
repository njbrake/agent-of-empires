---
layout: ../../../layouts/Docs.astro
title: Diff View
description: Review git changes and edit files directly from the Agent of Empires TUI.
---

The diff view lets you review changes between your working directory and a base branch (like `main`), then edit files directly.

## Opening Diff View

From the main screen, press `D` to open the diff view. It shows:
- **Left panel**: List of changed files with status indicators (M=modified, A=added, D=deleted)
- **Right panel**: Diff content for the selected file

The diff is computed against the base branch (defaults to `main` or your repo's default branch).

## Navigation

| Key | Action |
|-----|--------|
| `j` / `k` or Up / Down | Navigate between files |
| Scroll wheel | Scroll through diff content |
| `PgUp` / `PgDn` | Page through diff |
| `g` / `G` | Jump to top / bottom of diff |

## Editing Files

Press `e` or `Enter` to open the selected file in your editor (`$EDITOR`, or vim/nano if not set).

After saving and exiting, the diff view refreshes automatically to show your changes.

## Other Commands

| Key | Action |
|-----|--------|
| `b` | Change base branch |
| `r` | Refresh the diff |
| `?` | Show help |
| `Esc` | Close diff view |

## Configuration

In your config file (`~/.config/agent-of-empires/config.toml` on Linux, `~/.agent-of-empires/config.toml` on macOS):

```toml
[diff]
# Default branch to compare against (auto-detected if not set)
default_branch = "main"

# Lines of context around changes (default: 3)
context_lines = 3
```

## Tips: See Changes While Editing

The diff view shows you where changes are before you edit. For an even better experience, you can install editor plugins that show git diff markers in the gutter while you edit:

### Vim

Install [vim-gitgutter](https://github.com/airblade/vim-gitgutter) or [vim-signify](https://github.com/mhinz/vim-signify). These show `+`, `-`, and `~` markers in the sign column for added, removed, and modified lines.

With vim-plug:
```vim
Plug 'airblade/vim-gitgutter'
```

### Nano

Nano doesn't have a plugin system, so there's no equivalent. Use the diff view to note line numbers before editing, or consider switching to vim for this workflow.

### Other Editors

- **Emacs**: [git-gutter](https://github.com/emacsorphanage/git-gutter)
- **VS Code**: Built-in git gutter support
- **Sublime Text**: [GitGutter](https://packagecontrol.io/packages/GitGutter)

## Workflow Example

1. Press `D` to open diff view
2. Use `j`/`k` to browse changed files
3. Scroll to review each file's changes
4. Press `e` to edit a file that needs work
5. Save and exit the editor
6. Continue reviewing (diff auto-refreshes)
7. Press `Esc` when done
