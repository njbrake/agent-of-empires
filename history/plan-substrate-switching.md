# Bidirectional Non-Destructive Substrate Switching — Implementation Plan

**Goal:** TUI/CLI/Web all switch a session between cockpit (ACP) and tmux substrates, as many times as wanted, preserving agent context where capability allows. Refuse cleanly where it doesn't; offer separate destructive "restart" path.

## Debate Summary

**Positions:**
- **gemini-3.1-pro-preview:** Standalone `cockpit/switch.rs` orchestrator; zero-byte sentinel files per reason; fail-fast 409; capability matrix on `AgentSpec`; conditional auto-stopping cockpit poller; concedes `--force` destructive belongs as separate "Restart in <substrate>" verb.
- **gpt-5.5:** Standalone `SubstrateSwitcher` coordinator; typed `WorkerStopIntent` API (implementation can be zero-byte files); fail-fast 409 v1 with schema leaving room for `queue_when_idle` later; mandatory empirical adapter probe before exact switching; refuse unsupported, separate destructive reset.
- **grok-4.3:** Supervisor owns switch methods; `pending_switch` field on `WorkerRecord`; fail-fast 409; lazy TUI-gated poller; hard gate on `load_session_capable`; concedes empirical probe required in PR 1.

**Points of agreement:**
- Backend `cockpit_disable` must stop being destructive — preserve `cockpit_acp_session_id`, event_store rows, `agent_session_id`.
- Fail-fast 409 (or structured `SwitchError::InFlightTurn`) for in-flight turns; no 30s blocking wait.
- Capability matrix on `AgentSpec` exposed via `/api/about`; remove hardcoded TS set.
- No `--force` destructive fallback inside "switch" verb. Separate "Restart in <substrate>" / `--destructive` path.
- Empirical adapter probe required before marking any path exact-switch-safe.
- Cockpit-phase poller (if used) must be capability-gated and auto-stopping.
- Single 11-module PR is rejected by all three; split mandatory.
- Generic typed stop-intent API beats per-reason ad-hoc helpers, even if zero-byte files implement it.

**Resolved disagreements:**

- **Switch logic location**: gemini/openai standalone module vs grok supervisor methods. **Verdict: standalone `src/cockpit/switch.rs`.** Supervisor owns cockpit worker lifecycle; switch coordinates two substrates. Grok conceded `pending_switch` on WorkerRecord; that fits the standalone orchestrator pattern just fine. Supervisor exposes primitives (`shutdown_with_intent`, `is_idle`); switch calls them.

- **Stop-intent mechanism**: zero-byte files (gemini) vs JSON marker (openai) vs WorkerRecord field (grok). **Verdict: zero-byte files behind typed API** (openai's synthesis). Atomic FS op, no parse failures. Public API is `WorkerStopIntent` enum with `mark_stop_intent` / `take_stop_intent`. Files at `<workers_dir>/<id>.intent.{restart|substrate-to-tmux|substrate-to-cockpit}`. Reaper consumes in priority order.

- **In-flight policy**: queue (openai original) vs fail-fast (gemini, grok). **Verdict: fail-fast 409 for v1.** Openai conceded. Structured `409 Conflict { error: "in_flight_turn", retryable: true }`. Request shape supports future `busy_policy` field for opt-in queueing.

- **PR split shape**: 3 vs 4 stacks. **Verdict: 3 PRs.** Compact, end-to-end testable per PR.

- **Empirical adapter probe**: parallel (gemini) vs pre-gate (openai/grok). **Verdict: PR 1 mandatory.** Probe is an ignored test + xtask harness. Capabilities default `false` until probe flips them.

**Verdict:** Build a standalone substrate-switch coordinator that orchestrates cockpit supervisor + tmux Instance + storage. Capability-aware: refuse exact switch when adapter can't preserve context. Empirical probe harness validates per-adapter claims before flipping capability bits. Zero-byte sentinel files behind typed stop-intent API. Fail-fast 409 on in-flight turn. Three PRs: foundation+probe, coordinator+backend non-destructive, client surfaces.

---

## Architecture

```
src/cockpit/switch.rs                  Substrate switch orchestrator (new)
src/cockpit/capabilities.rs            Directional capability resolver (new)
src/cockpit/worker_registry.rs         Generic WorkerStopIntent API (extend)
src/cockpit/supervisor.rs              Expose shutdown_with_intent; map intent → stopped reason
src/server/api/cockpit.rs              cockpit_enable/disable refactored to call switch coordinator (non-destructive)
src/server/api/about.rs                Add substrate_capabilities DTO
src/session/instance.rs                start_with_resume helper threading --resume <agent_session_id>
src/cli/cockpit.rs                     `aoe cockpit switch --to <cockpit|tmux>` (new)
src/tui/home/input.rs                  Shift+S binding
src/tui/dialogs/substrate_switch.rs    Confirm dialog (new)
src/tui/app.rs                         Action::SwitchSubstrate
web/src/lib/acpCapableTools.ts         Remove static set; consume /api/about
web/src/components/cockpit/SwitchSubstrateAction.tsx   Non-destructive wording; handle 409
web/src/components/cockpit/CockpitView.tsx             Handle Stopped { reason: "substrate_switch" }
xtask/src/cockpit_probe.rs             Adapter probe harness (new)
docs/cockpit.md                        Substrate switching section + agent support matrix
```

---

# PR 1 — Foundation: Capabilities + Stop-Intent + Empirical Probe

**Scope**: groundwork. No user-visible feature yet. Lands first because PR 2 depends on capability data.

## Task 1.1 — Generic `WorkerStopIntent` API

**File: `src/cockpit/worker_registry.rs`** (extend)

Add typed API alongside existing `mark_restart_pending` / `take_restart_marker` (keep for backwards compat during PR 1; PR 2 removes).

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerStopIntent {
    RestartPending,
    SubstrateSwitchToTmux,
    SubstrateSwitchToCockpit,
}

impl WorkerStopIntent {
    fn filename_suffix(self) -> &'static str {
        match self {
            Self::RestartPending => "intent.restart",
            Self::SubstrateSwitchToTmux => "intent.substrate-to-tmux",
            Self::SubstrateSwitchToCockpit => "intent.substrate-to-cockpit",
        }
    }
}

