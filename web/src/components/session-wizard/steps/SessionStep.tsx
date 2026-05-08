interface WizardData {
  title: string;
  worktreeBranch: string;
  useWorktree: boolean;
  group: string;
  tool: string;
  [key: string]: unknown;
}

interface Props {
  data: WizardData;
  onChange: (field: string, value: unknown) => void;
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

export function SessionStep({ data, onChange }: Props) {
  return (
    <div>
      <h2 className="text-lg font-semibold text-text-primary mb-1">Name your session</h2>
      <p className="text-sm text-text-muted mb-5">Give it a title and decide whether to work in a git worktree.</p>

      <div className="mb-5">
        <label className="block text-sm text-text-dim mb-1.5">Session title</label>
        <input
          type="text"
          value={data.title}
          onChange={(e) => onChange("title", e.target.value)}
          placeholder="Auto-generated if empty"
          className="w-full bg-surface-900 border border-surface-700 rounded-lg px-3 py-2.5 text-base font-mono text-text-primary placeholder:text-text-dim focus:border-brand-600 focus:outline-none"
        />
        <p className="text-xs text-text-dim mt-1">Shown in the dashboard. Renaming it later does not rename the git branch.</p>
      </div>

      <label
        className="flex items-center justify-between gap-3 p-3 bg-surface-900 border border-surface-700 rounded-lg cursor-pointer mb-3"
        onClick={() => onChange("useWorktree", !data.useWorktree)}
      >
        <div className="flex-1">
          <div className="text-sm font-medium text-text-primary">Create a worktree</div>
          <div className="text-xs text-text-dim mt-0.5 leading-snug">
            Run the agent in a new git worktree branched off the current HEAD. Off = run directly in the repo folder.
          </div>
        </div>
        <Toggle
          checked={data.useWorktree}
          onChange={(v) => onChange("useWorktree", v)}
        />
      </label>

      {data.useWorktree && (
        <div className="mb-5">
          <label className="block text-sm text-text-dim mb-1.5">Branch / worktree name</label>
          <input
            type="text"
            value={data.worktreeBranch}
            onChange={(e) => onChange("worktreeBranch", e.target.value)}
            placeholder="Uses session title if empty"
            className="w-full bg-surface-900 border border-surface-700 rounded-lg px-3 py-2.5 text-base font-mono text-text-primary placeholder:text-text-dim focus:border-brand-600 focus:outline-none"
          />
          <p className="text-xs text-text-dim mt-1">The branch name is also the worktree directory name. Leave blank to use the session title.</p>
        </div>
      )}

      <div>
        <label className="block text-sm text-text-dim mb-1.5">Group</label>
        <input
          type="text"
          value={data.group}
          onChange={(e) => onChange("group", e.target.value)}
          placeholder="Optional, for organizing related sessions"
          className="w-full bg-surface-900 border border-surface-700 rounded-lg px-3 py-2.5 text-sm font-mono text-text-primary placeholder:text-text-dim focus:border-brand-600 focus:outline-none"
        />
      </div>
    </div>
  );
}
