//! Substrate switching capability model.
//!
//! Resolves whether a given tool can move a session between cockpit
//! (ACP) and tmux substrates non-destructively. Used by the substrate
//! switch coordinator (lands in PR 2) to refuse unsupported flips with
//! a structured reason, and surfaced via `/api/about` so the web UI
//! can render the right confirmation copy (or disable the button).
//!
//! Capability rules are conservative by default: a tool is marked
//! `Exact` only when we have evidence that the underlying agent
//! conversation survives the round trip. Evidence is the
//! `cockpit-probe` xtask harness (also new in PR 1) — its results
//! land in `<app_dir>/cockpit-probe-results.json` and override the
//! hardcoded defaults below. Until a probe runs, unverified directions
//! report `Unsupported` with a reason pointing at the probe.

use serde::{Deserialize, Serialize};

use super::agent_registry::AgentRegistry;
use crate::agents::ResumeStrategy;

/// Substrate a session can run in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Substrate {
    Cockpit,
    Tmux,
}

/// How much of the conversation survives a switch in a given
/// direction. `Exact` = same underlying agent session continues;
/// `Unsupported` = refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContinuityMode {
    Exact,
    Unsupported,
}

/// One direction of a substrate switch for a given tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectionalCapability {
    pub from: Substrate,
    pub to: Substrate,
    /// Whether the switch is permitted at all. `false` always pairs
    /// with `continuity == Unsupported` and a populated `reason`.
    pub supported: bool,
    pub continuity: ContinuityMode,
    /// True if the direction requires `Instance.cockpit_acp_session_id`
    /// to be set (tmux → cockpit, when relying on `session/load`).
    pub requires_acp_session_id: bool,
    /// True if the direction requires `Instance.agent_session_id` to be
    /// set (cockpit → tmux, when relying on the agent CLI's `--resume`).
    pub requires_agent_session_id: bool,
    /// Human-readable reason. `None` on supported directions, `Some`
    /// otherwise.
    pub reason: Option<String>,
}

/// Aggregate substrate capability for a tool. Built from the union of
/// the agent registry, the legacy `ResumeStrategy` table, and probe
/// results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCapabilities {
    pub tool: String,
    /// Registry name of the ACP adapter, if one exists. `None` for
    /// tools that have no ACP adapter at all (e.g. cursor, copilot).
    pub acp_agent_name: Option<String>,
    /// Whether an ACP adapter is registered for this tool. `false`
    /// disables both directions of substrate switching.
    pub acp_available: bool,
    /// Whether the ACP adapter advertises `agent_capabilities
    /// .load_session = true`. Required for tmux → cockpit non-
    /// destructive resume via `session/load`.
    pub load_session_capable: bool,
    /// Whether the agent CLI supports `--resume <session_id>` (or its
    /// equivalent per `ResumeStrategy`). Required for cockpit → tmux
    /// non-destructive resume.
    pub tmux_resume_strategy_present: bool,
    /// Whether ACP-mode operation produces a discoverable
    /// `agent_session_id` on disk (so cockpit → tmux has something to
    /// pass to `--resume`). Probe-verified per adapter.
    pub native_session_discoverable: bool,
    pub directions: Vec<DirectionalCapability>,
}

/// Build capability data for a single tool from the registry, legacy
/// resume table, and probe defaults.
pub fn resolve_for_tool(tool: &str) -> ToolCapabilities {
    let registry = AgentRegistry::with_defaults();
    let acp_agent_name = registry
        .get(tool)
        .filter(|_| !is_aoe_agent_fallback(tool))
        .map(|_| tool.to_string());
    let acp_available = acp_agent_name.is_some();

    let probe = probe_defaults_for(tool);
    let load_session_capable = probe.load_session_capable;
    let native_session_discoverable = probe.native_session_discoverable;

    let tmux_resume_strategy_present = crate::agents::get_agent(tool)
        .is_some_and(|a| !matches!(a.resume_strategy, ResumeStrategy::Unsupported));

    let directions = vec![
        cockpit_to_tmux(
            acp_available,
            tmux_resume_strategy_present,
            native_session_discoverable,
        ),
        tmux_to_cockpit(acp_available, load_session_capable),
    ];

    ToolCapabilities {
        tool: tool.to_string(),
        acp_agent_name,
        acp_available,
        load_session_capable,
        tmux_resume_strategy_present,
        native_session_discoverable,
        directions,
    }
}

