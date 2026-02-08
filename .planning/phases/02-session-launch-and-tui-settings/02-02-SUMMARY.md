---
phase: 02-session-launch-and-tui-settings
plan: 02
subsystem: docker
tags: [docker, environment-variables, profile-config]

# Dependency graph
requires:
  - phase: 01-config-data-model-and-shared-resolution
    provides: Profile environment fields (environment, environment_values) and merge_config() utility
provides:
  - Merged profile env vars with sandbox env vars for docker exec commands
  - Helper function for profile precedence in env var merging
affects: [02-session-launch-and-tui-settings]

# Tech tracking
tech-stack:
  added: merge_env_vars_with_profile() helper function
  patterns: Profile environment variable resolution and merging

key-files:
  modified: src/session/instance.rs
  - collect_env_keys(): Updated to accept and use profile_config parameter for profile env var inclusion
  - collect_env_values(): Updated to accept and use profile_config parameter for profile env var inclusion
  - merge_env_vars_with_profile(): Helper function added for merging profile and sandbox env vars with profile precedence

key-decisions:
  - Profile env vars are loaded at docker exec and container creation points to enable merging with sandbox config
  - Function signatures updated to accept optional profile_config parameter
  - Profile env vars override sandbox env vars on name conflicts (profile wins)

patterns-established:
  - Profile config loading pattern: Config::load().map(|c| c.default_profile) → load_profile_config()

# Metrics
duration: 25 min
completed: 2026-02-08
---
