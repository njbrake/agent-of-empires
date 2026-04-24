# Repository Guidelines

> `CLAUDE.md` is a symlink to this file. Do not edit `CLAUDE.md` directly; edit `AGENTS.md` instead.

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
- `src/server/`: web dashboard backend (axum server, REST API, WebSocket PTY relay, auth).
- `src/update/`: version checking against GitHub releases.
- `web/`: React + TypeScript frontend for the web dashboard (built with Vite + Tailwind CSS).
- `src/migrations/`: versioned data migrations for breaking changes (see below).
- `tests/`: integration tests (`tests/*.rs`).
- `tests/e2e/`: end-to-end tests exercising the full `aoe` binary (see E2E Tests below).
- `docs/`: user-facing documentation and guides.
- `scripts/`: installation and utility scripts.
- `xtask/`: build automation workspace.

- `contrib/`: community-maintained integration files (e.g., OpenClaw skill). Checked by `cargo xtask check-skill` in CI.

## Build, Test, and Development Commands

- `cargo build` / `cargo build --release`: TUI-only (release binary at `target/release/aoe`).
- `cargo build --profile dev-release`: optimized local builds without LTO; faster compile. Use `--release` for CI.
- `cargo build --features serve`: includes the web dashboard (needs Node.js + npm).
- `cargo test`: unit + integration tests (some skip if `tmux` unavailable).
- `cargo fmt` + `cargo clippy`: run before pushing; fix clippy warnings unless there's a strong reason not to.
- Debug logging: `AGENT_OF_EMPIRES_DEBUG=1 cargo run` (writes `debug.log` in app data dir).
- Running from source needs `tmux` installed.

### Web Dashboard

- Stack: React 19, TypeScript, Vite, Tailwind v4, xterm.js v6. Installable as a PWA ("Install Agent of Empires" in Chrome; "Add to Home Screen" on iOS).
- Build: `cargo build --features serve` (build.rs runs `npm install && npm run build` in `web/` when inputs change).
- Run: `aoe serve --host 0.0.0.0` (token-based auth by default).
- Frontend dev: `cd web && npm run dev` for Vite HMR; the Rust server must also be running for API/WebSocket requests.
- TUI-only `cargo build` (without `--features serve`) needs no JS tooling.

## Settings & Configuration

Every configurable field must be editable in the settings TUI. When adding one to `SandboxConfig`, `WorktreeConfig`, etc., also: add a `FieldKey` in `src/tui/settings/fields.rs`; add a `SettingField` entry in the matching `build_*_fields()`; wire `apply_field_to_global()` + `apply_field_to_profile()`; add a `clear_profile_override()` case in `src/tui/settings/input.rs`; include the field in the `*ConfigOverride` struct in `profile_config.rs` with merge logic in `merge_configs()`.

## Coding Style & Naming Conventions

- Let `cargo fmt` + `cargo clippy` decide; fix warnings.
- **No dead code.** Never add `#[allow(dead_code)]` or write fields/functions that nothing reads. If a field isn't used yet, don't add it; if it stops being used, remove it.
- **No emdashes or `--`** as separators in docs/comments; use commas, semicolons, or rephrase.
- Rust naming: `snake_case` modules/functions, `CamelCase` types, `SCREAMING_SNAKE_CASE` constants.
- Keep OS-specific logic in `src/process/{macos,linux}.rs`, not sprinkled `cfg` checks.
- Don't preserve backwards compatibility by default; call it out when a change is breaking.
- Comments: explain non-obvious "why"; skip section headers and comments that restate the code.

## Testing Guidelines

- Use unit tests in-module (`#[cfg(test)]`) for pure logic; use `tests/*.rs` for integration tests.
- Tests must be deterministic and clean up after themselves (tmux tests should use unique names like `aoe_test_*` or `aoe_e2e_*`).
- Avoid reading/writing real user state; prefer temp dirs (see `tempfile` usage in `src/session/storage.rs`).
- New features touching TUI rendering, CLI subcommands, or session lifecycle should consider adding an e2e test.

### E2E Tests

Full-binary e2e tests live in `tests/e2e/`, exercising `aoe` through tmux (TUI) and as a subprocess (CLI). Run with `cargo test --test e2e` (add `-- --nocapture` for screen dumps on failure).

The harness (`tests/e2e/harness.rs`) exposes `TuiTestHarness` with `spawn_tui()`/`spawn(args)`, `send_keys(keys)`/`type_text(text)`, `wait_for(text)` (10s timeout), `capture_screen()`/`assert_screen_contains(text)`, and `run_cli(args)`. TUI tests auto-skip without tmux; Docker tests use `#[ignore]`; all use `#[serial]` for tmux isolation.

Recording (for PR reviews): `RECORD_E2E=1 cargo test --test e2e -- --nocapture` locally (needs `asciinema` + `agg`, outputs to `target/e2e-recordings/`), or add the `needs-recording` label in CI.

### Web Dashboard Playwright Tests

Baseline Playwright tests live in `web/tests/` (run with `cd web && npx playwright test`). For any change to touch events, xterm.js integration, mobile-viewport behavior, WebSocket message shape, or the soft keyboard, write a real-session e2e against a running `aoe serve` rather than relying on the user to verify on their phone.