fn intent_path(session_id: &str, intent: WorkerStopIntent) -> Result<PathBuf> {
    Ok(workers_dir()?.join(format!("{session_id}.{}", intent.filename_suffix())))
}

pub fn mark_stop_intent(session_id: &str, intent: WorkerStopIntent) {
    let Ok(path) = intent_path(session_id, intent) else { return };
    let _ = std::fs::write(&path, b"");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
}

pub fn take_stop_intent(session_id: &str) -> Option<WorkerStopIntent> {
    // Priority: RestartPending > SubstrateSwitchToTmux > SubstrateSwitchToCockpit.
    // Multiple markers existing is a CLI-script bug; pick one deterministically.
    for intent in [
        WorkerStopIntent::RestartPending,
        WorkerStopIntent::SubstrateSwitchToTmux,
        WorkerStopIntent::SubstrateSwitchToCockpit,
    ] {
        let Ok(path) = intent_path(session_id, intent) else { continue };
        if std::fs::remove_file(&path).is_ok() {
            // Defensive: clean up any other intent files for this session.
            for other in [
                WorkerStopIntent::RestartPending,
                WorkerStopIntent::SubstrateSwitchToTmux,
                WorkerStopIntent::SubstrateSwitchToCockpit,
            ] {
                if other != intent {
                    if let Ok(p) = intent_path(session_id, other) {
                        let _ = std::fs::remove_file(&p);
                    }
                }
            }
            return Some(intent);
        }
    }
    // Backwards-compat: read the legacy `.restart` marker.
    if take_restart_marker(session_id) {
        return Some(WorkerStopIntent::RestartPending);
    }
    None
}
```

**Tests:** roundtrip mark→take, priority order when multiple set, take consumes (subsequent take returns None), legacy `.restart` fallback.

## Task 1.2 — Supervisor reaper consumes typed intent

**File: `src/cockpit/supervisor.rs`** (modify `reap_user_stopped` at `:1083`)

```rust
let intent = super::worker_registry::take_stop_intent(&id);
let reason = match intent {
    Some(WorkerStopIntent::RestartPending) => "restart_pending",
    Some(WorkerStopIntent::SubstrateSwitchToTmux)
    | Some(WorkerStopIntent::SubstrateSwitchToCockpit) => "substrate_switch",
    None => "user_stopped",
};
```

Existing `restart_pending` return-list behavior preserved. New: also include `SubstrateSwitch*` in the returned list so reconciler can re-enable spawn (for the `SubstrateSwitchToCockpit` direction).

Add `shutdown_with_intent`:

```rust
pub async fn shutdown_with_intent(
    &self,
    session_id: &str,
    intent: WorkerStopIntent,
) -> Result<(), SupervisorError> {
    super::worker_registry::mark_stop_intent(session_id, intent);
    self.shutdown(session_id).await
}
```

**Tests:** reaper publishes `substrate_switch` reason when intent file set; reaper returns id in respawn-list for SubstrateSwitchToCockpit; existing restart tests unchanged.

## Task 1.3 — Capability resolver

**File: `src/cockpit/capabilities.rs`** (new)

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Substrate {
    Cockpit,
    Tmux,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContinuityMode {
    /// Switch preserves model context exactly (same underlying agent session).
    Exact,
    /// Switch refused — no continuity available.
    Unsupported,
}

#[derive(Debug, Clone, Serialize)]
pub struct DirectionalCapability {
    pub from: Substrate,
    pub to: Substrate,
    pub supported: bool,
    pub continuity: ContinuityMode,
    pub requires_acp_session_id: bool,
    pub requires_agent_session_id: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolCapabilities {
    pub tool: String,
    pub acp_agent_name: Option<String>,
    pub acp_available: bool,         // adapter binary present
    pub load_session_capable: bool,  // ACP adapter advertises session/load
    pub tmux_resume_strategy_present: bool,
    pub native_session_discoverable: bool, // ACP-phase emits agent_session_id (per probe)
    pub directions: Vec<DirectionalCapability>,
}

pub fn resolve_for_tool(tool: &str) -> ToolCapabilities { /* combine
    AgentRegistry::with_defaults() + crate::agents::get_agent(tool) +
    cockpit::probe_results.json or hardcoded defaults */ }

pub fn resolve_all() -> Vec<ToolCapabilities> { /* iterate known tools */ }
```

