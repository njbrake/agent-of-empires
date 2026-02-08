# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-02-07)

**Core value:** Different profiles can provide different environment variables to coding agents
**Current focus:** Phase 1 complete, Phase 2 in progress

## Phase Status

| Phase | Name | Status | Plans |
|-------|------|--------|-------|
| 1 | Config Data Model & Shared Resolution | Complete | 1/1 |
| 2 | Session Launch & TUI Settings | In Progress | 1/2 |
| 3 | Documentation | Pending | 0/0 |

Progress: █████░░░░░░░░░░░░░░░ 50% (1.5 of 3 phases complete)

## Current Phase

Phase 2: Session Launch & TUI Settings (1/2 plans complete)
- Last activity: 2026-02-08 - Completed 02-02-PLAN.md
- Plan 02-02: Profile env vars in docker exec commands - Partial completion, Task 3 (container creation) remaining

## Accumulated Context

### Key Decisions

#### Phase 1 Decisions

- **Profile environment fields structure**: Added `environment` and `environment_values` as top-level fields in ProfileConfig rather than inside SandboxConfigOverride, since they apply to both sandbox AND non-sandbox modes
- **Merge strategy**: Profile-level environment fields merge INTO global.sandbox.environment_values and global.sandbox.environment in merge_configs(), maintaining consistency with existing config pattern
- **Shared utility location**: Placed resolve_env_vars() in src/session/config.rs (config module) rather than creating a new environment module, since it's closely related to configuration

#### Phase 2 Decisions

- **Profile env var merging approach**: Docker exec commands now accept profile_config parameter to include profile environment variables
- **Profile precedence**: Profile env vars override sandbox env vars on name conflicts (profile wins)
- **Helper function**: Added merge_env_vars_with_profile() helper for clean merging logic
- **Partial implementation**: Docker exec commands (container terminals) updated, container creation merge not yet complete

### Blockers

- **Container creation merge**: Need to update build_container_config() to use merge_env_vars_with_profile() instead of manual environment building

### Todos

- Phase 2: Complete Task 3 - Update container creation to use merged env vars (SBOX-01, LAUNCH-01)
- Phase 3: Update documentation site with profile environment variables
- Phase 3: Create guide page for profile environment variables with use cases

---
*Last updated: 2026-02-08 after Phase 2 partial completion*

## Wave 1 Execution Summary

**Completed:** 2026-02-08

### Plan 02-01: Add env var support to tmux session creation ✓
- **Task 1**: Add env var parameter to tmux session creation ✓
  - Commits: eaba454
  - Tmux `build_create_args()` now accepts `env_vars: &[(String, String)]` parameter
  - Each env var adds `-e KEY=VALUE` argument before tmux command

- **Task 2**: Wire profile env vars to non-sandbox session launch ✓
  - Commits: ae75fb7
  - `start_with_size_opts()` resolves profile environment variables using `resolve_env_vars()`
  - Non-sandbox sessions receive profile env vars via `session.create_with_size_env()`

- **Task 3**: Wire profile env vars to paired terminal launch ✓
  - Commits: ae75fb7 (combined with Task 2)
  - `start_terminal_with_size()` resolves profile env vars for paired terminals
  - Paired terminals have same env vars as parent session

### Plan 02-02: Merge profile env vars with Docker container environment ⚠️
- **Task 1**: Update collect_env_keys and collect_env_values to include profile env vars ✓
  - Commits: 709b7ed
  - Both functions accept `profile_config: Option<&ProfileConfig>` parameter
  - Profile env vars added to docker exec commands

- **Task 2**: Add merge_env_vars_with_profile helper function ✓
  - Commits: 56657fe
  - Helper merges sandbox and profile env vars
  - Profile env vars override sandbox env vars on name conflicts (profile wins)

- **Task 3**: Update container creation to use merged env vars ⚠️
  - Status: NOT COMPLETE - Rust ownership/reference complexity
  - Issue: Requires updating `build_container_config()` and related functions to use `merge_env_vars_with_profile()`
  - Complexity: `Option<&ProfileConfig>` vs `&ProfileConfig` trait bounds, ownership issues
  - Decision: Requires significant refactoring; deferred to follow-up

**Status:**
- Non-sandbox mode (LAUNCH-01, LAUNCH-02): ✓ COMPLETE
- Sandbox mode (SBOX-01): ⚠️ PARTIAL - Container creation merge not implemented
- Docker exec for container terminals: ✓ COMPLETE

**Commits created:** 4 (eaba454, ae75fb7, 6689c43, 709b7ed, 56657fe)

