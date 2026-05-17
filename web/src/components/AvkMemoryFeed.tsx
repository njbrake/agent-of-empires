/**
 * AVK memory recall feed widget — FUR-4118.
 *
 * `GET /api/avk/memory-recall` server endpoint'inden son 24 saat canon
 * kayıtları (lesson, signal, recall) çeker; tier badge + role + tags +
 * preview ile feed render eder. Mobile-first (1 col), desktop 2 col grid.
 *
 * Mock implementation şu an; gerçek agentmemory MCP proxy eklendiğinde
 * UI değişikliği yok (server contract aynı).
 *
 * Tier renkleri:
 *   - core     → status-running (yeşil) — kalıcı kanon
 *   - working  → status-waiting (sarı)  — aktif iş
 *   - archival → text-muted    (gri)    — referans/eski
 */

import { useEffect, useState } from "react";
import { fetchAvkMemoryRecall } from "../lib/api";
import type { AvkMemoryEntry, AvkMemoryTier } from "../lib/types";

const REFRESH_INTERVAL_MS = 60_000;

const TIER_LABEL: Record<AvkMemoryTier, string> = {
  core: "Çekirdek",
  working: "Aktif",
  archival: "Arşiv",
};

const TIER_BADGE_CLASS: Record<AvkMemoryTier, string> = {
  core: "bg-status-running/15 text-status-running",
  working: "bg-status-waiting/15 text-status-waiting",
  archival: "bg-surface-700 text-text-muted",
};

function formatRelativeTime(iso: string): string {
  const then = new Date(iso).getTime();
  if (Number.isNaN(then)) return iso;
  const now = Date.now();
  const diffMin = Math.floor((now - then) / 60_000);
  if (diffMin < 1) return "az önce";
  if (diffMin < 60) return `${diffMin}dk önce`;
  const diffHours = Math.floor(diffMin / 60);
  if (diffHours < 24) return `${diffHours}sa önce`;
  const diffDays = Math.floor(diffHours / 24);
  return `${diffDays}g önce`;
}

export function AvkMemoryFeed() {
  const [entries, setEntries] = useState<AvkMemoryEntry[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;

    async function load() {
      const result = await fetchAvkMemoryRecall();
      if (!cancelled) {
        setEntries(result);
        setLoading(false);
      }
    }

    load();
    const id = setInterval(load, REFRESH_INTERVAL_MS);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, []);

  if (loading) {
    return (
      <div>
        <h3 className="font-mono text-sm uppercase tracking-widest text-text-muted mb-4">
          AVK Hafıza
        </h3>
        <p className="font-body text-[14px] text-text-muted">Yükleniyor…</p>
      </div>
    );
  }

  if (entries.length === 0) {
    return (
      <div>
        <h3 className="font-mono text-sm uppercase tracking-widest text-text-muted mb-4">
          AVK Hafıza
        </h3>
        <p className="font-body text-[14px] text-text-muted">
          Hafıza kaydı yok (sunucu `/api/avk/memory-recall` boş döndü).
        </p>
      </div>
    );
  }

  return (
    <div>
      <h3 className="font-mono text-sm uppercase tracking-widest text-text-muted mb-4">
        AVK Hafıza ({entries.length})
      </h3>
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-3">
        {entries.map((entry) => (
          <article
            key={entry.id}
            className="rounded border border-surface-700 bg-surface-800 p-4"
          >
            <div className="flex items-start justify-between gap-3 mb-2">
              <h4 className="font-body text-[14px] font-medium leading-tight">
                {entry.title}
              </h4>
              <span
                className={`font-mono text-[10px] uppercase tracking-wider px-2 py-0.5 rounded shrink-0 ${TIER_BADGE_CLASS[entry.tier]}`}
              >
                {TIER_LABEL[entry.tier]}
              </span>
            </div>
            <p className="font-body text-[13px] text-text-secondary mb-3 leading-relaxed">
              {entry.content_preview}
            </p>
            <div className="flex items-center justify-between font-mono text-[11px] text-text-muted">
              <div className="flex items-center gap-2 flex-wrap">
                <span className="font-medium">{entry.role}</span>
                {entry.tags.slice(0, 3).map((tag) => (
                  <span key={tag} className="opacity-70">
                    #{tag}
                  </span>
                ))}
              </div>
              <span title={entry.created_at}>{formatRelativeTime(entry.created_at)}</span>
            </div>
          </article>
        ))}
      </div>
    </div>
  );
}