Capability rules:
- `cockpit → tmux` supported when `acp_available && tmux_resume_strategy_present && native_session_discoverable` (need `agent_session_id` to feed `--resume`).
- `tmux → cockpit` supported when `acp_available && load_session_capable && (existing cockpit_acp_session_id OR adapter can import native id)`. For now, require existing `cockpit_acp_session_id` — refuse pure tmux-origin promotion until adapter import support is verified.

## Task 1.4 — Empirical adapter probe harness

**File: `xtask/src/cockpit_probe.rs`** (new) + wire into `xtask` main.

```bash
cargo xtask cockpit-probe --agent claude
cargo xtask cockpit-probe --agent codex
cargo xtask cockpit-probe --agent opencode
cargo xtask cockpit-probe --agent gemini
cargo xtask cockpit-probe --agent vibe
cargo xtask cockpit-probe --agent pi
cargo xtask cockpit-probe --all
```

Each probe runs four sub-tests against a temp HOME:
1. **ACP load**: `session/new` → unique token prompt → restart adapter → `session/load <acp_id>` → recall test.
2. **Native artifact**: scan known native session stores after step 1; record if probe token reachable from disk.
3. **Cockpit→tmux exact**: if (2) yielded native id, native CLI `--resume <id>` → recall test.
4. **Tmux→cockpit exact**: native CLI prompt → capture native id → attempt ACP load/import → recall test.

Output: `<app_dir>/cockpit-probe-results.json`. Each capability bit:

```json
{
  "claude": {
    "acp_load_session_capable": true,
    "native_session_discoverable": false,
    "cockpit_to_tmux_exact": false,
    "tmux_to_cockpit_exact_via_acp_id": true,
    "tmux_to_cockpit_exact_via_native_import": false,
    "tested_at": "...",
    "adapter_version": "..."
  }
}
```

Hardcoded conservative defaults shipped in `capabilities.rs` until probe results found on disk. Probe is `#[ignore]` by default — manual run, results committed to repo as fixture.

Verification: this gates how PR 2 reports per-agent supported directions. **If probe shows `claude-agent-acp` doesn't emit native artifacts, then cockpit→tmux exact for Claude is `Unsupported` and the switch dialog refuses.**

## Task 1.5 — `/api/about` capability exposure

**File: `src/server/api/about.rs`** (extend; may be `system.rs`)

Add:
```rust
pub substrate_capabilities: Vec<ToolCapabilities>,
```

Drive frontend. PR 3 consumes.

**Tests:** unit test verifies known tools appear; gated tools (e.g. master disabled) report `supported = false` with reason.

## PR 1 Acceptance

- `cargo test --features serve` green.
- `take_stop_intent` works including legacy `.restart` fallback.
- Probe harness runs against at least claude + aoe-agent and writes fixture.
- `/api/about` returns substrate_capabilities.
- No user-visible behavior change yet.

