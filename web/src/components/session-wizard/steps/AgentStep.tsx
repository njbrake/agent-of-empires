import type { AgentInfo } from "../../../lib/types";

interface WizardData {
  tool: string;
  title: string;
  sandboxEnabled: boolean;
  yoloMode: boolean;
  advancedEnabled: boolean;
  [key: string]: unknown;
}

interface Props {
  data: WizardData;
  onChange: (field: string, value: unknown) => void;
  agents: AgentInfo[];
  dockerAvailable: boolean;
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

export function AgentStep({ data, onChange, agents, dockerAvailable }: Props) {
  const installedAgents = agents.filter((a) => a.installed);
  const selectedAgent = agents.find((a) => a.name === data.tool);
  const isHostOnly = selectedAgent?.host_only ?? false;

  return (
    <div>
      <h2 className="text-lg font-semibold text-text-primary mb-1">Which AI agent?</h2>
      <p className="text-sm text-text-muted mb-5">Pick the coding assistant you want to use.</p>

      <div className="grid grid-cols-2 gap-2 mb-6">
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
            <div className="text-xs text-text-dim mt-0.5 leading-snug">{agent.description}</div>
          </button>
        ))}
      </div>

      {/* Session name = branch name (every web session is a worktree) */}
      <div className="mb-5">
        <label className="block text-sm text-text-dim mb-1.5">Session name</label>
        <input
          type="text"
          value={data.title}
          onChange={(e) => onChange("title", e.target.value)}
          placeholder="e.g. feature/add-auth, fix/login-bug"
          className="w-full bg-surface-900 border border-surface-700 rounded-lg px-3 py-2.5 text-base font-mono text-text-primary placeholder:text-text-dim focus:border-brand-600 focus:outline-none"
        />
        <p className="text-xs text-text-dim mt-1.5">This will be used as the git branch name.</p>
      </div>

      <div className="space-y-2">
        <label
          className="flex items-center justify-between gap-3 p-3 bg-surface-900 border border-surface-700 rounded-lg cursor-pointer"
          onClick={() => !(isHostOnly || !dockerAvailable) && (() => { onChange("sandboxEnabled", !data.sandboxEnabled); if (!data.sandboxEnabled) onChange("advancedEnabled", true); })()}
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
            onChange={(v) => { onChange("sandboxEnabled", v); if (v) onChange("advancedEnabled", true); }}
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
        <p className="text-xs text-status-warning mt-3">{selectedAgent?.name} can only run on the host. Container option is disabled.</p>
      )}

      {data.sandboxEnabled && (
        <p className="text-xs text-accent-600 mt-3">
          A new step will appear to configure your container setup.
        </p>
      )}

      {!data.advancedEnabled && (
        <button
          onClick={() => onChange("advancedEnabled", true)}
          className="text-sm text-text-dim hover:text-text-secondary py-2 mt-2 cursor-pointer"
        >
          More options...
        </button>
      )}
    </div>
  );
}
