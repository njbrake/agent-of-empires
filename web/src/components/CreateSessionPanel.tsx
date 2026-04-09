import { useEffect, useState } from "react";
import { fetchAgents } from "../lib/api";
import type { AgentInfo } from "../lib/types";

interface Props {
  onSubmit: (data: CreateSessionData) => void;
  onCancel: () => void;
}

export interface CreateSessionData {
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

export function CreateSessionPanel({ onSubmit, onCancel }: Props) {
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

  useEffect(() => {
    fetchAgents().then((a) => {
      setAgents(a);
      if (a.length > 0) setTool(a[0].name);
    });
  }, []);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!path.trim()) return;
    onSubmit({
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
  };

  return (
    <div className="fixed inset-0 bg-black/50 flex justify-end z-50">
      <form
        onSubmit={handleSubmit}
        className="w-panel max-w-full bg-surface-800 border-l border-surface-700 h-full overflow-y-auto shadow-2xl"
      >
        {/* Header */}
        <div className="flex items-center justify-between px-5 py-4 border-b border-surface-700">
          <h2 className="font-display text-sm font-semibold text-text-bright">
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

        <div className="p-5 space-y-4">
          {/* Project Path -- required */}
          <label className="block">
            <span className="font-mono text-label uppercase tracking-wider text-text-muted block mb-1">
              Project Path *
            </span>
            <input
              type="text"
              value={path}
              onChange={(e) => setPath(e.target.value)}
              autoFocus
              placeholder="/path/to/your/project"
              className="w-full bg-surface-900 border border-surface-700 rounded px-3 py-2 font-mono text-sm text-text-primary placeholder:text-text-dim focus:border-brand-600 focus:outline-none"
            />
          </label>

          {/* Agent Tool */}
          <label className="block">
            <span className="font-mono text-label uppercase tracking-wider text-text-muted block mb-1">
              Agent
            </span>
            <div className="grid grid-cols-3 gap-1.5">
              {agents.map((a) => (
                <button
                  key={a.name}
                  type="button"
                  onClick={() => setTool(a.name)}
                  className={`px-2 py-1.5 rounded text-xs font-body cursor-pointer transition-colors ${
                    tool === a.name
                      ? "bg-brand-600/20 text-brand-500 border border-brand-600/40"
                      : "bg-surface-900 text-text-secondary border border-surface-700 hover:border-surface-700/80"
                  }`}
                >
                  {a.name}
                </button>
              ))}
            </div>
          </label>

          {/* Title */}
          <label className="block">
            <span className="font-mono text-label uppercase tracking-wider text-text-muted block mb-1">
              Title
              <span className="text-text-dim ml-1 normal-case tracking-normal">
                (auto-generated if empty)
              </span>
            </span>
            <input
              type="text"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              placeholder="My Session"
              className="w-full bg-surface-900 border border-surface-700 rounded px-3 py-1.5 font-body text-sm text-text-primary placeholder:text-text-dim focus:border-brand-600 focus:outline-none"
            />
          </label>

          {/* Group */}
          <label className="block">
            <span className="font-mono text-label uppercase tracking-wider text-text-muted block mb-1">
              Group
            </span>
            <input
              type="text"
              value={group}
              onChange={(e) => setGroup(e.target.value)}
              placeholder="work/projects"
              className="w-full bg-surface-900 border border-surface-700 rounded px-3 py-1.5 font-body text-sm text-text-primary placeholder:text-text-dim focus:border-brand-600 focus:outline-none"
            />
          </label>

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
              <span className="font-body text-xs text-text-secondary">Sandbox</span>
            </label>
          </div>

          {/* Advanced */}
          <button
            type="button"
            onClick={() => setShowAdvanced(!showAdvanced)}
            className="font-body text-xs text-text-muted hover:text-text-secondary cursor-pointer"
          >
            {showAdvanced ? "Hide" : "Show"} advanced options
          </button>

          {showAdvanced && (
            <div className="space-y-3 border-t border-surface-700 pt-3">
              <label className="block">
                <span className="font-mono text-label uppercase tracking-wider text-text-muted block mb-1">
                  Worktree Branch
                </span>
                <input
                  type="text"
                  value={branch}
                  onChange={(e) => setBranch(e.target.value)}
                  placeholder="feature/my-branch"
                  className="w-full bg-surface-900 border border-surface-700 rounded px-3 py-1.5 font-body text-sm text-text-primary placeholder:text-text-dim focus:border-brand-600 focus:outline-none"
                />
              </label>
              {branch && (
                <label className="flex items-center gap-2 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={newBranch}
                    onChange={(e) => setNewBranch(e.target.checked)}
                    className="accent-brand-600"
                  />
                  <span className="font-body text-xs text-text-secondary">
                    Create new branch
                  </span>
                </label>
              )}
              <label className="block">
                <span className="font-mono text-label uppercase tracking-wider text-text-muted block mb-1">
                  Extra Args
                </span>
                <input
                  type="text"
                  value={extraArgs}
                  onChange={(e) => setExtraArgs(e.target.value)}
                  placeholder="--resume abc123"
                  className="w-full bg-surface-900 border border-surface-700 rounded px-3 py-1.5 font-mono text-sm text-text-primary placeholder:text-text-dim focus:border-brand-600 focus:outline-none"
                />
              </label>
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="sticky bottom-0 flex justify-end gap-2 px-5 py-4 border-t border-surface-700 bg-surface-800">
          <button
            type="button"
            onClick={onCancel}
            className="px-4 py-2 font-body text-xs rounded-md text-text-secondary hover:bg-surface-700 transition-colors cursor-pointer"
          >
            Cancel
          </button>
          <button
            type="submit"
            disabled={!path.trim()}
            className="px-4 py-2 font-body text-xs rounded-md bg-brand-600 text-white hover:bg-brand-700 transition-colors cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed"
          >
            Create Session
          </button>
        </div>
      </form>
    </div>
  );
}