---

# PR 2 — Backend Coordinator + Non-Destructive Disable

**Scope**: substrate switching works end-to-end via HTTP endpoints. No new CLI/TUI yet (existing web UI starts working since endpoints are non-destructive).

## Task 2.1 — Switch orchestrator

**File: `src/cockpit/switch.rs`** (new)

```rust
use anyhow::Result;
use crate::session::Storage;
use super::capabilities::{Substrate, ContinuityMode};
use super::worker_registry::{self, WorkerStopIntent};

#[derive(Debug, thiserror::Error)]
pub enum SwitchError {
    #[error("session not found: {0}")]
    SessionNotFound(String),
    #[error("already in target substrate")]
    AlreadyInTargetState,
    #[error("switch from {from:?} to {to:?} not supported: {reason}")]
    Unsupported { from: Substrate, to: Substrate, reason: String },
    #[error("session is processing a turn; retry when idle")]
    InFlightTurn,
    #[error("required adapter not on PATH: {0}")]
    AdapterMissing(String),
    #[error("gate disabled: {0}")]
    GateDisabled(&'static str),
    #[error("io: {0}")]
    Io(#[from] anyhow::Error),
}

pub async fn switch_to_cockpit(
    state: &Arc<AppState>,
    session_id: &str,
) -> Result<(), SwitchError> { /* see steps */ }

pub async fn switch_to_tmux(
    state: &Arc<AppState>,
    session_id: &str,
) -> Result<(), SwitchError> { /* see steps */ }
```

**`switch_to_tmux` (cockpit → tmux) steps:**
1. Load instance from `state.instances.read()`. Bail `AlreadyInTargetState` if `cockpit_mode == false`.
2. Resolve capability for `Substrate::Cockpit → Substrate::Tmux`. Bail `Unsupported` if not.
3. In-flight check: `state.cockpit_event_store.has_in_flight_turn(id)` → `InFlightTurn` if true. No wait.
4. Validate `agent_session_id` present (required for `--resume`). If missing — try one synchronous best-effort capture; if still missing, refuse with `Unsupported { reason: "agent_session_id not captured" }`.
5. `state.cockpit_supervisor.shutdown_with_intent(id, WorkerStopIntent::SubstrateSwitchToTmux).await` — marks intent, SIGTERM runner, reaper publishes `Stopped { reason: "substrate_switch" }`.
6. Mutate instance: `cockpit_mode = false`. Keep `cockpit_acp_session_id`, keep event_store rows, keep `agent_session_id`. Persist via `Storage::save`.
7. Update in-memory `state.instances`.
8. `instance.start_with_size(...)` — launches tmux with agent's resume flag.

**`switch_to_cockpit` (tmux → cockpit) steps:**
1. Load instance. Bail `AlreadyInTargetState` if `cockpit_mode == true`.
2. Resolve capability for `Substrate::Tmux → Substrate::Cockpit`. Bail `Unsupported`.
3. Master/experimental gates: existing `cockpit_gate(&state)` — `GateDisabled` if off.
4. Adapter present: `command_present(&spec.command)` — `AdapterMissing` if not.
5. Require `cockpit_acp_session_id` present (no pure tmux-origin promotion in v1) — `Unsupported { reason: "no prior cockpit session id" }` if absent. Future: lift if probe proves adapter native-import works.
6. Status check: instance status must be `Idle` — `InFlightTurn` if `Running` (tmux side has no event_store; rely on existing status detection).
7. `Instance::kill()` — tear down tmux best-effort.
8. Mutate instance: `cockpit_mode = true`. Keep both ids. Persist.
9. Update in-memory `state.instances`.
10. No supervisor call — reconciler's next tick (2s) spawns runner via `session/load(cockpit_acp_session_id)`.

## Task 2.2 — Refactor REST handlers

**File: `src/server/api/cockpit.rs`**

Replace destructive bodies of `cockpit_enable` (`:355`) and `cockpit_disable` (`:464`) to call `switch::switch_to_cockpit` / `switch::switch_to_tmux`. Map errors to HTTP codes:

- `AlreadyInTargetState` → `200 OK` with `cockpit_mode` body (idempotent).
- `Unsupported` → `422 Unprocessable Entity` with `{ error, reason }`.
- `InFlightTurn` → `409 Conflict` with `{ error: "in_flight_turn", retryable: true }`.
- `AdapterMissing` → `400`.
- `GateDisabled` → `403`.
- `SessionNotFound` → `404`.

