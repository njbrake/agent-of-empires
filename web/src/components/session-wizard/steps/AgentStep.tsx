import { useCallback, useState } from "react";
import type { AgentInfo, ProfileInfo } from "../../../lib/types";
import { getSettings } from "../../../lib/api";

interface WizardData {
  tool: string;
  title: string;
  profile: string;
  profileDirty: boolean;
  sandboxEnabled: boolean;
  yoloMode: boolean;
  advancedEnabled: boolean;
  sandboxImage: string;
  extraEnv: string[];
  customInstruction: string;
  extraArgs: string;
  commandOverride: string;
  group: string;
  [key: string]: unknown;
}

interface Props {
  data: WizardData;
  onChange: (field: string, value: unknown) => void;
  agents: AgentInfo[];
  profiles: ProfileInfo[];
  dockerAvailable: boolean;
  onApplyProfileDefaults: (defaults: { yoloMode: boolean; sandboxEnabled: boolean; tool: string; extraEnv: string[] }) => void;
}

function Toggle({ checked, onChange, disabled }: { checked: boolean; onChange: (v: boolean) => void; disabled?: boolean }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      onClick={() => !disabled && onChange(!checked)}
      className={`relative inline-flex h-7 w-12 shrink-0 items-center rounded-full transition-colors duration-200 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-brand-600 ${
        disabled ? "opacity-40 cursor-not-allowed" : "cursor-pointer"
      } ${checked ? "bg-brand-600" : "bg-surface-700"}`}
    >
      <span
        className={`inline-block h-5 w-5 rounded-full bg-white shadow-sm transition-transform duration-200 ${
          checked ? "translate-x-6" : "translate-x-1"
        }`}
      />
    </button>
  );
}

