import { useEffect, useState } from "react";
import { fetchAgents } from "../lib/api";
import type { AgentInfo } from "../lib/types";

interface Props {
  knownPaths: string[];
  onSubmit: (data: CreateWorkspaceData) => Promise<void> | void;
  onCancel: () => void;
}

export interface CreateWorkspaceData {
  title?: string;
  path: string;
  tool: string;
  group: string;
  yolo_mode: boolean;
  worktree_branch?: string;
  create_new_branch: boolean;
  sandbox: boolean;
  extra_args: string;
}

const INPUT_CLASS =
  "w-full bg-surface-800 border border-surface-700 rounded-md px-3 py-1.5 font-body text-sm text-text-primary placeholder:text-text-dim focus:border-brand-600 focus:outline-none";
const INPUT_CLASS_MONO =
  "w-full bg-surface-800 border border-surface-700 rounded-md px-3 py-2 font-mono text-sm text-text-primary placeholder:text-text-dim focus:border-brand-600 focus:outline-none";

export function CreateWorkspaceModal({
  knownPaths,
  onSubmit,
  onCancel,
}: Props) {
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [path, setPath] = useState("");
  const [title, setTitle] = useState("");
  const [tool, setTool] = useState("claude");
  const [group, setGroup] = useState("");
  const [yolo, setYolo] = useState(false);
  const [branch, setBranch] = useState("");
  const [newBranch, setNewBranch] = useState(false);
  const [sandbox, setSandbox] = useState(false);
  const [extraArgs, setExtraArgs] = useState("");
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showPathDropdown, setShowPathDropdown] = useState(false);

  useEffect(() => {
    fetchAgents().then((a) => {
      setAgents(a);
      const first = a[0];
      if (first) setTool(first.name);
    });
  }, []);

  const filteredPaths = path.trim()
    ? knownPaths.filter((p) =>
        p.toLowerCase().includes(path.toLowerCase()),
      )
    : knownPaths;

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!path.trim()) return;
    setSubmitting(true);
    setError(null);
    try {
      await onSubmit({
        title: title.trim() || undefined,
        path: path.trim(),
        tool,
        group: group.trim(),
        yolo_mode: yolo,
        worktree_branch: branch.trim() || undefined,
        create_new_branch: newBranch,
        sandbox,
        extra_args: extraArgs.trim(),
      });
    } catch {
      setError("Failed to create session");
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div
      className="fixed inset-0 bg-black/60 flex items-center justify-center z-50 animate-fade-in"
      onClick={(e) => {
        if (e.target === e.currentTarget) onCancel();
      }}
    >
      <form
        onSubmit={handleSubmit}
        className="w-[480px] max-w-[90vw] bg-surface-900 border border-surface-700 rounded-xl shadow-2xl"
        onClick={() => setShowPathDropdown(false)}
      >
        {/* Header */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-surface-700">
          <h2 className="font-display text-base font-semibold text-text-bright">
            New Session
          </h2>
          <button
            type="button"
            onClick={onCancel}
            className="text-text-muted hover:text-text-secondary cursor-pointer text-lg"
          >
            &times;
          </button>
        </div>

        <div className="p-5 space-y-4 max-h-[60vh] overflow-y-auto">
          {/* Project Path (combobox) */}
          <label className="block relative">
            <span className="font-mono text-[11px] uppercase tracking-wider text-text-dim block mb-1">
              Project Path *
            </span>
            <input
              type="text"
              value={path}
              onChange={(e) => {
                setPath(e.target.value);
                setShowPathDropdown(true);
              }}
              onFocus={() => setShowPathDropdown(true)}
              autoFocus
              placeholder="/path/to/your/project"
              className={INPUT_CLASS_MONO}
            />
            {showPathDropdown && filteredPaths.length > 0 && (
              <div className="absolute z-10 w-full mt-1 bg-surface-800 border border-surface-700 rounded-md shadow-lg max-h-32 overflow-y-auto">
                {filteredPaths.map((p) => (
                  <button
                    key={p}
                    type="button"
                    onClick={(e) => {
                      e.stopPropagation();
                      setPath(p);
                      setShowPathDropdown(false);
                    }}
                    className="w-full text-left px-3 py-1.5 font-mono text-sm text-text-secondary hover:bg-surface-700 cursor-pointer truncate"
                  >
                    {p}
                  </button>
                ))}
              </div>
            )}
          </label>

          {/* Branch */}
          <label className="block">
            <span className="font-mono text-[11px] uppercase tracking-wider text-text-dim block mb-1">
              Branch
              <span className="text-text-dim ml-1 normal-case tracking-normal">
                (creates worktree)
              </span>
            </span>
            <input
              type="text"
              value={branch}
              onChange={(e) => setBranch(e.target.value)}
              placeholder="feat/my-feature"
              className={INPUT_CLASS_MONO}
            />
            {branch && (
              <label className="flex items-center gap-2 mt-1.5 cursor-pointer">
                <input
                  type="checkbox"
                  checked={newBranch}
                  onChange={(e) => setNewBranch(e.target.checked)}
                  className="accent-brand-600"
                />
                <span className="font-body text-xs text-text-muted">
                  Create new branch
                </span>
              </label>
            )}
          </label>

          {/* Agent Grid */}
          <div>
            <span className="font-mono text-[11px] uppercase tracking-wider text-text-dim block mb-1.5">
              Agent
            </span>
            {agents.length === 0 ? (
              <div className="text-text-dim font-body text-xs py-2">
                Loading agents...
              </div>
            ) : (
              <div className="grid grid-cols-3 gap-1.5">
                {agents.map((a) => (
                  <button
                    key={a.name}
                    type="button"
                    onClick={() => setTool(a.name)}
                    className={`px-2 py-2 rounded-md text-xs font-body cursor-pointer transition-colors text-center ${
                      tool === a.name
                        ? "bg-brand-600/20 text-brand-500 border border-brand-600/40"
                        : "bg-surface-800 text-text-secondary border border-surface-700 hover:border-surface-700/80"
                    }`}
                  >
                    {a.name}
                  </button>
                ))}
              </div>
            )}
          </div>

          {/* Toggles */}
          <div className="flex gap-4">
            <label className="flex items-center gap-2 cursor-pointer">
              <input
                type="checkbox"
                checked={yolo}
                onChange={(e) => setYolo(e.target.checked)}
                className="accent-brand-600"
              />
              <span className="font-body text-xs text-text-secondary">
                YOLO mode
              </span>
            </label>
            <label className="flex items-center gap-2 cursor-pointer">
              <input
                type="checkbox"
                checked={sandbox}
                onChange={(e) => setSandbox(e.target.checked)}
                className="accent-brand-600"
              />
              <span className="font-body text-xs text-text-secondary">
                Sandbox
              </span>
            </label>
          </div>

          {/* Advanced */}
          <button
            type="button"
            onClick={() => setShowAdvanced(!showAdvanced)}
            className="font-body text-xs text-text-dim hover:text-text-muted cursor-pointer"
          >
            {showAdvanced ? "Hide" : "Show"} advanced options
          </button>

          {showAdvanced && (
            <div className="space-y-3 border-t border-surface-700 pt-3">
              <label className="block">
                <span className="font-mono text-[11px] uppercase tracking-wider text-text-dim block mb-1">
                  Title
                </span>
                <input
                  type="text"
                  value={title}
                  onChange={(e) => setTitle(e.target.value)}
                  placeholder="Auto-generated if empty"
                  className={INPUT_CLASS}
                />
              </label>
              <label className="block">
                <span className="font-mono text-[11px] uppercase tracking-wider text-text-dim block mb-1">
                  Group
                </span>
                <input
                  type="text"
                  value={group}
                  onChange={(e) => setGroup(e.target.value)}
                  placeholder="work/projects"
                  className={INPUT_CLASS}
                />
              </label>
              <label className="block">
                <span className="font-mono text-[11px] uppercase tracking-wider text-text-dim block mb-1">
                  Extra Args
                </span>
                <input
                  type="text"
                  value={extraArgs}
                  onChange={(e) => setExtraArgs(e.target.value)}
                  placeholder="--resume abc123"
                  className={INPUT_CLASS_MONO}
                />
              </label>
            </div>
          )}

          {error && (
            <p className="font-body text-xs text-status-error">{error}</p>
          )}
        </div>

        {/* Footer */}
        <div className="flex justify-end gap-2 px-5 py-4 border-t border-surface-700">
          <button
            type="button"
            onClick={onCancel}
            className="px-4 py-2 font-body text-xs rounded-md text-text-secondary hover:bg-surface-800 transition-colors cursor-pointer"
          >
            Cancel
          </button>
          <button
            type="submit"
            disabled={!path.trim() || submitting}
            className="px-4 py-2 font-body text-xs rounded-md bg-brand-600 text-surface-950 font-semibold hover:bg-brand-700 transition-colors cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed"
          >
            {submitting ? "Creating..." : "Create Session"}
          </button>
        </div>
      </form>
    </div>
  );
}