Remove:
- `state.cockpit_supervisor.forget_session(&id);`
- `state.cockpit_event_store.delete_session(&id);`
- `instance.cockpit_acp_session_id = None;`

## Task 2.3 — Reconciler hook for `SubstrateSwitchToCockpit`

**File: `src/server/mod.rs`** reconciler at `:1353`

Already attempts attach/spawn for `cockpit_mode == true` instances. After PR 1's reaper extension, `SubstrateSwitchToCockpit` returns the id in the respawn-list, clearing `attempted`. No further reconciler change needed for the v1 flow because the in-process call path is: `switch_to_cockpit` mutates state directly, reconciler picks up next tick.

## Task 2.4 — `start_with_resume` helper on `Instance`

**File: `src/session/instance.rs`**

Verify `Instance::start_with_size` already threads `agent_session_id` via `append_resume_flags` (it does — see `instance.rs:260` for `build_resume_flags`). If yes, no change needed; if it gates on context state, expose explicit `start_for_substrate_switch(...)` that always resumes.

## Task 2.5 — `cockpit_master_enabled` escape hatch for disable

Disable must work even when master is off, so users can escape stuck cockpit sessions. Switch out from cockpit always allowed; switch in honors gates.

```rust
// In switch_to_tmux — DON'T check cockpit_gate (already-in-cockpit user can always escape).
// In switch_to_cockpit — DO check cockpit_gate.
```

## Task 2.6 — Tests

**File: `tests/cockpit_substrate_switch.rs`** (new integration test)

- Round-trip: create cockpit session → send prompt → switch to tmux → verify event_store preserved + `cockpit_acp_session_id` preserved + tmux command has `--resume <agent_session_id>` → switch back to cockpit → verify ACP `session/load` called with same id.
- Refusal: switch when `has_in_flight_turn == true` → 409.
- Refusal: tmux→cockpit with no `cockpit_acp_session_id` → 422.
- Idempotency: switch to cockpit when already cockpit → 200 with no state change.
- Reaper: substrate switch intent published `Stopped { reason: "substrate_switch" }`.

## PR 2 Acceptance

- Existing web UI `SwitchSubstrateAction` keeps working but is now non-destructive.
- Round-trip test passes for at least one agent that probe marks as fully supported.
- All HTTP error responses are structured JSON with `error` and `retryable` fields where applicable.

---

# PR 3 — Client Surfaces (TUI + CLI + Web Polish)

**Scope**: user-facing UX. Depends on PR 1 + PR 2.

## Task 3.1 — TUI keybind

**Files**:
- `src/tui/app.rs`: add `Action::SwitchSubstrate { session_id: String, target: Substrate }`.
- `src/tui/home/input.rs:1119`: insert `KeyCode::Char('S')` arm before `Enter`. Gate: real session selected, not Creating/Deleting. Open dialog.
- `src/tui/dialogs/substrate_switch.rs` (new): confirm dialog. Wording:
  - cockpit→tmux: "Open this session in terminal mode? The agent and conversation continue. Tmux scrollback is not preserved."
  - tmux→cockpit: "Open this session in cockpit mode? The agent and conversation continue."
  - If capability says `Unsupported`: "This agent doesn't support seamless substrate switching. <reason>. To start fresh, delete the session and create a new one."
- Update existing banner at `home/input.rs:1125`: append "Press Shift+S to switch to terminal mode."
- Action handler: spawn async task → `switch::switch_to_*`. Surface result via `SetTransientStatus`. Refresh session list.

## Task 3.2 — CLI

**File: `src/cli/cockpit.rs`** (extend)

```rust
pub enum CockpitCommands {
    // ... existing variants ...
    /// Switch a session to cockpit mode (open in web dashboard).
    Switch {
        /// Session id, prefix, or title.
        session: String,
        /// Target substrate.
        #[arg(long, value_enum)]
        to: SubstrateArg,
        /// Skip the destructive-action warning dialog.
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Copy, Clone, ValueEnum)]
pub enum SubstrateArg { Cockpit, Tmux }
```

Reuses `cockpit::switch::*`. Lookup via `crate::cli::resolve_session`. Interactive prompt by default unless `--yes`.

For destructive reset (PR 4 territory; deferred):
```rust
// Future:
// /// Reset a session in the target substrate (destructive; agent context lost).
// Reset { session: String, #[arg(long, value_enum)] r#in: SubstrateArg, #[arg(long)] yes: bool },
```

