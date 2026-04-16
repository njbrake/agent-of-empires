import { useEffect, useMemo, useRef, useState } from "react";
import { browseFilesystem, fetchSessions } from "../../../lib/api";
import type { SessionResponse } from "../../../lib/types";

interface WizardData {
  path: string;
  [key: string]: unknown;
}

interface Props {
  data: WizardData;
  onChange: (field: string, value: unknown) => void;
}

interface RecentProject {
  path: string;
  displayName: string;
  lastAccessedAt: string | null;
}

function collectRecentProjects(sessions: SessionResponse[]): RecentProject[] {
  const map = new Map<string, RecentProject>();
  for (const s of sessions) {
    const path = s.main_repo_path || s.project_path;
    if (!path) continue;
    const existing = map.get(path);
    const ts = s.last_accessed_at ?? s.created_at ?? null;
    if (existing) {
      if ((ts ?? "") > (existing.lastAccessedAt ?? "")) {
        existing.lastAccessedAt = ts;
      }
    } else {
      map.set(path, {
        path,
        displayName: path.split("/").filter(Boolean).pop() || path,
        lastAccessedAt: ts,
      });
    }
  }
  return Array.from(map.values()).sort(
    (a, b) => (b.lastAccessedAt ?? "").localeCompare(a.lastAccessedAt ?? ""),
  );
}

export function ProjectStep({ data, onChange }: Props) {
  const [pathSuggestions, setPathSuggestions] = useState<string[]>([]);
  const [showPathSuggestions, setShowPathSuggestions] = useState(false);
  const [recent, setRecent] = useState<RecentProject[]>([]);
  const debounceRef = useRef<ReturnType<typeof setTimeout>>(undefined);

  useEffect(() => {
    fetchSessions().then((s) => {
      if (s) setRecent(collectRecentProjects(s).slice(0, 6));
    });
  }, []);

  useEffect(() => {
    if (!data.path) return;
    clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(async () => {
      const entries = await browseFilesystem(data.path);
      setPathSuggestions(entries.map((e) => e.path));
    }, 300);
    return () => clearTimeout(debounceRef.current);
  }, [data.path]);

  const filteredRecent = useMemo(() => {
    if (!data.path) return recent;
    const q = data.path.toLowerCase();
    return recent.filter(
      (r) => r.path.toLowerCase().includes(q) || r.displayName.toLowerCase().includes(q),
    );
  }, [recent, data.path]);

  return (
    <div>
      <h2 className="text-lg font-semibold text-text-primary mb-1">Project folder</h2>
      <p className="text-sm text-text-muted mb-5">
        Enter the path to the project where the agent will work.
      </p>

      <div className="relative">
        <label className="block text-sm text-text-dim mb-1.5">Path</label>
        <input
          type="text"
          value={data.path}
          onChange={(e) => { onChange("path", e.target.value); setShowPathSuggestions(true); }}
          onBlur={() => setTimeout(() => setShowPathSuggestions(false), 200)}
          placeholder="/path/to/your/project"
          autoFocus
          className="w-full bg-surface-900 border border-surface-700 rounded-lg px-3 py-2.5 text-base font-mono text-text-primary placeholder:text-text-dim focus:border-brand-600 focus:outline-none"
        />
        {showPathSuggestions && pathSuggestions.length > 0 && (
          <div className="absolute z-10 w-full mt-1 bg-surface-800 border border-surface-700 rounded-lg max-h-48 overflow-y-auto">
            {pathSuggestions.slice(0, 10).map((s) => (
              <button key={s} onMouseDown={() => { onChange("path", s); setShowPathSuggestions(false); }}
                className="w-full text-left px-3 py-2.5 text-sm font-mono text-text-secondary hover:bg-surface-700 cursor-pointer">{s}</button>
            ))}
          </div>
        )}
      </div>

      {filteredRecent.length > 0 && (
        <div className="mt-6">
          <p className="font-mono text-[11px] uppercase tracking-wider text-text-muted mb-2">
            Recent projects
          </p>
          <div className="flex flex-col gap-1">
            {filteredRecent.map((r) => (
              <button
                key={r.path}
                type="button"
                onClick={() => onChange("path", r.path)}
                className="flex items-center justify-between gap-3 px-3 py-2 rounded-md border border-surface-700/40 bg-surface-900 hover:border-surface-700 hover:bg-surface-850 cursor-pointer text-left transition-colors"
              >
                <span className="text-sm text-text-primary truncate">{r.displayName}</span>
                <span className="font-mono text-[11px] text-text-dim truncate max-w-[55%]">
                  {r.path}
                </span>
              </button>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
