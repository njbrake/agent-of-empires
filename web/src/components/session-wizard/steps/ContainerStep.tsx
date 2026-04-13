interface WizardData { sandboxImage: string; extraEnv: string[]; [key: string]: unknown; }
interface Props { data: WizardData; onChange: (field: string, value: unknown) => void; }

function ListEditor({ label, items, onUpdate, placeholder }: { label: string; items: string[]; onUpdate: (items: string[]) => void; placeholder: string }) {
  return (
    <div className="mb-4">
      <label className="block text-sm text-text-dim mb-1.5">{label}</label>
      {items.map((item, i) => (
        <div key={i} className="flex gap-2 mb-2">
          <input type="text" value={item} onChange={(e) => { const u = [...items]; u[i] = e.target.value; onUpdate(u); }} placeholder={placeholder}
            className="flex-1 bg-surface-900 border border-surface-700 rounded-lg px-3 py-2.5 text-base font-mono text-text-primary placeholder:text-text-dim focus:border-brand-600 focus:outline-none" />
          <button onClick={() => onUpdate(items.filter((_, j) => j !== i))}
            className="text-sm text-text-dim hover:text-red-400 px-3 py-2 rounded-lg hover:bg-surface-900 cursor-pointer transition-colors"
            aria-label="Remove">&times;</button>
        </div>
      ))}
      <button onClick={() => onUpdate([...items, ""])} className="text-sm text-text-dim hover:text-text-secondary py-2 cursor-pointer">+ Add</button>
    </div>
  );
}

export function ContainerStep({ data, onChange }: Props) {
  return (
    <div>
      <h2 className="text-lg font-semibold text-text-primary mb-1">Container setup</h2>
      <div className="bg-surface-900 border border-surface-700 rounded-lg p-3 mb-5">
        <p className="text-sm text-text-muted leading-relaxed">A container is like a sealed room where the agent works. It can see your code but can't change anything else on your computer.</p>
      </div>
      <div className="mb-4">
        <label className="block text-sm text-text-dim mb-1.5">Docker image</label>
        <input type="text" value={data.sandboxImage} onChange={(e) => onChange("sandboxImage", e.target.value)}
          className="w-full bg-surface-900 border border-surface-700 rounded-lg px-3 py-2.5 text-base font-mono text-text-primary focus:border-brand-600 focus:outline-none" />
      </div>
      <ListEditor label="Environment variables" items={data.extraEnv} onUpdate={(v) => onChange("extraEnv", v)} placeholder="KEY=value or KEY" />
    </div>
  );
}
