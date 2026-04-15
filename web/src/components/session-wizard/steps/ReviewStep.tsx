import type { StepDef, StepId } from "../StepIndicator";

interface WizardData { path: string; title: string; group: string; tool: string; yoloMode: boolean; sandboxEnabled: boolean; sandboxImage: string; extraArgs: string; customInstruction: string; commandOverride: string; [key: string]: unknown; }
interface Props { data: WizardData; isSubmitting: boolean; error: string | null; onSubmit: () => void; onJumpTo: (stepId: StepId) => void; steps: StepDef[]; }

function Row({ label, value, stepId, onJumpTo, accent }: { label: string; value: string; stepId?: StepId; onJumpTo?: (id: StepId) => void; accent?: boolean }) {
  const interactive = stepId && onJumpTo;
  return (
    <button
      type="button"
      onClick={() => interactive && onJumpTo(stepId)}
      disabled={!interactive}
      className={`flex justify-between items-center w-full py-3 border-b border-surface-800 last:border-0 text-left ${
        interactive ? "cursor-pointer hover:bg-surface-800/50 -mx-2 px-2 rounded-md" : "-mx-2 px-2"
      }`}
    >
      <span className="text-sm text-text-dim">{label}</span>
      <span className={`text-sm font-mono truncate ml-4 ${accent ? "text-accent-600" : "text-text-primary"}`}>{value}</span>
    </button>
  );
}

export function ReviewStep({ data, isSubmitting, error, onSubmit, onJumpTo, steps }: Props) {
  const hasStep = (id: StepId) => steps.some((s) => s.id === id);
  return (
    <div>
      <h2 className="text-lg font-semibold text-text-primary mb-1">Review & Launch</h2>
      <p className="text-sm text-text-muted mb-5">Here's what will be created. Make sure everything looks right.</p>
      <div className="bg-surface-900 border border-surface-700 rounded-lg p-4 mb-5">
        <Row label="Project" value={data.path || "(not set)"} stepId="project" onJumpTo={onJumpTo} />
        {data.title && (
          <Row label="Branch" value={data.title} stepId="agent" onJumpTo={onJumpTo} accent />
        )}
        <Row label="Agent" value={data.tool || "(not set)"} stepId="agent" onJumpTo={onJumpTo} />
        {data.sandboxEnabled && (
          <Row label="Container" value={data.sandboxImage || "default"} stepId={hasStep("container") ? "container" : undefined} onJumpTo={onJumpTo} />
        )}
        <Row label="Auto-approve" value={data.yoloMode ? "On" : "Off"} stepId="agent" onJumpTo={onJumpTo} />
        {data.group && <Row label="Group" value={data.group} />}
        {data.extraArgs && <Row label="Extra args" value={data.extraArgs} />}
        {data.customInstruction && <Row label="Instructions" value="(set)" />}
        {data.commandOverride && <Row label="Command override" value={data.commandOverride} />}
      </div>
      {error && <div className="text-sm text-red-400 bg-red-400/10 rounded-lg p-3 mb-4">{error}</div>}
      <button
        onClick={onSubmit}
        disabled={isSubmitting || !data.path || !data.tool}
        className={`w-full py-3 rounded-lg font-semibold text-sm transition-colors focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-green-500 ${
          isSubmitting || !data.path || !data.tool
            ? "bg-green-500/50 text-surface-900/50 cursor-not-allowed"
            : "bg-green-500 hover:bg-green-600 active:bg-green-700 text-surface-900 cursor-pointer"
        }`}
      >
        {isSubmitting ? (
          <span className="flex items-center justify-center gap-2">
            <svg className="animate-spin h-4 w-4" viewBox="0 0 24 24"><circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" fill="none" /><path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" /></svg>
            Creating session...
          </span>
        ) : "Launch session"}
      </button>
    </div>
  );
}
