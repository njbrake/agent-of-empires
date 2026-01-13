# Repository Guidelines

## Project Structure & Module Organization

- `src/main.rs`: binary entrypoint (`aoe`).
- `src/lib.rs`: shared library code used by the CLI/TUI.
- `src/cli/`: clap command handlers (e.g., `src/cli/add.rs`, `src/cli/session.rs`).
- `src/tui/`: ratatui UI and input handling.
- `src/session/`, `src/tmux/`: session storage + tmux integration and status detection.
- `tests/`: integration tests (`tests/*.rs`). `target/` is build output.

## Build, Test, and Development Commands

- `cargo build` / `cargo build --release`: compile (release binary at `target/release/aoe`).
- `cargo build --profile dev-release`: faster optimized builds for local development. Skips LTO for quicker compile times while still producing an optimized binary. Use `--release` for final/CI builds.
- `cargo run --release`: run from source; requires `tmux` installed.
- `cargo check`: fast type-checking during development.
- `cargo test`: run unit + integration tests (some tests skip if `tmux` is unavailable).
- `cargo fmt`: format with rustfmt (run before pushing).
- `cargo clippy`: lint (fix warnings unless there’s a strong reason not to).
- Debug logging: `RUST_LOG=agent_of_empires=debug cargo run` (or `AGENT_OF_EMPIRES_DEBUG=1 cargo run`).

## Coding Style & Naming Conventions

- Prefer “let the tools decide”: keep code `cargo fmt`-clean and `cargo clippy`-clean.
- Rust naming: `snake_case` for modules/functions, `CamelCase` for types, `SCREAMING_SNAKE_CASE` for constants.
- Keep OS-specific logic in `src/process/{macos,linux}.rs` rather than sprinkling `cfg` checks.

## Testing Guidelines

- Use unit tests in-module (`#[cfg(test)]`) for pure logic; use `tests/*.rs` for end-to-end behavior.
- Tests must be deterministic and clean up after themselves (tmux tests should use unique names like `aoe_test_*`).
- Avoid reading/writing real user state; prefer temp dirs (see `tempfile` usage in `src/session/storage.rs`).

## Commit & Pull Request Guidelines

- Branch names: `feature/...`, `fix/...`, `docs/...`, `refactor/...`.
- Commit messages: history is small; follow the repo convention from `CONTRIBUTING.md` (`feat:`, `fix:`, `docs:`, `refactor:`).
- PRs: include a clear “what/why”, how you tested (`cargo test`, plus any manual tmux/TUI checks), and screenshots/recordings for UI changes.

## Local Data & Configuration Tips

- Runtime config/data lives in `~/.agent-of-empires/`; keep it out of commits. For repo-local experiments, use ignored paths like `./.agent-of-empires/`, `.env`, and `.mcp.json`.
