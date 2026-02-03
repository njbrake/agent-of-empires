# Repository Guidelines

## Project Structure & Module Organization

- `src/main.rs`: binary entrypoint (`aoe`).
- `src/lib.rs`: shared library code used by the CLI/TUI.
- `src/cli/`: clap command handlers (e.g., `src/cli/add.rs`, `src/cli/session.rs`).
- `src/tui/`: ratatui UI and input handling.
- `src/session/`: session storage, configuration, and group management.
- `src/tmux/`: tmux integration and status detection.
- `src/process/`: OS-specific process handling (`macos.rs`, `linux.rs`).
- `src/docker/`: Docker sandboxing and container management.
- `src/git/`: git worktree operations and template resolution.
- `src/update/`: version checking against GitHub releases.
- `src/migrations/`: versioned data migrations for breaking changes (see below).
- `tests/`: integration tests (`tests/*.rs`).
- `docs/`: user-facing documentation and guides.
- `scripts/`: installation and utility scripts.
- `xtask/`: build automation workspace.

## Build, Test, and Development Commands

- `cargo build` / `cargo build --release`: compile (release binary at `target/release/aoe`).
- `cargo build --profile dev-release`: faster optimized builds for local development. Skips LTO for quicker compile times while still producing an optimized binary. Use `--release` for final/CI builds.
- `cargo run --release`: run from source; requires `tmux` installed.
- `cargo check`: fast type-checking during development.
- `cargo test`: run unit + integration tests (some tests skip if `tmux` is unavailable).
- `cargo fmt`: format with rustfmt (run before pushing).
- `cargo clippy`: lint (fix warnings unless there’s a strong reason not to).
- Debug logging: `RUST_LOG=agent_of_empires=debug cargo run` (or `AGENT_OF_EMPIRES_DEBUG=1 cargo run`).

## Settings & Configuration

- **Every configurable field must be editable in the settings TUI.** When adding a new config field to `SandboxConfig`, `WorktreeConfig`, etc., you must also:
  1. Add a `FieldKey` variant in `src/tui/settings/fields.rs`
  2. Add a `SettingField` entry in the corresponding `build_*_fields()` function
  3. Wire up `apply_field_to_global()` and `apply_field_to_profile()`
  4. Add a `clear_profile_override()` case in `src/tui/settings/input.rs`
- Profile overrides (`*ConfigOverride` structs in `profile_config.rs`) must also include the new field with merge logic in `merge_configs()`.

## Coding Style & Naming Conventions

- Prefer "let the tools decide": keep code `cargo fmt`-clean and `cargo clippy`-clean.
- **Never use emdashes (—)** in documentation or comments.
- Rust naming: `snake_case` for modules/functions, `CamelCase` for types, `SCREAMING_SNAKE_CASE` for constants.
- Keep OS-specific logic in `src/process/{macos,linux}.rs` rather than sprinkling `cfg` checks.
- Do not be concerned about maintaining backwards compatibility. You should not assume that it needs to be backwards compatible, but you should mention when you make a change that breaks backwards compatibility.
- Add comments where they aid understanding, but remove obvious ones before finishing:
  - **Keep**: comments explaining non-obvious formulas, layout structure documentation, or "why" something is done
  - **Remove**: section headers that just name what the next line does (e.g., `// Render buttons` before `render_buttons()`), or comments restating the code

## Testing Guidelines

- Use unit tests in-module (`#[cfg(test)]`) for pure logic; use `tests/*.rs` for end-to-end behavior.
- Tests must be deterministic and clean up after themselves (tmux tests should use unique names like `aoe_test_*`).
- Avoid reading/writing real user state; prefer temp dirs (see `tempfile` usage in `src/session/storage.rs`).

## Commit & Pull Request Guidelines

- Branch names: `feature/...`, `fix/...`, `docs/...`, `refactor/...`.
- Commit messages: use conventional commit prefixes (`feat:`, `fix:`, `docs:`, `refactor:`).
- PRs: include a clear “what/why”, how you tested (`cargo test`, plus any manual tmux/TUI checks), and screenshots/recordings for UI changes.

## Local Data & Configuration Tips

- Runtime config/data location:
  - **Linux**: `$XDG_CONFIG_HOME/agent-of-empires/` (defaults to `~/.config/agent-of-empires/`)
  - **macOS/Windows**: `~/.agent-of-empires/`
- Keep user data out of commits. For repo-local experiments, use ignored paths like `./.agent-of-empires/`, `.env`, and `.mcp.json`.

## Data Migrations

When making breaking changes to stored data (file locations, config schema, etc.), use the migration system in `src/migrations/` instead of adding fallback/compatibility logic to the main code.

**Why**: Keeps the main codebase clean. Legacy transition logic is isolated and clearly marked as such.

**How it works**:
1. A `.schema_version` file in the app directory tracks the current version
2. On startup, `migrations::run_migrations()` runs any pending migrations in order
3. Each migration bumps the version after completion

**Adding a new migration**:

1. Create `src/migrations/vNNN_description.rs`:
   ```rust
   use anyhow::Result;

   pub fn run() -> Result<()> {
       // Migration logic here
       Ok(())
   }
   ```

2. Update `src/migrations/mod.rs`:
   ```rust
   mod vNNN_description;

   const CURRENT_VERSION: u32 = NNN;  // bump this

   const MIGRATIONS: &[Migration] = &[
       // ... existing migrations ...
       Migration { version: NNN, name: "description", run: vNNN_description::run },
   ];
   ```

**Guidelines**:
- Migrations must be idempotent (safe to run multiple times)
- Use `tracing::info!` to log what's happening
- Platform-specific migrations should use `#[cfg(target_os = "...")]`
- Test migrations by creating the old state manually and verifying the transition
- Before finishing any feature request, make sure that you have run cargo fmt, clippy, and tests.
