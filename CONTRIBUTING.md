# Contributing to aoe

Thanks for your interest in contributing to aoe (Agent of Empires)! This document provides guidelines for contributing to the project.

## Before You Start

- Search existing [issues](../../issues) and [pull requests](../../pulls) to avoid duplicates
- For significant changes (new features, architectural modifications), please open an issue first to discuss the approach
- Read the [Code of Conduct](CODE_OF_CONDUCT.md)

## Development Setup

### Prerequisites

- **Rust**: Install via [rustup](https://rustup.rs/)
- **tmux**: Required for running the application (`brew install tmux` on macOS, `apt install tmux` on Ubuntu)
- **Git**: For version control

### Quick Start

```bash
# Fork the repo on GitHub, then clone your fork
git clone https://github.com/YOUR_USERNAME/agent-of-empires.git
cd agent-of-empires

# Add upstream remote
git remote add upstream https://github.com/ORIGINAL_OWNER/agent-of-empires.git

# Build and run
cargo build --release
cargo run --release
```

### Useful Commands

```bash
cargo build                    # Debug build
cargo build --release          # Release build
cargo build --profile dev-release  # Fast optimized build for local dev
cargo check                    # Fast type-checking
cargo test                     # Run tests
cargo fmt                      # Format code
cargo clippy                   # Lint
```

For debug logging:
```bash
RUST_LOG=agent_of_empires=debug cargo run
```

## Making Changes

### Branch Naming

Use descriptive branch names with prefixes:
- `feature/...` - New features
- `fix/...` - Bug fixes
- `docs/...` - Documentation changes
- `refactor/...` - Code refactoring

### Code Style

- Run `cargo fmt` before committing
- Fix `cargo clippy` warnings unless there's a strong reason not to
- Follow Rust naming conventions: `snake_case` for functions/modules, `CamelCase` for types
- Keep OS-specific logic in `src/process/{macos,linux}.rs`

See [CLAUDE.md](CLAUDE.md) for detailed coding guidelines and project structure.

### Commit Messages

Use conventional commit prefixes:
- `feat:` - New features
- `fix:` - Bug fixes
- `docs:` - Documentation
- `refactor:` - Code refactoring
- `test:` - Test changes
- `chore:` - Build/tooling changes

Example: `feat: add session export command`

## Testing

- Run `cargo test` before submitting PRs
- Tests should be deterministic and clean up after themselves
- tmux-related tests use unique names prefixed with `aoe_test_*`
- For TUI changes, test manually in a real terminal

## Submitting Pull Requests

1. Push your branch to your fork
2. Open a pull request against the `main` branch
3. Fill out the PR template completely
4. Ensure CI checks pass

### What to Include

- Clear description of what changed and why
- How you tested the changes
- Screenshots/recordings for UI changes
- Link to related issues

## Your First Contribution

New to the project? Here are some ways to get started:

- Look for issues labeled `good-first-issue` or `help-wanted`
- Fix typos or improve documentation
- Add tests for existing functionality
- Try the app and report bugs

Don't hesitate to ask questions in issues or PRs. Every contributor started somewhere!

## Questions?

Open a [GitHub Discussion](../../discussions) or file an issue.
