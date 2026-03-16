# Agent-Safehouse Compatibility with Agent-of-Empires

## Context

You use **agent-safehouse** for macOS native VM sandboxing (via `sandbox-exec`) as an alternative to Docker containers. Your `scd` function in `~/.zshrc` wraps `claude` with `safehouse` to provide OS-level sandboxing without containers.

**agent-of-empires** currently supports two sandbox backends:

- `Docker` (default)
- `AppleContainer` (Apple's `container` CLI)

There is **zero** agent-safehouse integration today. No references to `safehouse`, `sandbox-exec`, or similar anywhere in the codebase.

## Does it work out of the box?

**Partially, with workarounds:**

### What works now (no changes needed)

1. **Non-sandboxed sessions** -- If you disable sandboxing in aoe (`sandbox.enabled_by_default = false`), aoe launches agent binaries directly in tmux. You could set a **custom command** per session like:

   ```
   safehouse --env --enable=docker,shell-init,process-control,clipboard --trust-workdir-config --append-profile ~/.config/agent-safehouse/defaults.sb -- claude --dangerously-skip-permissions
   ```

   This works because aoe's non-sandboxed path (instance.rs:444-460) uses the custom command as-is and appends YOLO flags at the end, which would land after `claude` and be parsed correctly.

2. **Profile-scoped config** -- You could create an aoe profile (e.g. `wfn`) with sandbox disabled, so sessions in that profile always launch on host.

### What doesn't work / is painful

1. **No first-class safehouse backend** -- You can't select "safehouse" as a sandbox runtime in the TUI settings. There's no `ContainerRuntimeName::Safehouse` variant.

2. **Repetitive custom commands** -- Every session would need the full safehouse command manually specified. No way to set a default "wrapper command" at the config level.

3. **YOLO flag double-append** -- If you include `--dangerously-skip-permissions` in your custom command AND have YOLO mode enabled in aoe, the flag gets appended twice (harmless but ugly).

4. **No safehouse-specific config surface** -- Things like safehouse's `--enable`, `--append-profile`, `--add-dirs-ro` would all need to be baked into each custom command string rather than managed in the settings TUI.

5. **Container-specific features won't apply** -- Volume mounts, `custom_instruction` (system prompt injection), env passthrough, container terminal mode -- all of these are wired through the Docker/AppleContainer path and would be skipped.

## Chosen approach: "Command wrapper" config field (Option A)

Add a `command_wrapper` field to `SandboxConfig` (or a new config section) that prefixes every agent launch command. Example config:

```toml
[sandbox]
enabled_by_default = false

[sandbox.wrapper]
command = "safehouse --env --enable=docker,shell-init,process-control,clipboard --trust-workdir-config --append-profile ~/.config/agent-safehouse/defaults.sb --"
```

Then in `start_with_size_opts()`, when not sandboxed but wrapper is set, prepend it to the agent command. This gives you safehouse wrapping without per-session custom commands.

**Scope:** ~50 lines of code. Config struct + field key + merge logic + one conditional in the launch path.

## Files to modify

| File                            | Change                                                                              |
| ------------------------------- | ----------------------------------------------------------------------------------- |
| `src/session/config.rs`         | Add `command_wrapper: Option<String>` to `SandboxConfig`                            |
| `src/session/profile_config.rs` | Add to `SandboxConfigOverride` + merge logic                                        |
| `src/session/instance.rs`       | In non-sandboxed branch (~line 425), prepend wrapper if set                         |
| `src/tui/settings/fields.rs`    | Add `FieldKey::SandboxCommandWrapper` + field entry                                 |
| `src/tui/settings/input.rs`     | Wire up `apply_field_to_global`, `apply_field_to_profile`, `clear_profile_override` |

## Verification

1. `cargo fmt && cargo clippy && cargo test`
2. Set `command_wrapper` in config.toml, create a non-sandboxed session, verify the tmux pane runs `safehouse ... -- claude --dangerously-skip-permissions`
3. Verify profile override works (set wrapper in profile, not global)
4. Verify clearing the wrapper in TUI settings works
5. Verify sandboxed sessions (Docker/AppleContainer) are unaffected
