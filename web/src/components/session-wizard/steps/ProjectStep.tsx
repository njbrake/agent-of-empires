import { useEffect, useMemo, useState } from "react";
import { fetchSessions } from "../../../lib/api";
import type { SessionResponse } from "../../../lib/types";
import { DirectoryBrowser } from "../../DirectoryBrowser";

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
  tool: string;
  sessionCount: number;
}

function collectRecentProjects(sessions: SessionResponse[]): RecentProject[] {
  const map = new Map<string, RecentProject>();
  for (const s of sessions) {
    const path = s.main_repo_path || s.project_path;
    if (!path) continue;
    const existing = map.get(path);
    const ts = s.last_accessed_at ?? s.created_at ?? null;
    if (existing) {
      existing.sessionCount++;
      if ((ts ?? "") > (existing.lastAccessedAt ?? "")) {
        existing.lastAccessedAt = ts;
        existing.tool = s.tool;
      }
    } else {
      map.set(path, {
        path,
        displayName: path.split("/").filter(Boolean).pop() || path,
        lastAccessedAt: ts,
        tool: s.tool,
        sessionCount: 1,
      });
    }
  }
  return Array.from(map.values()).sort(
    (a, b) => (b.lastAccessedAt ?? "").localeCompare(a.lastAccessedAt ?? ""),
  );
}

function timeAgo(ts: string | null): string {
  if (!ts) return "";
  const diff = Date.now() - new Date(ts).getTime();
  const mins = Math.floor(diff / 60000);
  if (mins < 1) return "just now";
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

export function ProjectStep({ data, onChange }: Props) {
  const [recent, setRecent] = useState<RecentProject[]>([]);
  const [loading, setLoading] = useState(true);
  const [showBrowser, setShowBrowser] = useState(false);

  useEffect(() => {
    fetchSessions().then((s) => {
      if (s) setRecent(collectRecentProjects(s).slice(0, 6));
      setLoading(false);
    });
  }, []);

  const filteredRecent = useMemo(() => {
    if (!data.path) return recent;
    const q = data.path.toLowerCase();
    return recent.filter(
      (r) => r.path.toLowerCase().includes(q) || r.displayName.toLowerCase().includes(q),
    );
  }, [recent, data.path]);

  const hasRecents = recent.length > 0;
  // Adaptive: show recents as hero when they exist, browser as hero when empty
  const browserIsHero = !loading && !hasRecents;

  const handleBrowseSelect = (path: string) => {
    onChange("path", path);
    setShowBrowser(false);
  };

  return (
    <div>
      <h2 className="text-lg font-semibold text-text-primary mb-1">Project folder</h2>
      <p className="text-sm text-text-muted mb-5">
        {hasRecents ? "Pick a recent project or browse for a new one." : "Browse to select your project folder."}
      </p>

      {/* Loading skeleton */}
      {loading && (
        <div className="animate-pulse space-y-2">
          {[...Array(3)].map((_, i) => (
            <div key={i} className="h-[60px] bg-surface-900 border border-surface-700/40 rounded-md" />
          ))}
        </div>
      )}

      {/* Recent projects (hero when available) */}
      {!loading && hasRecents && !showBrowser && (
        <>
          <div className="flex flex-col gap-1.5 mb-4">
            {filteredRecent.map((r) => (
              <button
                key={r.path}
                type="button"
                onClick={() => onChange("path", r.path)}
                className={`flex items-center gap-3 px-3 py-2.5 rounded-md border transition-colors text-left cursor-pointer ${
                  data.path === r.path
                    ? "border-brand-600 bg-surface-900"
                    : "border-surface-700/40 bg-surface-900 hover:border-surface-700 hover:bg-surface-850"
                }`}
              >
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <span className="text-sm font-medium text-text-primary truncate">{r.displayName}</span>
                    <span className="text-[10px] font-mono text-text-dim shrink-0">{r.tool}</span>
                  </div>
                  <div className="flex items-center gap-2 mt-0.5">
                    <span className="font-mono text-[11px] text-text-dim truncate">{r.path}</span>
                  </div>
                </div>
                <div className="flex flex-col items-end shrink-0 gap-0.5">
                  <span className="text-[10px] text-text-dim">{timeAgo(r.lastAccessedAt)}</span>
                  <span className="text-[10px] text-text-dim">{r.sessionCount} session{r.sessionCount !== 1 ? "s" : ""}</span>
                </div>
              </button>
            ))}
          </div>

          <button
            onClick={() => setShowBrowser(true)}
            className="w-full py-2.5 text-sm text-text-dim hover:text-text-secondary border border-dashed border-surface-700 rounded-md hover:border-surface-600 cursor-pointer transition-colors"
          >
            Browse for a different project...
          </button>
        </>
      )}

      {/* Directory browser (hero when no recents, or toggled from "Browse" button) */}
      {!loading && (browserIsHero || showBrowser) && (
        <>
          {showBrowser && hasRecents && (
            <button
              onClick={() => setShowBrowser(false)}
              className="text-sm text-text-dim hover:text-text-secondary cursor-pointer mb-3 flex items-center gap-1"
            >
              &larr; Back to recent projects
            </button>
          )}
          <DirectoryBrowser onSelect={handleBrowseSelect} />
        </>
      )}

      {/* Selected path display */}
      {data.path && !showBrowser && (
        <div className="mt-4 px-3 py-2 bg-surface-900 border border-brand-600/30 rounded-md">
          <p className="text-[10px] font-mono uppercase tracking-wider text-text-dim mb-1">Selected project</p>
          <p className="text-sm font-mono text-text-primary truncate">{data.path}</p>
        </div>
      )}
    </div>
  );
}