export function AgentStep({ data, onChange, agents, profiles, dockerAvailable, onApplyProfileDefaults }: Props) {
  const installedAgents = agents.filter((a) => a.installed);
  const selectedAgent = agents.find((a) => a.name === data.tool);
  const isHostOnly = selectedAgent?.host_only ?? false;
  const [showAdvanced, setShowAdvanced] = useState(data.advancedEnabled);
  const showProfilePicker = profiles.length > 1;

  const handleProfileChange = useCallback(async (profileName: string) => {
    // If user had manual edits, confirm before overwriting
    if (data.profileDirty && profileName) {
      const ok = window.confirm("Selecting a profile will reset your settings to that profile's defaults. Continue?");
      if (!ok) return;
    }

    onChange("profile", profileName);

    if (!profileName) return;

    // Load profile-resolved settings (global + profile overrides merged)
    try {
      const settings = await getSettings(profileName);
      if (settings) {
        const session = settings.session as Record<string, unknown> | undefined;
        const sandbox = settings.sandbox as Record<string, unknown> | undefined;
        // Pre-populate sandbox env from the profile so the user can see and edit
        // it before submission; without this, an empty extra_env is sent and the
        // backend falls back to the wrong (globally-default) profile's env vars.
        const env = Array.isArray(sandbox?.environment)
          ? (sandbox.environment as unknown[]).filter((v): v is string => typeof v === "string")
          : [];
        onApplyProfileDefaults({
          yoloMode: (session?.yolo_mode_default as boolean) ?? false,
          sandboxEnabled: (sandbox?.enabled_by_default as boolean) ?? false,
          tool: (session?.default_tool as string) || data.tool,
          extraEnv: env,
        });
      }
    } catch {
      // If we can't load profile settings, just set the profile name
    }
  }, [data.profileDirty, data.tool, onChange, onApplyProfileDefaults]);

  return (
    <div>
      <h2 className="text-lg font-semibold text-text-primary mb-1">Which AI agent?</h2>
      <p className="text-sm text-text-muted mb-5">Pick the coding assistant and configure your session.</p>

      {/* No agents installed */}
      {installedAgents.length === 0 && agents.length > 0 && (
        <div className="mb-5 p-4 rounded-lg border border-status-warning/30 bg-status-warning/5">
          <p className="text-sm font-semibold text-status-warning mb-2">No agents installed</p>
          <p className="text-sm text-text-muted mb-3">
            Install at least one AI coding agent to create a session.
          </p>
          <div className="space-y-1.5">
            {agents.filter((a) => ["claude", "codex", "gemini"].includes(a.name)).map((agent) => (
              <div key={agent.name} className="flex items-baseline gap-2">
                <span className="text-sm font-medium text-text-primary w-20">{agent.name}</span>
                <code className="text-xs text-text-dim font-mono">{agent.install_hint}</code>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Agent picker */}
      <div className="grid grid-cols-2 gap-2 mb-5">
        {installedAgents.map((agent) => (
          <button
            key={agent.name}
            onClick={() => onChange("tool", agent.name)}
            className={`text-left p-3 rounded-lg border transition-colors cursor-pointer focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-brand-600 ${
              data.tool === agent.name
                ? "border-brand-600 bg-surface-900"
                : "border-surface-700 bg-surface-950 hover:border-surface-600"
            }`}
          >
            <div className="text-sm font-semibold text-text-primary">{agent.name}</div>
          </button>
        ))}
      </div>

      {/* Profile selector */}
      {showProfilePicker && (
        <div className="mb-5">
          <label className="block text-sm text-text-dim mb-1.5">Profile</label>
          <select
            value={data.profile}
            onChange={(e) => handleProfileChange(e.target.value)}
            className="w-full bg-surface-900 border border-surface-700 rounded-lg px-3 py-2.5 text-sm text-text-primary focus:border-brand-600 focus:outline-none cursor-pointer"
          >
            <option value="">Server default</option>
            {profiles.map((p) => (
              <option key={p.name} value={p.name}>
                {p.name}{p.is_default ? " (active)" : ""}
              </option>
            ))}
          </select>
          {data.profile && data.profileDirty && (
            <p className="text-xs text-brand-500 mt-1">(Custom) Settings differ from profile defaults</p>
          )}
        </div>
      )}

      {/* Core toggles */}
      <div className="space-y-2 mb-4">
        <label
          className="flex items-center justify-between gap-3 p-3 bg-surface-900 border border-surface-700 rounded-lg cursor-pointer"
          onClick={() => !(isHostOnly || !dockerAvailable) && onChange("sandboxEnabled", !data.sandboxEnabled)}
        >
          <div className="flex-1">
            <div className="text-sm font-medium text-text-primary">Run in a safe container</div>
            <div className="text-xs text-text-dim mt-0.5 leading-snug">
              {!dockerAvailable
                ? "Docker is not running. Install or start Docker to use containers."
                : "Isolate the agent so it can't affect your system"}
            </div>
          </div>
          <Toggle
            checked={data.sandboxEnabled}
            onChange={(v) => onChange("sandboxEnabled", v)}
            disabled={isHostOnly || !dockerAvailable}
          />
        </label>

        <label
          className="flex items-center justify-between gap-3 p-3 bg-surface-900 border border-surface-700 rounded-lg cursor-pointer"
          onClick={() => onChange("yoloMode", !data.yoloMode)}
        >
          <div className="flex-1">
            <div className="text-sm font-medium text-text-primary">Auto-approve actions</div>
            <div className="text-xs text-text-dim mt-0.5 leading-snug">Let the agent run commands without asking. Faster, less safe.</div>
          </div>
          <Toggle checked={data.yoloMode} onChange={(v) => onChange("yoloMode", v)} />
        </label>
      </div>

      {isHostOnly && (
        <p className="text-xs text-status-warning mt-3 mb-3">{selectedAgent?.name} can only run on the host. Container option is disabled.</p>
      )}

      {/* Advanced settings (collapsible) */}
      <button
        onClick={() => { setShowAdvanced(!showAdvanced); if (!showAdvanced) onChange("advancedEnabled", true); }}
        className="flex items-center gap-2 text-sm text-text-dim hover:text-text-secondary py-2 cursor-pointer w-full"
      >
        <svg className={`w-3 h-3 transition-transform ${showAdvanced ? "rotate-90" : ""}`} viewBox="0 0 12 12" fill="currentColor">
          <path d="M4.5 2l4.5 4-4.5 4" stroke="currentColor" strokeWidth="1.5" fill="none" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
        Advanced settings
      </button>

      {showAdvanced && (
        <div className="mt-2 space-y-4 border-t border-surface-700/30 pt-4">
          {/* Session name / branch */}
          <div>
            <label className="block text-sm text-text-dim mb-1.5">Session name</label>
            <input
              type="text"
              value={data.title}
              onChange={(e) => onChange("title", e.target.value)}
              placeholder="Auto-generated if empty"
              className="w-full bg-surface-900 border border-surface-700 rounded-lg px-3 py-2.5 text-base font-mono text-text-primary placeholder:text-text-dim focus:border-brand-600 focus:outline-none"
            />
            <p className="text-xs text-text-dim mt-1">Used as the git branch name. Leave empty for auto-generated name.</p>
          </div>

          {/* Container config (if sandbox enabled) */}
          {data.sandboxEnabled && (
            <>
              <div>
                <label className="block text-sm text-text-dim mb-1.5">Container image</label>
                <input
                  type="text"
                  value={data.sandboxImage}
                  onChange={(e) => onChange("sandboxImage", e.target.value)}
                  placeholder="ghcr.io/njbrake/aoe-sandbox:latest"
                  className="w-full bg-surface-900 border border-surface-700 rounded-lg px-3 py-2.5 text-sm font-mono text-text-primary placeholder:text-text-dim focus:border-brand-600 focus:outline-none"
                />
              </div>
              <div>
                <label className="block text-sm text-text-dim mb-1.5">Environment variables</label>
                {data.extraEnv.map((env, i) => (
                  <div key={i} className="flex gap-2 mb-1">
                    <input
                      type="text"
                      value={env}
                      onChange={(e) => {
                        const updated = [...data.extraEnv];
                        updated[i] = e.target.value;
                        onChange("extraEnv", updated);
                      }}
                      placeholder="KEY=value"
                      className="flex-1 bg-surface-900 border border-surface-700 rounded-md px-2 py-1.5 text-sm font-mono text-text-primary placeholder:text-text-dim focus:border-brand-600 focus:outline-none"
                    />
                    <button
                      onClick={() => onChange("extraEnv", data.extraEnv.filter((_, j) => j !== i))}
                      className="px-2 text-text-dim hover:text-status-error cursor-pointer"
                    >&times;</button>
                  </div>
                ))}
                <button
                  onClick={() => onChange("extraEnv", [...data.extraEnv, ""])}
                  className="text-xs text-text-dim hover:text-text-secondary cursor-pointer"
                >+ Add variable</button>
              </div>
            </>
          )}

          {/* Custom instruction */}
          <div>
            <label className="block text-sm text-text-dim mb-1.5">Agent instructions</label>
            <textarea
              value={data.customInstruction}
              onChange={(e) => onChange("customInstruction", e.target.value)}
              placeholder="Custom instructions for this session..."
              rows={3}
              className="w-full bg-surface-900 border border-surface-700 rounded-lg px-3 py-2 text-sm text-text-primary placeholder:text-text-dim focus:border-brand-600 focus:outline-none resize-y"
            />
          </div>

          {/* Extra args */}
          <div>
            <label className="block text-sm text-text-dim mb-1.5">Additional arguments</label>
            <input
              type="text"
              value={data.extraArgs}
              onChange={(e) => onChange("extraArgs", e.target.value)}
              placeholder="e.g. --port 8080"
              className="w-full bg-surface-900 border border-surface-700 rounded-lg px-3 py-2.5 text-sm font-mono text-text-primary placeholder:text-text-dim focus:border-brand-600 focus:outline-none"
            />
          </div>

          {/* Command override */}
          <div>
            <label className="block text-sm text-text-dim mb-1.5">Command override</label>
            <input
              type="text"
              value={data.commandOverride}
              onChange={(e) => onChange("commandOverride", e.target.value)}
              placeholder="Override the agent launch command"
              className="w-full bg-surface-900 border border-surface-700 rounded-lg px-3 py-2.5 text-sm font-mono text-text-primary placeholder:text-text-dim focus:border-brand-600 focus:outline-none"
            />
          </div>

          {/* Group */}
          <div>
            <label className="block text-sm text-text-dim mb-1.5">Group</label>
            <input
              type="text"
              value={data.group}
              onChange={(e) => onChange("group", e.target.value)}
              placeholder="Optional session group"
              className="w-full bg-surface-900 border border-surface-700 rounded-lg px-3 py-2.5 text-sm font-mono text-text-primary placeholder:text-text-dim focus:border-brand-600 focus:outline-none"
            />
          </div>
        </div>
      )}
    </div>
  );
}
