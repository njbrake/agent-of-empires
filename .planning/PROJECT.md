# Project Profile Environment Variables

## What This Is

A feature set that enables per-profile environment variable configuration in Agent of Empires. Profiles can define environment variables that apply to both sandboxed and non-sandboxed coding agent sessions, with automatic precedence resolution when conflicts occur between global sandbox and profile-specific values.

## Core Value

Different profiles can provide different environment variables to coding agents, enabling per-context configuration without manually exporting environment variables or maintaining multiple shell configurations.

## Current State

**Version:** v1.0 shipped 2026-02-14
**Status:** Feature complete, ready for user feedback

The profile environment variables feature is fully implemented and documented. Users can now define per-profile environment variables via TOML configuration that apply to both sandbox and non-sandbox sessions.

## Requirements

### Validated

- ✓ Profile environment fields in data model (`environment`, `environment_values`) — v1.0
- ✓ Environment variable resolution with `$VAR` expansion and `$$` escaping — v1.0
- ✓ Profile environment variable merging with precedence — v1.0
- ✓ TUI settings support for editing profile environment fields — v1.0
- ✓ Session launch integration (tmux + Docker) — v1.0
- ✓ Documentation with merge behavior and use cases — v1.0
- ✓ High test coverage (27 unit + 5 integration tests) — v1.0

### Active

(Awaiting user feedback to define next milestone)

### Out of Scope

- Environment variable validation (beyond format checking)
- Environment variable encryption
- Dynamic environment variable loading (requires config reload)
- Environment variable templating beyond simple `$VAR` expansion
- Profile-specific shell aliases or functions

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Place `environment` and `environment_values` as top-level fields in ProfileConfig | These fields apply to both sandbox AND non-sandbox modes, not just sandbox | ✓ Good |
| Profile env vars merge INTO global.sandbox.environment_* | Maintains consistency with existing config pattern and ensures profile env vars are available in both modes | ✓ Good |
| Profile precedence on name conflicts | User should have final control over env var values in their profile | ✓ Good |
| Extract `resolve_env_vars()` to src/session/config.rs | Shared utility needed by multiple modules; config module is appropriate location | ✓ Good |
| `Option<&ProfileConfig>` parameter pattern | Avoids cloning large ProfileConfig structs while allowing None for "no profile" case | ✓ Good |
| Skip container creation merge refactoring | Implementation already working; would require significant Rust ownership/refactoring for no functional benefit | ⚠ Tech Debt |

## Known Issues

None currently known.

## Technical Debt

- Container creation `build_container_config()` uses profile_config parameter directly without refactoring to use `merge_env_vars_with_profile()` helper. Works correctly but could be more consistent.

## Codebase State

- **Language:** Rust (edition 2021, rust-version 1.74)
- **LOC:** ~30,000 lines
- **Key modules:**
  - `src/session/config.rs` - resolve_env_vars() utility
  - `src/session/profile_config.rs` - environment and environment_values fields
  - `src/session/instance.rs` - merge_env_vars_with_profile() and collection functions
  - `src/tmux/session.rs` - env_vars parameter in session creation
  - `src/tui/settings/` - TUI field definitions for environment fields
- **Documentation:** `docs/guides/configuration.md` - Profile Environment Variables section
- **Test coverage:** 27 unit tests + 5 integration tests for profile env var functionality

## Development Commands

### Build & Test
- `cargo build` - Compile
- `cargo test` - Run all tests (unit + integration)
- `cargo test -- --test-threads=1` - Run tests sequentially (for serial_test)

### Quality Checks
- `cargo fmt` - Format with rustfmt
- `cargo clippy` - Lint code

### Debugging
- `RUST_LOG=agent_of_empires=debug cargo run`

## User Configuration

### Profile Environment Variables Location
- Global config: `$XDG_CONFIG_HOME/agent-of-empires/config.toml` (Linux) or `~/.agent-of-empires/config.toml` (macOS/Windows)
- Profile config: `$XDG_CONFIG_HOME/agent-of-empires/profiles/<profile-name>/config.toml`

### Example Configuration

```toml
# In profile config (e.g., profiles/client-a/config.toml)
environment = ["API_KEY", "DATABASE_URL"]
environment_values = { "PROJECT_ID" = "client-a-123" }
```

## Next Milestone Goals

After v1.0 shipping:
- Gather user feedback on profile environment variables usage
- Consider additional environment variable features (validation, encryption)
- Potential improvements to TUI UX for managing env var lists

---

*Last updated: 2026-02-14 after v1.0 milestone*