Regenerate `docs/cli/reference.md` via `cargo xtask gen-docs`.

## Task 3.3 — Web frontend

**Files**:
- `web/src/lib/acpCapableTools.ts`: remove static set. Replace with hook reading `substrate_capabilities` from `/api/about`.
- `web/src/components/cockpit/SwitchSubstrateAction.tsx`: rewrite dialog. Drop destructive warning. Show `Unsupported` state with reason from capability. Handle `409 Conflict` → toast "Session is processing a turn; try again when idle."
- `web/src/lib/cockpitTypes.ts`: handle `Stopped { reason: "substrate_switch" }` → new flag `workerSwitchingSubstrate`.
- `web/src/components/cockpit/CockpitView.tsx`: render transient "Switching substrate…" banner (no Reconnect button). Cleared on next `AcpSessionAssigned` / `UserPromptSent` (parallels `restart_pending`).

## Task 3.4 — New-session dialog cockpit toggle

**File: `src/tui/dialogs/new_session/`**:
- Add `cockpit_mode` + `cockpit_agent` fields on `NewSessionData`.
- Visibility gate via `capabilities::resolve_for_tool(tool).acp_available`.
- Default mirrors `aoe add` (`cli/add.rs:407-416`).

## Task 3.5 — Docs

**Files**:
- `docs/cockpit.md`: new section "Switching Between Substrates" — describes the round-trip, per-agent support matrix (from probe), CLI/TUI/Web entry points.
- `docs/cli/reference.md`: regenerated.
- TUI keybinds doc (if exists): document Shift+S.

## Task 3.6 — E2E test

**File: `tests/e2e/substrate_switch.rs`** (new)

- Spawn `aoe serve --no-auth` background.
- Create cockpit session via `aoe add --cockpit`.
- Send prompt via HTTP.
- Run `aoe cockpit switch <id> --to tmux --yes`.
- Assert `cockpit_mode = false` in storage.
- Assert tmux session exists.
- Run `aoe cockpit switch <id> --to cockpit --yes`.
- Assert worker spawned + transcript intact.

## PR 3 Acceptance

- Shift+S in TUI does round-trip switch.
- `aoe cockpit switch --to tmux/cockpit` works.
- Web shows correct capability-aware messaging.
- All three surfaces refuse cleanly on unsupported configurations.

---

# Out of scope (later)

- **Destructive `Restart` verb** (`aoe cockpit reset --in <substrate>`): explicit fresh-start path for unsupported agents. Adds separate UX. PR 4.
- **Durable queueing** (`busy_policy=queue_when_idle`, 202 Accepted): protocol shape leaves room. PR 5.
- **Context-handoff fallback** (synthetic prompt injection with summarized transcript): lossy, requires safety review. PR 6 if product wants it.
- **Native-import tmux→cockpit promotion** (adapter loads native CLI session id directly): only if probe proves adapter supports it for a tool.

# Risks

1. **`claude-agent-acp` native-artifact emission unverified**: gemini argued it uses Anthropic SDK directly, doesn't write to `~/.claude/projects/`. If probe confirms: cockpit→tmux for Claude is `Unsupported`. Feature usable only for tmux→cockpit→tmux→cockpit roundtrip starting from cockpit (preserves ACP id). Critical to run probe before locking capability defaults.
2. **Tmux status detection reliability for tmux→cockpit in-flight check**: status heuristic is per-agent. False positives ("idle when busy") → switch interrupts mid-turn. Conservative: refuse on `Running` status.
3. **Reconciler vs switch race**: switch_to_cockpit mutates `cockpit_mode = true`; reconciler reads at next tick. Brief window where state is "wants cockpit but no worker yet". UI shows "Switching…" banner. Acceptable.
4. **Stop-intent file leak**: CLI crashes between `mark_stop_intent` and registry delete. Defensive: take_stop_intent clears all intent files for the session when consuming one.

# Decisions surfaced for user

All three debaters argued **single PR is rejected; split mandatory**. User had earlier said "single PR unless good reason." All three LLMs agree it's a good reason. Plan above splits to 3 PRs. Confirm acceptable.

All three debaters argued **destructive `--force` does NOT belong inside `switch`**. Separate `reset` verb deferred to later PR. v1 refuses unsupported switches with structured error. Confirm acceptable.

**Empirical probe is a hard prerequisite** before PR 2 lands. Probe results determine which directions are marked Exact vs Unsupported. Cannot guess at adapter behavior. Confirm acceptable to gate PR 2 on probe completion.
