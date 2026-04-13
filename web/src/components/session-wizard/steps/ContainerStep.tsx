interface WizardData { sandboxImage: string; extraEnv: string[]; cpuLimit: string; memoryLimit: string; portMappings: string[]; mountSsh: boolean; volumeIgnores: string[]; extraVolumes: string[]; [key: string]: unknown; }
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
      <div className="grid grid-cols-2 gap-3 mb-4">
        <div>
          <label className="block text-sm text-text-dim mb-1.5">CPU limit</label>
          <input type="text" value={data.cpuLimit} onChange={(e) => onChange("cpuLimit", e.target.value)} placeholder="e.g. 2"
            className="w-full bg-surface-900 border border-surface-700 rounded-lg px-3 py-2.5 text-base font-mono text-text-primary placeholder:text-text-dim focus:border-brand-600 focus:outline-none" />
        </div>
        <div>
          <label className="block text-sm text-text-dim mb-1.5">Memory limit</label>
          <input type="text" value={data.memoryLimit} onChange={(e) => onChange("memoryLimit", e.target.value)} placeholder="e.g. 4g"
            className="w-full bg-surface-900 border border-surface-700 rounded-lg px-3 py-2.5 text-base font-mono text-text-primary placeholder:text-text-dim focus:border-brand-600 focus:outline-none" />
        </div>
      </div>
      <ListEditor label="Exposed ports" items={data.portMappings} onUpdate={(v) => onChange("portMappings", v)} placeholder="8080:8080" />
      <label
        className="flex items-center justify-between gap-3 p-3 bg-surface-900 border border-surface-700 rounded-lg mb-4 cursor-pointer"
        onClick={() => onChange("mountSsh", !data.mountSsh)}
      >
        <div>
          <div className="text-sm font-medium text-text-primary">Mount SSH keys</div>
          <div className="text-xs text-text-dim mt-0.5 leading-snug">Let the container use your SSH keys for git</div>
        </div>
        <button
          type="button" role="switch" aria-checked={data.mountSsh}
          onClick={(e) => { e.stopPropagation(); onChange("mountSsh", !data.mountSsh); }}
          className={`relative inline-flex h-7 w-12 shrink-0 items-center rounded-full transition-colors duration-200 cursor-pointer focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-brand-600 ${
            data.mountSsh ? "bg-brand-600" : "bg-surface-700"
          }`}
        >
          <span className={`inline-block h-5 w-5 rounded-full bg-white shadow-sm transition-transform duration-200 ${data.mountSsh ? "translate-x-6" : "translate-x-1"}`} />
        </button>
      </label>
      <ListEditor label="Excluded folders" items={data.volumeIgnores} onUpdate={(v) => onChange("volumeIgnores", v)} placeholder="node_modules" />
      <ListEditor label="Extra volumes" items={data.extraVolumes} onUpdate={(v) => onChange("extraVolumes", v)} placeholder="/host/path:/container/path" />
    </div>
  );
}
