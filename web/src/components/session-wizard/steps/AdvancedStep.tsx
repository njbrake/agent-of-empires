interface WizardData { customInstruction: string; extraArgs: string; commandOverride: string; [key: string]: unknown; }
interface Props { data: WizardData; onChange: (field: string, value: unknown) => void; }

export function AdvancedStep({ data, onChange }: Props) {
  return (
    <div>
      <h2 className="text-lg font-semibold text-text-primary mb-1">Extra options</h2>
      <p className="text-sm text-text-muted mb-5">Advanced configuration for the agent session.</p>
      <div className="mb-4">
        <label className="block text-sm text-text-dim mb-1.5">Agent instructions</label>
        <p className="text-xs text-text-muted mb-1.5">Extra instructions the agent sees when it starts. Like giving it a briefing.</p>
        <textarea value={data.customInstruction} onChange={(e) => onChange("customInstruction", e.target.value)}
          rows={4} placeholder="e.g. Focus on writing tests for the auth module..."
          className="w-full bg-surface-900 border border-surface-700 rounded-lg px-3 py-2.5 text-base text-text-primary placeholder:text-text-dim focus:border-brand-600 focus:outline-none resize-y" />
      </div>
      <div className="mb-4">
        <label className="block text-sm text-text-dim mb-1.5">Additional arguments</label>
        <p className="text-xs text-text-muted mb-1.5">Command-line flags passed to the agent. Leave empty unless you know what you need.</p>
        <input type="text" value={data.extraArgs} onChange={(e) => onChange("extraArgs", e.target.value)}
          placeholder="e.g. --resume abc123"
          className="w-full bg-surface-900 border border-surface-700 rounded-lg px-3 py-2.5 text-base font-mono text-text-primary placeholder:text-text-dim focus:border-brand-600 focus:outline-none" />
      </div>
      <div>
        <label className="block text-sm text-text-dim mb-1.5">Command override</label>
        <p className="text-xs text-text-muted mb-1.5">Replace the default agent binary entirely.</p>
        <input type="text" value={data.commandOverride} onChange={(e) => onChange("commandOverride", e.target.value)}
          placeholder="e.g. /usr/local/bin/my-claude-wrapper"
          className="w-full bg-surface-900 border border-surface-700 rounded-lg px-3 py-2.5 text-base font-mono text-text-primary placeholder:text-text-dim focus:border-brand-600 focus:outline-none" />
      </div>
    </div>
  );
}