/// Aggregate capabilities for every tool the agent table knows about.
/// Used to populate `/api/about` and the web wizard's substrate gate.
pub fn resolve_all() -> Vec<ToolCapabilities> {
    crate::agents::agent_names()
        .iter()
        .map(|n| resolve_for_tool(n))
        .collect()
}

fn is_aoe_agent_fallback(tool: &str) -> bool {
    // `aoe-agent` is a registry entry but not bound to a `tool` name in
    // `agents.rs`. Callers asking "can tool X switch substrates?" never
    // pass it; this guard is defensive.
    tool == "aoe-agent"
}

fn cockpit_to_tmux(
    acp_available: bool,
    tmux_resume_strategy_present: bool,
    native_session_discoverable: bool,
) -> DirectionalCapability {
    if !acp_available {
        return DirectionalCapability {
            from: Substrate::Cockpit,
            to: Substrate::Tmux,
            supported: false,
            continuity: ContinuityMode::Unsupported,
            requires_acp_session_id: false,
            requires_agent_session_id: false,
            reason: Some("no ACP adapter for this tool".into()),
        };
    }
    if !tmux_resume_strategy_present {
        return DirectionalCapability {
            from: Substrate::Cockpit,
            to: Substrate::Tmux,
            supported: false,
            continuity: ContinuityMode::Unsupported,
            requires_acp_session_id: false,
            requires_agent_session_id: false,
            reason: Some("agent CLI has no `--resume` mechanism".into()),
        };
    }
    if !native_session_discoverable {
        return DirectionalCapability {
            from: Substrate::Cockpit,
            to: Substrate::Tmux,
            supported: false,
            continuity: ContinuityMode::Unsupported,
            requires_acp_session_id: false,
            requires_agent_session_id: true,
            reason: Some(
                "ACP adapter doesn't emit a discoverable native session id; \
                 run `cargo xtask cockpit-probe` to verify"
                    .into(),
            ),
        };
    }
    DirectionalCapability {
        from: Substrate::Cockpit,
        to: Substrate::Tmux,
        supported: true,
        continuity: ContinuityMode::Exact,
        requires_acp_session_id: false,
        requires_agent_session_id: true,
        reason: None,
    }
}

fn tmux_to_cockpit(acp_available: bool, load_session_capable: bool) -> DirectionalCapability {
    if !acp_available {
        return DirectionalCapability {
            from: Substrate::Tmux,
            to: Substrate::Cockpit,
            supported: false,
            continuity: ContinuityMode::Unsupported,
            requires_acp_session_id: false,
            requires_agent_session_id: false,
            reason: Some("no ACP adapter for this tool".into()),
        };
    }
    if !load_session_capable {
        return DirectionalCapability {
            from: Substrate::Tmux,
            to: Substrate::Cockpit,
            supported: false,
            continuity: ContinuityMode::Unsupported,
            requires_acp_session_id: true,
            requires_agent_session_id: false,
            reason: Some(
                "ACP adapter doesn't advertise `load_session`; \
                 cockpit can't reattach to a prior conversation"
                    .into(),
            ),
        };
    }
    // tmux → cockpit only supported when the session already has a
    // cockpit_acp_session_id (was cockpit-mode at some point). Pure
    // tmux-origin promotion needs adapter-side native-id import,
    // which no current adapter advertises.
    DirectionalCapability {
        from: Substrate::Tmux,
        to: Substrate::Cockpit,
        supported: true,
        continuity: ContinuityMode::Exact,
        requires_acp_session_id: true,
        requires_agent_session_id: false,
        reason: None,
    }
}

/// Conservative hardcoded defaults until `cargo xtask cockpit-probe`
/// runs and writes `<app_dir>/cockpit-probe-results.json`. Once the
/// probe has run, those results override these values.
///
/// Today every entry returns `false` for `native_session_discoverable`
/// because no adapter has been empirically verified to emit a
/// discoverable native session id during ACP operation. The probe is
/// what flips that bit.
struct ProbeDefaults {
    load_session_capable: bool,
    native_session_discoverable: bool,
}

