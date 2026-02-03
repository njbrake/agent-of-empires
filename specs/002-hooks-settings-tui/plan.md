# Implementation Plan: Hooks at Global/Profile Level & Repo Settings in TUI

**Branch**: `002-hooks-settings-tui` | **Date**: 2026-02-03 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/002-hooks-settings-tui/spec.md`

## Summary

Add `on_create` and `on_launch` hooks to global and profile config
levels (currently repo-only), wire them into the settings TUI as a new
"Hooks" tab, and add a "Repo" tab for editing `.aoe/config.toml` from
the TUI. Hook resolution uses per-field override semantics:
repo > profile > global. All hooks follow the session's sandbox setting
for execution location (container vs local). Failure semantics and
duplicate execution prevention apply uniformly across all config levels.

## Technical Context

**Language/Version**: Rust 1.74+ (MSRV from Cargo.toml)
**Primary Dependencies**: serde/toml (config), ratatui (TUI), tui-input (text editing)
**Storage**: TOML config files (global, profile, repo-level)
**Testing**: `cargo test` (unit + integration)
**Target Platform**: Linux, macOS
**Project Type**: Single Rust workspace
**Performance Goals**: TUI remains responsive; no startup regression
**Constraints**: Config I/O must be fast (buffered); no new dependencies needed
**Scale/Scope**: ~10 files modified, 4 new FieldKey variants, 2 new SettingsCategory variants

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Code Quality & Tooling | PASS | All changes follow existing patterns; `cargo fmt`/`clippy` will be run |
| II. Testing Standards | PASS | Unit tests for hook resolution (all 3 levels, sandbox/local, failure semantics); integration tests for config round-trip |
| III. User Experience Consistency | PASS | New config fields get full TUI wiring (FieldKey, SettingField, apply, clear) in same PR |
| IV. Performance & Efficiency | PASS | No async operations added; hook execution reuses existing sandbox-aware code paths |

**Post-Phase 1 Re-check**: PASS. No new violations introduced by design.

## Project Structure

### Documentation (this feature)

```text
specs/002-hooks-settings-tui/
├── plan.md
├── spec.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/
│   └── config-schema.md
├── checklists/
│   └── requirements.md
└── tasks.md              # Phase 2 output (/speckit.tasks)
```

### Source Code (files touched)

```text
src/
├── session/
│   ├── config.rs              # Add hooks: HooksConfig to Config
│   ├── profile_config.rs      # Add HooksConfigOverride, merge logic
│   ├── repo_config.rs         # Update merge_repo_config() for per-field hook merge
│   └── instance.rs            # Use resolved hooks (global+profile) as fallback
├── tui/
│   ├── settings/
│   │   ├── mod.rs             # Add Hooks + Repo categories, project_path, repo_config fields
│   │   ├── fields.rs          # Add FieldKey variants, build functions, apply functions
│   │   ├── input.rs           # Add clear_profile_override cases, repo save logic
│   │   └── render.rs          # Handle Repo tab disabled state rendering
│   ├── home/
│   │   └── input.rs           # Pass project_path to SettingsView; use resolved config hooks as fallback when repo has no hooks
│   ├── creation_poller.rs     # Accept hooks from resolved config (no structural change needed)
│   └── app.rs                 # No change needed (on_launch skip flag already works)

tests/
├── hooks_config.rs            # New: unit tests for 3-level resolution, sandbox awareness, failure semantics
└── repo_config.rs             # Existing: may need updates for new resolution logic
```

**Structure Decision**: Single Rust workspace, modifications to existing
modules. No new modules needed. Hook execution paths in
`creation_poller.rs` and `instance.rs` already handle sandbox vs local
branching - global/profile hooks flow through the same code path once
resolved into `HooksConfig`.

## Key Design Decisions

### Sandbox Execution (FR-011)

All hooks execute in the session's sandbox container when sandboxed, or
locally when not sandboxed. The config level (global/profile/repo) does
NOT affect execution location. This works naturally because:

1. Global + profile hooks merge into `Config.hooks` via `merge_configs()`
2. Repo hooks override per-field via `merge_repo_config()`
3. The resolved `HooksConfig` is passed to `creation_poller.rs` which
   already branches on `data.sandbox`
4. No new execution paths are needed

### Trust Semantics (FR-004)

Global/profile hooks are implicitly trusted (user-authored in app config
dir). Only repo hooks from `.aoe/config.toml` trigger the trust dialog.
The hook resolution must happen in two stages:

1. Resolve global + profile hooks (always trusted)
2. Check repo hooks via `check_hook_trust()` (may need approval)
3. If repo hooks exist and are trusted, they override per-field
4. If repo hooks need trust, show dialog; if skipped, fall back to
   global/profile hooks

### Duplicate on_launch Prevention (FR-013)

The existing `on_launch_hooks_ran` flag in `CreationResult::Success`
prevents duplicate execution when attaching to a newly created session.
This works without modification because global/profile hooks resolve
into the same `HooksConfig` struct and flow through the same creation
poller code path.

## Complexity Tracking

No constitution violations. No complexity justification needed.