Recipe:
1. Shim a fake tool on `$PATH` (a script named `claude` that execs `bash -i`) so `aoe add --cmd claude` creates a live tmux session.
2. `cargo build --features serve`, then start `aoe serve --no-auth --port N` in the background with isolated `HOME=/tmp/aoehome`.
3. In a Node script using `@playwright/test` (already a devDep), emulate mobile with `devices['iPhone 13']` so `pointer: coarse` matches.
4. Spy on PTY bytes by patching `WebSocket.prototype.send` in an `addInitScript` and pushing into `window.__WS_SENT__`.
5. Synthesize multi-touch via `page.evaluate` dispatching raw `new TouchEvent(...)` on `.xterm` — Playwright's `page.touchscreen` is single-finger only.

Keep the script ephemeral unless promoted to `web/tests/` with a mobile Playwright project (`hasTouch: true`, `isMobile: true`). Install browser deps with `npx playwright install-deps` if missing.

**Gotcha:** synthetic touchmove events fire back-to-back with Δt≈1ms, which blows up any `Δpx / Δtime` velocity calculation. Cap velocity and per-frame emit counts, or a real device will look sane while the e2e produces runaway momentum (or vice-versa).

## Commit & Pull Request Guidelines

- Branch names: `feature/...`, `fix/...`, `docs/...`, `refactor/...`.
- Commit messages: use conventional commit prefixes (`feat:`, `fix:`, `docs:`, `refactor:`).
- PRs: follow the template in `.github/pull_request_template.md`. When creating PRs via `gh pr create`, read the template first and use its structure for the `--body` argument. Include a clear “what/why”, how you tested (`cargo test`, plus any manual tmux/TUI checks), and screenshots/recordings for UI changes.

## Git Configuration

- Do not modify git configuration (e.g., `.gitconfig`, `.git/config`, `git config` commands) without explicit user approval.
- The one exception: adding a new remote to fetch a contributor's fork during PR code review is allowed without asking.

## Local Data & Configuration Tips

- Runtime config/data location:
  - **Linux**: `$XDG_CONFIG_HOME/agent-of-empires/` (defaults to `~/.config/agent-of-empires/`)
  - **macOS/Windows**: `~/.agent-of-empires/`
- Keep user data out of commits. For repo-local experiments, use ignored paths like `./.agent-of-empires/`, `.env`, and `.mcp.json`.
- `aoe serve` writes several files to the app dir while running. All are owner-only (0600) where they contain secrets. The daemon cleans them up on shutdown; `daemon_pid()`'s stale-PID check sweeps them otherwise.
  - `serve.pid`: daemon PID for `--stop` and reattach detection.
  - `serve.url`: primary URL (includes the auth token) plus alternates.
  - `serve.mode`: `tunnel` / `tailscale` / `local`.
  - `serve.log`: daemon stdout/stderr tail.
  - `serve.passphrase`: plaintext Tunnel passphrase, so the TUI can show it on reopen across restarts.
  - `serve.last_mode`, `serve.last_port`: picker defaults across launches.

## Data Migrations

Breaking changes to stored data (file locations, config schema) go through `src/migrations/`, not inline fallback/compat shims. A `.schema_version` file tracks state; `migrations::run_migrations()` runs pending ones in order on startup and bumps the version.

To add one:
1. Create `src/migrations/vNNN_description.rs` with a `pub fn run() -> anyhow::Result<()>`.
2. In `src/migrations/mod.rs`: add `mod vNNN_description;`, bump `CURRENT_VERSION`, append a `Migration { version: NNN, name: "description", run: vNNN_description::run }` entry.

Migrations must be idempotent, use `tracing::info!`, gate platform-specific ones with `#[cfg(target_os = "...")]`, and be tested by hand-crafting the old state.

`docs/cli/reference.md` is auto-generated by `cargo xtask gen-docs`; edit the clap help in `src/cli/` and re-run instead. CI enforces it.

## Website & Documentation

The public website (agent-of-empires.com) is an Astro static site in `website/`.

- **`docs/`** is the canonical source for all documentation and guide content. Edit docs here, never on the website side.
- Astro component pages (`*.astro`) like `website/src/pages/guides/index.astro` are not generated; edit them directly.

**Adding a new page to the website:**
1. Create the page in `docs/` (with a `# Title` as the first line).
2. Add an entry to the `PAGES` array in `website/scripts/sync-docs.mjs` with `source`, `dest`, `title`, and `description`.
3. Add the page's source path → website URL mapping to `URL_MAP` in the same script.
4. Add a nav entry in `website/src/data/docsNav.ts`.

The CI workflow (`.github/workflows/docs.yml`) triggers on changes to `docs/**`, `website/**`, and other relevant paths.

## Design System

Read `DESIGN.md` before any visual/UI change — fonts, colors, spacing, and aesthetic direction are defined there. Don't deviate without explicit approval; in QA mode, flag code that doesn't match.

## Skill routing

When the user's request matches an available skill, ALWAYS invoke it using the Skill
tool as your FIRST action. Do NOT answer directly, do NOT use other tools first.
The skill has specialized workflows that produce better results than ad-hoc answers.

Key routing rules:
- Product ideas, "is this worth building", brainstorming → invoke office-hours
- Bugs, errors, "why is this broken", 500 errors → invoke investigate
- Ship, deploy, push, create PR → invoke ship
- QA, test the site, find bugs → invoke qa
- Code review, check my diff → invoke review
- Update docs after shipping → invoke document-release
- Weekly retro → invoke retro
- Design system, brand → invoke design-consultation
- Visual audit, design polish → invoke design-review
- Architecture review → invoke plan-eng-review
- Save progress, checkpoint, resume → invoke checkpoint
- Code quality, health check → invoke health