fn probe_defaults_for(tool: &str) -> ProbeDefaults {
    let loaded = load_probe_results();
    if let Some(per_tool) = loaded.and_then(|r| r.get(tool).cloned()) {
        return ProbeDefaults {
            load_session_capable: per_tool.load_session_capable.unwrap_or(false),
            native_session_discoverable: per_tool.native_session_discoverable.unwrap_or(false),
        };
    }
    // Hardcoded conservative seeds. `claude-agent-acp` is known to
    // advertise `load_session = true` (used by the cockpit resume path
    // in #1045); other adapters get `false` until a probe confirms.
    // Every entry stays `native_session_discoverable = false` so the
    // cockpit → tmux direction is `Unsupported` until proven.
    match tool {
        "claude" => ProbeDefaults {
            load_session_capable: true,
            native_session_discoverable: false,
        },
        _ => ProbeDefaults {
            load_session_capable: false,
            native_session_discoverable: false,
        },
    }
}

/// On-disk probe result for one tool. Optional fields so partial
/// probe runs don't lose unrelated data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeResult {
    pub load_session_capable: Option<bool>,
    pub native_session_discoverable: Option<bool>,
    /// When the probe was run; useful for staleness audits.
    pub probed_at: Option<String>,
    /// Adapter version observed by the probe.
    pub adapter_version: Option<String>,
}

/// Full on-disk shape of `<app_dir>/cockpit-probe-results.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProbeResults(pub std::collections::HashMap<String, ProbeResult>);

impl ProbeResults {
    pub fn get(&self, tool: &str) -> Option<&ProbeResult> {
        self.0.get(tool)
    }
}

/// Path the probe writes to and the resolver reads from.
pub fn probe_results_path() -> Option<std::path::PathBuf> {
    let dir = crate::session::get_app_dir().ok()?;
    Some(dir.join("cockpit-probe-results.json"))
}

fn load_probe_results() -> Option<ProbeResults> {
    let path = probe_results_path()?;
    let bytes = std::fs::read(&path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_tool_is_unsupported_both_directions() {
        let cap = resolve_for_tool("definitely-not-a-real-tool");
        assert!(!cap.acp_available);
        for d in &cap.directions {
            assert!(!d.supported);
            assert_eq!(d.continuity, ContinuityMode::Unsupported);
            assert!(d.reason.is_some());
        }
    }

    #[test]
    fn claude_tmux_to_cockpit_supported_by_default() {
        // Hardcoded probe seed marks claude `load_session_capable = true`.
        let cap = resolve_for_tool("claude");
        assert!(cap.acp_available);
        assert!(cap.load_session_capable);
        let t2c = cap
            .directions
            .iter()
            .find(|d| d.from == Substrate::Tmux && d.to == Substrate::Cockpit)
            .expect("expected tmux→cockpit direction");
        assert!(t2c.supported);
        assert_eq!(t2c.continuity, ContinuityMode::Exact);
        assert!(t2c.requires_acp_session_id);
    }

    #[test]
    fn claude_cockpit_to_tmux_unsupported_until_probe_proves_native_discovery() {
        // Until the probe writes `native_session_discoverable = true`
        // for claude, the cockpit → tmux direction must refuse so we
        // don't promise non-destructive switching we can't deliver.
        let cap = resolve_for_tool("claude");
        let c2t = cap
            .directions
            .iter()
            .find(|d| d.from == Substrate::Cockpit && d.to == Substrate::Tmux)
            .expect("expected cockpit→tmux direction");
        assert!(!c2t.supported);
        assert_eq!(c2t.continuity, ContinuityMode::Unsupported);
        assert!(c2t.reason.as_ref().unwrap().contains("native session id"));
    }

    #[test]
    fn resume_unsupported_tool_blocks_cockpit_to_tmux() {
        // Cursor has `ResumeStrategy::Unsupported`, so cockpit → tmux
        // must refuse even if some adapter shows up later.
        let cap = resolve_for_tool("cursor");
        let c2t = cap
            .directions
            .iter()
            .find(|d| d.from == Substrate::Cockpit && d.to == Substrate::Tmux)
            .unwrap();
        assert!(!c2t.supported);
    }
}
