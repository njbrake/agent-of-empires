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
